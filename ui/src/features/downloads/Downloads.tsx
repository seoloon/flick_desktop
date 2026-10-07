// Settings › Flick Server › FlickDD: the queue, the folder, and what is on this
// computer. Downloads go through the Flick Server, the same server as Watch Together.
import { FolderOpen, Pause, Play, Trash2, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { Notice } from "@/components/tv/Feedback";
import { InfoRow, SettingsGroup } from "@/components/tv/SettingsList";
import { api, asError } from "@/ipc/api";
import type { DownloadItem } from "@/ipc/app-types";
import { imageUrl } from "@/ipc/images";
import { FocusGroup } from "@/nav/Focusable";
import { playPath } from "../player/route";
import { bytesText, etaText, percent, sortDownloads, stateText, totalRemainingSecs } from "./format";
import { useDownloads } from "./store";

const run = (p: Promise<unknown>) => void p.catch((e) => toast.error(asError(e).message));

/** A queued, running, paused or failed download. */
function QueueRow({ d }: { d: DownloadItem }) {
  const pct = percent(d);
  return (
    <div className="flex flex-col gap-2.5 px-4 py-3.5">
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <div className="truncate text-[0.9375rem] text-white/90">{d.title}</div>
          {d.subtitle && <div className="truncate text-sm text-muted-foreground">{d.subtitle}</div>}
        </div>
        <FocusGroup className="flex shrink-0 items-center gap-2">
          {(d.state === "active" || d.state === "queued") && <Button size="icon-sm" icon={Pause} label="Pause" onClick={() => run(api.downloadsPause(d.id))} />}
          {(d.state === "paused" || d.state === "failed") && (
            <Button size="icon-sm" icon={Play} label={d.state === "failed" ? "Try again" : "Resume"} onClick={() => run(api.downloadsResume(d.id))} />
          )}
          <Button size="icon-sm" variant="danger" icon={X} label="Cancel and delete" onClick={() => run(api.downloadsRemove(d.id))} />
        </FocusGroup>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-white/12" role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
        <div className={`h-full rounded-full ${d.state === "failed" ? "bg-red-400" : "bg-white"} transition-[width] duration-300`} style={{ width: `${pct}%` }} />
      </div>
      <div className={`text-sm ${d.state === "failed" ? "text-red-400" : "text-muted-foreground"}`}>{stateText(d)}</div>
    </div>
  );
}

/** A finished download: cover, name, size, play and delete. */
function LibraryRow({ d, localServer }: { d: DownloadItem; localServer: string }) {
  const navigate = useNavigate();
  const ref = `${localServer}:${d.id}`;
  const cover = imageUrl({ item: ref, kind: "poster", tag: "poster", blurhash: null }, "card");
  return (
    <div className="flex items-center gap-4 px-4 py-3">
      <div className="h-[4.5rem] w-12 shrink-0 overflow-hidden rounded-lg bg-white/8">
        {cover && <img src={cover} alt="" loading="lazy" className="size-full object-cover" onError={(e) => (e.currentTarget.style.display = "none")} />}
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-[0.9375rem] text-white/90">{d.title}</div>
        <div className="truncate text-sm text-muted-foreground">{[d.subtitle, d.size ? bytesText(d.size) : null].filter(Boolean).join(" · ")}</div>
        {d.missing && <div className="text-sm text-red-400">The file is no longer there</div>}
      </div>
      <FocusGroup className="flex shrink-0 items-center gap-2">
        {!d.missing && (
          <Button size="sm" icon={Play} iconFilled onClick={() => navigate(playPath(ref))}>
            Play
          </Button>
        )}
        <Button size="icon-sm" variant="danger" icon={Trash2} label="Delete" onClick={() => run(api.downloadsRemove(d.id))} />
      </FocusGroup>
    </div>
  );
}

function Storage({ directory, size }: { directory: string; size: number }) {
  // Two presses: the first arms the button, so a stray click deletes nothing.
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const t = setTimeout(() => setArmed(false), 4000);
    return () => clearTimeout(t);
  }, [armed]);
  return (
    <SettingsGroup title="Storage" note={<p>Downloads are kept in Flick's own data folder. Deleting a download frees its space at once.</p>}>
      <InfoRow label="Folder">{directory}</InfoRow>
      <InfoRow label="Used">{bytesText(size)}</InfoRow>
      <FocusGroup className="flex items-center gap-2 px-4 py-3">
        <Button size="sm" icon={FolderOpen} onClick={() => run(api.downloadsOpenFolder())}>
          Open folder
        </Button>
        <Button
          size="sm"
          variant="danger"
          icon={Trash2}
          label="Delete all downloads"
          onClick={() => {
            if (!armed) return setArmed(true);
            setArmed(false);
            run(api.downloadsClear());
          }}
        >
          {armed ? "Delete everything?" : "Clean"}
        </Button>
      </FocusGroup>
    </SettingsGroup>
  );
}

export function DownloadsSettings() {
  const items = useDownloads((s) => s.items);
  const status = useDownloads((s) => s.status);
  const all = sortDownloads(Object.values(items));
  const queue = all.filter((d) => d.state !== "done");
  const library = all.filter((d) => d.state === "done");
  const left = totalRemainingSecs(queue);
  const used = Object.values(items).reduce((sum, d) => sum + (d.state === "done" ? (d.size ?? 0) : d.offset), 0);
  return (
    <>
      {status && !status.configured && <Notice tone="warn">Downloads go through your Flick Server. Add its invitation link in the Connection tab first.</Notice>}
      <SettingsGroup
        title="FlickDD · Queue"
        note={
          <p>
            Files are fetched through your Flick Server, a piece at a time, and resume by themselves after a network drop or a restart. Open a title and use its download button; for a series, every episode is
            added.
          </p>
        }
      >
        {left != null && <InfoRow label="Time left for everything">{etaText(left)}</InfoRow>}
        {queue.length === 0 ? <div className="px-4 py-4 text-[0.9375rem] text-muted-foreground">Nothing is downloading.</div> : queue.map((d) => <QueueRow key={d.id} d={d} />)}
      </SettingsGroup>
      {status && <Storage directory={status.directory} size={used} />}
      <SettingsGroup title="Downloaded on this computer">
        {library.length === 0 || !status ? (
          <div className="px-4 py-4 text-[0.9375rem] text-muted-foreground">No downloaded titles yet.</div>
        ) : (
          library.map((d) => <LibraryRow key={d.id} d={d} localServer={status.localServer} />)
        )}
      </SettingsGroup>
    </>
  );
}
