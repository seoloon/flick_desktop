# FlickDD: client integration guide

For the developer of the Flick client. FlickDD lets the app download a movie or an episode **through Flick Server**,
in resumable, throttled segments, so that it can be watched offline. Examples are TypeScript-like pseudocode plus raw
HTTP. Every status, code, header and default below comes from the implementation (`src/api/downloads.rs`,
`src/dd/*`); where this guide and the code disagree, the code wins.

**Contents**
1. [What FlickDD is](#1-what-flickdd-is)
2. [Prerequisites](#2-prerequisites)
3. [API reference](#3-api-reference)
4. [The resumable download algorithm](#4-the-resumable-download-algorithm)
5. [Guard rails for unstable connections](#5-guard-rails-for-unstable-connections)
6. [Parallelism and queueing](#6-parallelism-and-queueing)
7. [Mobile specifics](#7-mobile-specifics)
8. [Cancelling and cleanup](#8-cancelling-and-cleanup)
9. [Offline playback notes](#9-offline-playback-notes)
10. [Checklist and reference implementation](#10-checklist-and-reference-implementation)
11. [Testing against a real server](#11-testing-against-a-real-server)

---

## 1. What FlickDD is

FlickSync never touches media bytes. **FlickDD does**: the file travels from Jellyfin or Plex to Flick Server and on
to the app, at most 10 MB/s per download, so the operator's uplink is shared fairly. The app never talks to Jellyfin
or Plex for a download and never needs their credentials: the operator configured them on the server.

A download is a **grant**: a server-side record bound to one file (name, size, ETag frozen at creation), one user and
a random **download token**. The grant holds one of the user's download slots until it completes, is cancelled or
expires.

```
 App                                   Flick Server                      Jellyfin / Plex
  |  POST /api/v1/downloads  (Flick JWT)     |                                  |
  |----------------------------------------->|  resolve item: size, name, ETag  |
  |<-- 201 {download_id, token, size,        |--------------------------------->|
  |         etag, chunk_bytes, ...}          |                                  |
  |                                          |                                  |
  |  GET .../file  Range: bytes=0-8388607    |                                  |
  |  Authorization: Bearer <download token>  |  open [0, 8 MiB) upstream        |
  |<-- 206 + bytes (throttled) --------------|<---------------------------------|
  |  GET .../file  Range: bytes=8388608-...  |   (repeat; any cut -> resume     |
  |<-- 206 + bytes --------------------------|    from the local offset)        |
  |  ... last segment: every byte served ... |                                  |
  |  grant completes, slot freed             |                                  |
  |  DELETE /api/v1/downloads/{id}  (idempotent cleanup)                        |
```

## 2. Prerequisites

* **A Flick JWT with the `downloads:create` permission.** It is the same kind of token as for FlickSync (HS256,
  `aud: flicksync`, issued by the Flick Server that owns the key; see
  [flick-integration.md](flick-integration.md)). Only `POST /api/v1/downloads` needs it. A token without the
  permission gets `403 FORBIDDEN`.
* **The server address.** The same invitation link as FlickSync gives it (see
  [client-invitation.md](client-invitation.md)); the REST API lives under `/api/v1/` on that address. FlickDD must
  also be switched on by the operator (`FLICKDD_ENABLED=true`): when it is not, **every** FlickDD route answers
  `404 DOWNLOAD_NOT_FOUND` ("FlickDD is disabled"). Treat a `404` on the very first create as "downloads are
  unavailable on this server".
* **The item.** `backend` (`jellyfin` or `plex`) and the backend's own `item_id` of the movie or episode. The server
  looks the file up with the credentials it holds.

## 3. API reference

All bodies are JSON. Errors always have this shape (the `code` is stable, the `message` is for humans):

```json
{ "error": { "code": "TOO_MANY_DOWNLOADS", "message": "at most 10 downloads in progress" } }
```

### 3.1 `POST /api/v1/downloads`: create a grant

Headers: `Authorization: Bearer <Flick JWT>`, `Content-Type: application/json`.

```json
{ "backend": "jellyfin", "item_id": "a1b2c3d4", "title": "The Long Night", "kind": "movie" }
```

* `backend`: `jellyfin` | `plex`. `item_id`: 1 to 64 characters of `A-Z a-z 0-9 _ -`. `title` (optional) is trimmed and
  cut to 200 characters; `kind` (optional) is `movie` | `episode`. Both are only labels for the operator's panel.

Response `201 Created`:

```json
{
  "url": "/api/v1/downloads/Qm9v...16chars/file",
  "download_id": "Qm9v...",
  "token": "x7Zk...43chars",
  "size": 4831838208,
  "filename": "The Long Night (2021).mkv",
  "mime": "video/x-matroska",
  "etag": "\"3f2a9c...32 hex...\"",
  "chunk_bytes": 8388608,
  "max_range_bytes": 67108864,
  "rate_limit_bps": 10485760,
  "expires_at": 1790981600000
}
```

* `url` is relative to the server address. `token` is shown **once**; keep it with the download (section 4).
* `etag` is a strong validator (quoted) over the item, its size and the backend's file tag. Use it verbatim in
  `If-Range`.
* `chunk_bytes` is the segment size to request. `max_range_bytes` is the most the server ever returns for one
  ranged request. `rate_limit_bps` is informative.
* `expires_at` is epoch milliseconds. A grant lives while it is used: idle for more than `FLICKDD_GRANT_TTL` (6 h) or
  older than `FLICKDD_GRANT_MAX_AGE` (24 h) and it is gone. Each request pushes the idle deadline.

### 3.2 `GET /api/v1/downloads/{id}`: status

```json
{ "download_id": "Qm9v...", "size": 4831838208, "covered": 83886080, "etag": "\"3f2a...\"", "expires_at": 1790981600000 }
```

`covered` is the number of distinct bytes the server has handed over so far (it can run slightly ahead of what the
app wrote to disk). It is a diagnostic: **your local offset is the truth**, never resume from `covered`.

### 3.3 `GET /api/v1/downloads/{id}/file`: the bytes

Also answers `HEAD` (same headers, no body, does not preempt the running stream and is not counted by the request
rate guard).

Request headers:

| Header | Meaning |
|---|---|
| `Authorization: Bearer <download token>` | The download token. Or use `?token=<download token>` (section 7). If both are present, **the header wins**. |
| `Range: bytes=<start>-<end>` or `bytes=<start>-` | One range. Multiple ranges (`a-b,c-d`) are refused with `416`. A suffix range `bytes=-N` is accepted. |
| `If-Range: <etag>` | The ETag from the create response. If it does not match exactly (weak `W/` tags and dates never match), the server **ignores `Range` and answers `200` with the whole file**. |

Behaviour:

* A valid range is clamped to the file and truncated to `max_range_bytes`. So `Range: bytes=0-` on a 4 GB file
  returns `206` with only the first 64 MiB; the real end is in `Content-Range`. A bounded range of `chunk_bytes`
  (8 MiB) is what the reference client asks for.
* No `Range` (or a malformed one, which RFC 9110 says to ignore) returns `200` with the entire file, untruncated.
  Do not rely on it: you could not resume cheaply.
* A new `GET` on a grant **preempts** the stream still running on it (the older response is cut).
* The response is throttled to `rate_limit_bps` per download (10 MiB/s by default; a 256 KiB burst is free).

Success response headers: `Content-Length` (bytes in *this* response), `Content-Range: bytes S-E/size` (206 only),
`Accept-Ranges: bytes`, `ETag`, `Content-Type` (the file's mime), `Content-Disposition: attachment; filename="..."; filename*=UTF-8''...`
(control characters and path separators are replaced; **pick the on-disk name yourself**, using the `filename` of the
create response), `Cache-Control: no-store`.

Responses: `206` (range), `200` (whole file, or `If-Range` mismatch), `416` with `Content-Range: bytes */<size>` (start
at or after the end, an empty or reversed range, or several ranges), and the errors of the table below.

```http
GET /api/v1/downloads/Qm9v.../file HTTP/1.1
Authorization: Bearer x7Zk...
Range: bytes=8388608-16777215
If-Range: "3f2a9c..."

HTTP/1.1 206 Partial Content
Content-Length: 8388608
Content-Range: bytes 8388608-16777215/4831838208
Accept-Ranges: bytes
ETag: "3f2a9c..."
```

A response can end early with fewer bytes than `Content-Length`, for example when the upstream failed beyond the
server's retry budget (`FLICKDD_UPSTREAM_RETRIES`, 2), when the client was too slow for `FLICKDD_STALL_TIMEOUT` (30 s)
or when a newer request preempted it. HTTP clients report this as a connection reset or a premature end of body. It is
**not** an error to surface: keep the bytes, resume (section 4).

### 3.4 `DELETE /api/v1/downloads/{id}`: cancel

Same token rules. `204 No Content`: the stream is closed and the slot is freed. A grant that no longer exists answers
`404`; for a cancel that means "already gone", i.e. success.

### 3.5 Errors

| Status | `code` | When | What to do |
|---|---|---|---|
| 400 | `INVALID_PAYLOAD` | Body is not the expected JSON, bad `item_id`, or **the file is empty (size 0, message "empty file")** | Fix the request. An empty file can never be downloaded: mark the item failed. |
| 401 | `UNAUTHENTICATED` | Create: missing, invalid or expired Flick JWT. Token routes: no download token sent at all | Create: refresh the Flick JWT and retry once. Token routes: bug, you did not send the token. |
| 403 | `FORBIDDEN` | The JWT lacks `downloads:create` | Stop. Hide downloads for this account. |
| 404 | `DOWNLOAD_NOT_FOUND` | Unknown id, **wrong token** (same answer on purpose), expired, completed, cancelled or **over-served** grant; on create: the item does not exist on the backend, or FlickDD is disabled | On a token route: create a new grant and resume (section 5). On create: the item is gone or downloads are off: stop. |
| 409 | `SOURCE_CHANGED` | The file changed on Jellyfin / Plex since the grant was created | Delete the partial file and restart from byte 0 with a new grant. |
| 416 | `RANGE_NOT_SATISFIABLE` | Start at or past the end, reversed range, multiple ranges | Offset at or past `size`: verify and finish. Otherwise fix the request. |
| 429 | `TOO_MANY_DOWNLOADS` | The user holds 10 grants (`FLICKDD_MAX_PARALLEL`), or the server holds 100 (`FLICKDD_MAX_GLOBAL`). Checked before the backend is asked | Wait and retry creation later. It is not a failure. No `Retry-After` is sent. |
| 429 | `RATE_LIMITED` | Token routes: more than 120 file requests per minute on one grant (`FLICKDD_MAX_REQUESTS_PER_MIN`). Create: more than 30 creation attempts per minute by the user (all devices, refused attempts included) | Wait for `Retry-After` (seconds), then retry. |
| 429 | `QUOTA_EXCEEDED` | The grant has already served its allowance (see below) | Do not retry on this grant: create a new one and resume. |
| 502 | `BACKEND_UNAVAILABLE` | Jellyfin / Plex did not answer or failed. The message is always the generic "the media backend is unavailable"; the detail stays in the server logs | Retry with backoff. |
| 503 | `BACKEND_UNAVAILABLE` | That backend is not configured on this server ("this media backend is not configured") | Stop for this backend. |
| 500 | `INTERNAL` | Unexpected server condition | Retry with backoff. |

**Over-served grants.** A grant may serve `size x FLICKDD_MAX_OVERSERVE` bytes (2 by default) in total, overlaps and
re-reads included. When it reaches that, the server cuts the running stream, **finishes the grant and frees the slot**.
The next request on it therefore gets `404 DOWNLOAD_NOT_FOUND` (a request that arrives exactly while the allowance is
spent may get one `429 QUOTA_EXCEEDED` first; the grant is gone right after). The client treats both exactly like
an expired grant: create a new one and resume from the local offset. If this happens repeatedly the app is re-reading
bytes it already has; look for a bug, the give-up counter of section 5 will eventually pause the item.

**A finished grant is a dead grant.** Once the server has handed over every byte, the grant completes and its token
stops working (`404`). Because the server counts bytes it *sent*, a connection that dies on the last segment can leave
the app a few bytes short of a grant the server already considers complete. The recovery is the normal one: `404`,
create a new grant, resume from the local offset.

## 4. The resumable download algorithm

Persist this record for every download, on disk, not in memory:

```ts
interface DownloadRecord {
  download_id: string;  token: string;      // of the current grant
  etag: string;         size: number;
  filename: string;     mime: string;
  offset: number;       // bytes safely written to tempPath
  tempPath: string;     // e.g. <dir>/<id>.part
  // plus what you need to recreate the grant: backend, item_id, title, kind
}
```

The loop:

1. Create the grant, save the record with `offset = 0` (or, after a recreate, the local `.part` size, see below).
2. While `offset < size`:
   1. Request one segment: `Range: bytes=<offset>-<min(offset + chunk_bytes, size) - 1>` and `If-Range: <etag>`,
      with the download token. (`bytes=<offset>-` is valid too, but the server then returns up to
      `max_range_bytes`, 64 MiB, which wastes work if the connection drops.)
   2. Expect `206` and check `Content-Range` starts at `offset` and ends in `/size`. A `200` means the validator did
      not match: treat it like `SOURCE_CHANGED`.
   3. Stream the body to `tempPath` at position `offset`. After **every written block**, advance `offset` by exactly
      the bytes written and save the record. Write first, save second, so the saved `offset` never exceeds the bytes
      on disk. On resume, truncate `tempPath` to `offset`.
   4. At the end of the segment, **fsync** the file (and save the record again).
   5. Compare the bytes received in this response with `Content-Length`. Fewer means the body was interrupted: it is
      just a short segment, loop again from the new `offset`. More, or a different `Content-Range` end than asked, is
      equally fine: always compute the next range from your own `offset`.
3. When `offset == size`: check that the `.part` file is exactly `size` bytes, **atomically rename** it from `.part` to
   the final name, then `DELETE` the grant (section 8) and mark the item done. If the size is wrong, delete the file
   and restart from 0.

**Resume on app restart.** At launch, load every record that is not done. For each, check that `tempPath` exists and
is at least `offset` bytes long (else restart from 0), truncate it to `offset`, and re-enter the loop with the saved
grant. If the grant is gone (`401/404`), recreate it as in section 5. You never need to re-download what is on disk.

**Recreating a grant.** The new `create` returns the file's current `size` and `etag`. If both equal the saved ones,
keep the `.part` and continue at `offset` with the **new** token and ETag. If either differs, the file changed: delete
the `.part` and start over from 0.

## 5. Guard rails for unstable connections

Phones lose Wi-Fi, trains enter tunnels, proxies cut long requests. The loop above survives all of it if you apply
these rules.

**Classify every failure.**

| Failure | Action |
|---|---|
| Network error, DNS failure, TLS failure, connection reset, body cut short | Retry the segment from the local offset (backoff) |
| Timeout, or no byte for 20 s (stall) | Cancel the request, retry (backoff) |
| `5xx` (including `502 BACKEND_UNAVAILABLE`) | Retry (backoff) |
| `429 RATE_LIMITED` | Retry after `Retry-After` |
| `429 TOO_MANY_DOWNLOADS` | Wait (backoff); this is queueing, not failing |
| `429 QUOTA_EXCEEDED`, `401`, `404` (on a token route) | Recreate the grant (counts as a failure: backoff first), resume at the local offset with `If-Range` of the **new** ETag; if the new `size` or `etag` differs from the saved ones, restart the file |
| `409 SOURCE_CHANGED` | Delete the partial, restart from 0 with a new grant (counts as a failure: backoff first); after 3 in a row without progress, mark the item failed |
| `416` | Offset is at or past the end: verify and finish |
| `403` | Stop, do not retry |

**Backoff.** Exponential with *full jitter*: `delay = random(0, min(60 s, 1 s * 2^n))`, with `n` the number of
consecutive failures. If a `Retry-After` is present, wait at least that long.

**Give up softly.** After **8 consecutive failures without progress**, stop retrying and mark the item **paused**
(not failed): the user, a connectivity change or the next app launch restarts it. **Reset the counter on any received
byte.** A download that crawls forward never gives up.

**Recreating a grant is a failure too.** A recreate after `401`, `404`, `QUOTA_EXCEEDED` or `409` increments the same
counter and waits the same backoff before the new `create`: an item whose grant dies at once (a file that keeps
changing, a validator that never matches) must not spin on `POST /api/v1/downloads`. On top of that, after **3
consecutive `SOURCE_CHANGED` without a received byte** (synthesized ones included, section 4), mark the item
**failed**: the file is not stable enough to download. Both counters reset on any received byte. The server enforces
it as well: a user gets at most 30 creation attempts a minute (`429 RATE_LIMITED` with `Retry-After` beyond).

**Wait for connectivity.** When the OS says there is no network, do not burn retries: wait for the reachability
event (or the Wi-Fi-only condition, section 7), then retry immediately and reset the backoff.

**Stall detection.** Reset a 20 s timer on every received chunk; when it fires, abort the request and treat it as a
network error. Do not rely on the OS socket timeout: it can be minutes.

**One request per grant.** Never run two `GET`s on the same grant. The server preempts the older connection when a
new one arrives, so a retry is always safe even if you are not sure the previous request is dead; but parallel
requests only cut each other, waste throttled bandwidth and burn the request rate guard.

**Respect the request rate.** A grant accepts 120 file requests a minute. At 10 MiB/s an 8 MiB segment takes about
0.8 s, i.e. roughly 75 requests a minute: fine. Do **not** shrink the segment below `chunk_bytes`: 4 MiB segments
would exceed the guard.

## 6. Parallelism and queueing

* A user may have **at most 10 active grants** (the default `FLICKDD_MAX_PARALLEL`; the server as a whole 100). A
  slot is held from the moment of creation until completion, cancel or expiry, **not** only while bytes flow. The
  user's slots are shared by all of their devices.
* Keep the rest of the queue **locally**. Create a grant only when a slot is free for it, i.e. just before the
  download starts. Do not create grants in advance.
* `429 TOO_MANY_DOWNLOADS` means another device is using the slots, or the server is busy: wait and retry the
  creation, keep the item `queued`. It is not a failure.
* The 10 MB/s cap is **per download, on the server**. Opening several grants for the same file does not make it
  faster, it only wastes slots and uplink. One file, one grant.
* A **series is one grant per episode**: enqueue each episode with its own `item_id`.
* Pausing an item should release its slot (best-effort `DELETE`), and resuming creates a fresh grant (section 5).

## 7. Mobile specifics

**The `?token=` form.** OS background downloaders (iOS `URLSession` background sessions, Android `DownloadManager` or
a WorkManager job) often cannot set per-request headers after the task is created, or can keep them only
unreliably. Give them the URL with the token in the query string:

```
https://flick.example.com/sync/api/v1/downloads/Qm9v.../file?token=x7Zk...
```

The `Range` and `If-Range` headers still do the resuming. When the app also sends `Authorization: Bearer`, the header
wins over `?token=`.

**Why this token is safe to hand over.** It is not your Flick JWT. It is a random secret (256 bits) that:

* authorizes exactly **one grant** and nothing else (status, bytes and cancel of that one download; it cannot create
  grants, join rooms or read anything else);
* stops working when the download completes, is cancelled or expires;
* is never logged by Flick Server (neither the header nor the query string), and is stored only as a hash.

Keep it out of your own logs and crash reports, and out of any analytics. Behind a reverse proxy, the operator should
not log query strings either.

**Policy suggestions.**

* Default to **Wi-Fi only**, with a setting to allow cellular, and pause (do not fail) when the condition is lost.
* Pause on **low battery** or Low Power Mode, and when the device is thermally throttled.
* Show the user honest state: `queued`, `downloading`, `paused (no network)`, `paused (retrying)`, `done`, `failed`.
* A background downloader hands you one response per task. If it ends early, schedule the next segment task from the
  local offset: the algorithm of section 4 is unchanged.

**App kill.** The record on disk is the contract (section 4). On the next launch, or when the OS wakes the app for
the finished background task, reload the records and resume. A background session may have written bytes the app
never accounted for: set `offset` from the real size of the `.part` file (rounded down to what you trust) rather than
from the last saved number.

## 8. Cancelling and cleanup

* **User cancels or deletes an unfinished download:** `DELETE /api/v1/downloads/{id}` with the download token (best
  effort, ignore `404` and network errors), then delete the `.part` file and the record. The slot is freed at once.
* **The download finished:** `DELETE` the grant too. If you resumed on a new grant the server never saw the full file
  on it, so it would hold the slot until it expires (up to 6 h idle). Cleaning up is cheap and idempotent.
* **The user removes a finished download:** nothing to tell the server. The grant is already gone; delete the file
  and the record.
* An abandoned grant expires on its own, but do not count on it for slot hygiene.

## 9. Offline playback notes

Store, next to the file: `filename`, `mime`, `size`, the **backend** and the **item id**, and the title. They let
the app list downloads without network, pick the right player settings, and link the file back to the library item.
Subtitles are out of scope: FlickDD downloads the media file only.

## 10. Checklist and reference implementation

Checklist:

- [ ] Flick JWT carries `downloads:create`; refresh it on a `401` from create.
- [ ] A `404` on the first create is "FlickDD not available on this server".
- [ ] At most 10 active grants; the rest of the queue is local; `429 TOO_MANY_DOWNLOADS` means wait.
- [ ] Segments of `chunk_bytes` with `Range` and `If-Range: <etag>`; never two requests on one grant.
- [ ] Record persisted after every written block, fsync at segment end, truncate `.part` to `offset` on resume.
- [ ] `Content-Range` and `Content-Length` checked; a short body is a normal short segment.
- [ ] Errors classified as in section 5; full-jitter backoff 1 s to 60 s; `Retry-After` honoured.
- [ ] 20 s stall detection; 8 failures without a byte pause the item; connectivity waits.
- [ ] `401/404/QUOTA_EXCEEDED` recreate the grant after a counted failure and its backoff; a changed `size` or `etag`
      restarts the file.
- [ ] `409 SOURCE_CHANGED` deletes the partial and restarts after the backoff; 3 in a row without a byte fail the item.
- [ ] Final size verified, atomic rename, `DELETE` of the grant at the end.
- [ ] The token never appears in logs.

A compact reference implementation (platform-neutral: `Fs` and `Net` are what you bind to your platform):

```ts
interface Grant { download_id: string; token: string; url: string; size: number; filename: string;
                  mime: string; etag: string; chunk_bytes: number }
interface Item { id: string; backend: "jellyfin" | "plex"; itemId: string; title?: string; kind?: "movie" | "episode";
                 state: "queued" | "active" | "paused" | "done" | "failed";
                 grant?: Grant; last?: { size: number; etag: string };   // `last`: identity of the previous grant
                 offset: number; tempPath: string; finalPath: string;
                 fails: number; changes: number }                       // both: consecutive, without a received byte
interface Store { save(i: Item): Promise<void>; load(): Promise<Item[]>; remove(id: string): Promise<void> }
interface Sink { write(b: Uint8Array): Promise<void>; sync(): Promise<void>; close(): Promise<void> }
interface Fs { open(path: string, at: number): Promise<Sink>;   // creates, truncates to `at`, positions there
               size(path: string): Promise<number>; rename(a: string, b: string): Promise<void>;
               remove(path: string): Promise<void> }
interface Net { online(): Promise<void> }                        // resolves when the network is reachable

class HttpError extends Error { constructor(public status: number, public code: string, public retryAfter?: number) { super(code); } }
const fail = async (r: Response) => { const j = await r.json().catch(() => ({}));
  return new HttpError(r.status, j?.error?.code ?? "", Number(r.headers.get("retry-after")) || undefined); };
const sleep = (ms: number) => new Promise(r => setTimeout(r, ms));
const backoff = (n: number) => Math.random() * Math.min(60_000, 1000 * 2 ** n);

class DownloadManager {
  private running = new Map<string, AbortController>();
  constructor(private base: string, private jwt: () => Promise<string>, private store: Store, private fs: Fs,
              private net: Net, private items: Item[] = [], private maxActive = 10, private giveUp = 8,
              private maxChanges = 3) {}
  async start() { this.items = await this.store.load(); this.pump(); }
  enqueue(i: Item) { this.items.push(i); this.pump(); }
  resume(id: string) { const i = this.items.find(x => x.id === id); if (i?.state === "paused") { i.state = "queued"; i.fails = 0; i.changes = 0; this.pump(); } }
  async cancel(id: string) { this.running.get(id)?.abort(); const i = this.items.find(x => x.id === id); if (!i) return;
    await this.release(i); await this.fs.remove(i.tempPath).catch(() => {}); this.items = this.items.filter(x => x !== i); await this.store.remove(id); }
  private pump() { for (const i of this.items) { if (this.running.size >= this.maxActive) return;
    if ((i.state === "queued" || i.state === "active") && !this.running.has(i.id)) { i.state = "active"; this.worker(i); } } }
  private async worker(i: Item) {
    const ctl = new AbortController(); this.running.set(i.id, ctl);
    try { while (i.state === "active" && !ctl.signal.aborted) {
      try { if (!i.grant) await this.create(i);
            if (i.offset >= i.grant!.size) { await this.finish(i); break; }
            await this.segment(i, ctl.signal); }
      catch (e) { if (ctl.signal.aborted) break; await this.onError(i, e); }
      await this.store.save(i); } }
    finally { this.running.delete(i.id); if (this.items.includes(i)) await this.store.save(i); this.pump(); }
  }
  private async create(i: Item) {                                   // a 401 here = expired Flick JWT: jwt() must refresh it; onError retries (counted)
    const r = await fetch(this.base + "/api/v1/downloads", { method: "POST", headers: { Authorization: `Bearer ${await this.jwt()}`,
      "Content-Type": "application/json" }, body: JSON.stringify({ backend: i.backend, item_id: i.itemId, title: i.title, kind: i.kind }) });
    if (!r.ok) throw await fail(r);
    const g: Grant = await r.json(), old = i.grant ?? i.last;
    if (old && (old.size !== g.size || old.etag !== g.etag)) { i.offset = 0; await this.fs.remove(i.tempPath).catch(() => {}); }
    i.grant = g; i.last = { size: g.size, etag: g.etag };
    if (i.offset > 0 && await this.fs.size(i.tempPath).catch(() => 0) < i.offset) i.offset = 0;  // lost bytes: restart
  }
  private async segment(i: Item, outer: AbortSignal) {
    const g = i.grant!, end = Math.min(i.offset + g.chunk_bytes, g.size) - 1, ctl = new AbortController();
    let timer: any; const arm = () => { clearTimeout(timer); timer = setTimeout(() => ctl.abort(), 20_000); };   // stall: 20 s
    const onAbort = () => ctl.abort(); outer.addEventListener("abort", onAbort); arm();
    try {
      const r = await fetch(this.base + g.url, { signal: ctl.signal, headers: { Authorization: `Bearer ${g.token}`,
        Range: `bytes=${i.offset}-${end}`, "If-Range": g.etag } });
      if (r.status === 200) throw new HttpError(409, "SOURCE_CHANGED");                    // If-Range mismatch
      if (r.status !== 206) throw await fail(r);
      const m = /^bytes (\d+)-(\d+)\/(\d+)$/.exec(r.headers.get("content-range") ?? "");
      if (!m || +m[1] !== i.offset || +m[3] !== g.size) throw new HttpError(409, "SOURCE_CHANGED");
      const want = Number(r.headers.get("content-length")), sink = await this.fs.open(i.tempPath, i.offset), rd = r.body!.getReader();
      let got = 0;
      try { for (;;) { const { done, value } = await rd.read(); arm(); if (done) break;
        await sink.write(value); i.offset += value.length; got += value.length; i.fails = i.changes = 0; await this.store.save(i); }   // write, then save
        await sink.sync(); } finally { await sink.close(); }
      if (got < want) throw new Error("body cut short");                                    // normal: loop resumes at i.offset
    } finally { clearTimeout(timer); outer.removeEventListener("abort", onAbort); ctl.abort(); }   // never drain an unwanted body (a 200 is the whole file)
  }
  private async onError(i: Item, e: any) {
    const s = e instanceof HttpError ? e.status : 0, code = e?.code;
    if (code === "TOO_MANY_DOWNLOADS") { await sleep(backoff(Math.min(i.fails + 3, 6))); return; }       // queueing: not a failure
    if (s === 403 || s === 400 || (s === 404 && !i.grant)) { i.state = "failed"; return; }               // 404 on create: item gone / FlickDD off
    if (s === 409) { await this.release(i); await this.fs.remove(i.tempPath).catch(() => {}); i.offset = 0; i.last = undefined;  // a synthesized 409 leaves the grant alive
      if (++i.changes >= this.maxChanges) { i.state = "failed"; return; } }                              // never stable: stop
    if (s === 401 || s === 404 || code === "QUOTA_EXCEEDED") i.grant = undefined;                      // recreate; create() compares size/etag
    if (s === 416) { if (i.grant && i.offset >= i.grant.size) return; await this.release(i); }        // at the end: finish()
    if (++i.fails >= this.giveUp) { i.state = "paused"; await this.release(i); return; }              // soft give-up; recreates count too
    await this.net.online(); await sleep(Math.max(backoff(i.fails), (e.retryAfter ?? 0) * 1000));
  }
  private async finish(i: Item) {
    if (await this.fs.size(i.tempPath) !== i.grant!.size) { i.offset = 0; await this.release(i); return; }  // wrong size: restart
    await this.fs.rename(i.tempPath, i.finalPath); i.state = "done"; await this.release(i);
  }
  private async release(i: Item) {                                  // free the slot; best effort and idempotent
    const g = i.grant; i.grant = undefined; if (!g) return;
    await fetch(this.base + g.url.replace(/\/file$/, ""), { method: "DELETE", headers: { Authorization: `Bearer ${g.token}` } }).catch(() => {});
  }
}
```

Notes on the reference: `create()` keeps the previous grant's `{size, etag}` in `last` so that a recreate after
`401/404` compares the new file identity with the old one, as section 5 requires. Every path that drops the grant
to recreate it goes through the counted failure and its backoff (`onError` falls through to `++i.fails`), so a broken
item never loops on `create`; `changes` fails the item after 3 `SOURCE_CHANGED` in a row without a received byte. `finish()` and `release()` both
tolerate a grant the server already completed (`404` is ignored). Whenever the client drops a grant that may still be
alive on the server (a synthesized `409` after a `200` or a wrong `Content-Range`, a `416`, a wrong final size), it
`release()`s it first, so the old grant does not hold one of the user's slots until it expires. `segment()` aborts its
request in `finally`: an unexpected `200` carries the whole file, and the body is not read to its end. Persist each `Item` with your `Store` (the token
inside `grant` included, in app-private storage); on `start()`, items found in state `active` are simply re-run by
`pump()`, and a stale saved grant is recovered by the `401/404` rule.

## 11. Testing against a real server

Enable FlickDD in `.env` (`FLICKDD_ENABLED=true` plus the Jellyfin or Plex URL and secret, see
[`.env.example`](../.env.example)), then mint a Flick JWT with the `downloads:create` permission. The example program
takes the permission list as its last argument (comma separated; the default is the FlickSync set only). The key is in
`$FLICKSYNC_DATA_DIR/auth_keys` as `kid:server_id:secret`:

```sh
TOKEN=$(cargo run -q --example mint_token -- main <secret> my-flick alice Alice 3600 downloads:create)
BASE=http://localhost:8787

# 1. create the grant
curl -s -X POST $BASE/api/v1/downloads -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
     -d '{"backend":"jellyfin","item_id":"<item id>","title":"Test","kind":"movie"}' | tee grant.json
DL=$(jq -r .download_id grant.json); DT=$(jq -r .token grant.json); ETAG=$(jq -r .etag grant.json)

# 2. first 1 MiB, then the next MiB, validated with If-Range
curl -s -D - -o part1.bin -H "Authorization: Bearer $DT" -H "Range: bytes=0-1048575" $BASE/api/v1/downloads/$DL/file
curl -s -D - -o part2.bin -H "Authorization: Bearer $DT" -H "Range: bytes=1048576-2097151" \
     -H "If-Range: $ETAG" $BASE/api/v1/downloads/$DL/file

# 3. same thing with the token in the query string (what background downloaders use)
curl -s -o /dev/null -w '%{http_code}\n' -H "Range: bytes=0-99" "$BASE/api/v1/downloads/$DL/file?token=$DT"

# 4. a stale validator must give 200 (whole file) instead of 206; HEAD shows it without downloading anything
curl -s -I -H "Authorization: Bearer $DT" -H "Range: bytes=0-99" -H 'If-Range: "nope"' $BASE/api/v1/downloads/$DL/file

# 5. status, then cancel
curl -s -H "Authorization: Bearer $DT" $BASE/api/v1/downloads/$DL
curl -s -o /dev/null -w '%{http_code}\n' -X DELETE -H "Authorization: Bearer $DT" $BASE/api/v1/downloads/$DL   # 204
```

Resume a whole file with curl alone: `curl -C - -o movie.mkv -H "Authorization: Bearer $DT" $BASE/api/v1/downloads/$DL/file`
re-issues `Range: bytes=<size on disk>-` after an interruption (without `If-Range`, and each response is capped at
`max_range_bytes`, so repeat it until the file is complete).

Watch it from the operator side: the panel's FlickDD page, the [admin API](admin-api.md#flickdd) and the
`flickdd_*` series of `/metrics`.
