import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Plus, Trash2 } from "lucide-react";
import { motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { ProviderLogo } from "@/components/tv/ServerBadge";
import { Switch } from "@/components/tv/Switch";
import { Notice, Spinner } from "@/components/tv/Feedback";
import { Pill } from "@/components/tv/Page";
import { TextField } from "@/components/tv/TextField";
import { TvDialog } from "@/components/tv/TvDialog";
import { api } from "@/ipc/api";
import type { PlexServerChoice, ProbeResult, ServerEntry } from "@/ipc/app-types";
import type { ServerStatus } from "@/ipc/bindings/ServerStatus";
import { enter, focusSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";
import { serversQuery } from "@/shell/navItems";
import { errorText } from "@/lib/errors";

function StatusPill({ status }: { status: ServerStatus | undefined }) {
  if (!status) return <Pill>Checking…</Pill>;
  switch (status.state) {
    case "online":
      return (
        <span className="inline-flex items-center gap-2 text-sm text-white/80">
          <span className="size-2 rounded-full bg-emerald-400 shadow-[0_0_10px_rgb(52_211_153/0.8)]" />
          Online · {status.latencyMs} ms
        </span>
      );
    case "unauthorized":
      return <Pill tone="warn">Sign in again</Pill>;
    default:
      return <Pill tone="warn">Unreachable</Pill>;
  }
}

/** Runs an async step with busy/error state. */
function useStep() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  return { busy, error, setError, run };
}

function usePoll() {
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearInterval(timer.current), []);
  return {
    start: (fn: () => void, ms: number) => {
      window.clearInterval(timer.current);
      timer.current = window.setInterval(fn, ms);
    },
    stop: () => window.clearInterval(timer.current),
  };
}

function AddJellyfin({ onDone }: { onDone: () => void }) {
  const [address, setAddress] = useState("");
  const [server, setServer] = useState<ProbeResult | null>(null);
  const [user, setUser] = useState("");
  const [password, setPassword] = useState("");
  const [quick, setQuick] = useState<string | null>(null);
  const step = useStep();
  const poll = usePoll();

  const probe = () => step.run(async () => setServer(await api.jellyfinProbe(address)));
  const login = () =>
    step.run(async () => {
      await api.jellyfinLogin(server!.url, user, password);
      onDone();
    });
  const quickConnect = () =>
    step.run(async () => {
      const url = server!.url;
      const qc = await api.jellyfinQuickConnectStart(url);
      setQuick(qc.code);
      poll.start(async () => {
        try {
          if (await api.jellyfinQuickConnectPoll(url, qc.secret)) {
            poll.stop();
            onDone();
          }
        } catch (e) {
          poll.stop();
          step.setError(errorText(e));
        }
      }, 3000);
    });

  if (!server) {
    return (
      <>
        <TextField label="Server address" value={address} onChange={setAddress} placeholder="192.168.1.20:8096 or https://jellyfin.example.com" autoFocus onEnter={probe} />
        <Button variant="primary" disabled={step.busy || !address} onClick={() => void probe()}>
          {step.busy ? "Connecting…" : "Continue"}
        </Button>
        {step.error && <Notice tone="error">{step.error}</Notice>}
      </>
    );
  }

  return (
    <>
      <div className="glass flex items-center justify-between rounded-2xl px-4 py-3">
        <span className="font-semibold">{server.name}</span>
        <span className="text-sm text-muted-foreground">Jellyfin {server.version}</span>
      </div>
      {quick ? (
        <div className="flex flex-col items-center gap-4 py-4 text-center">
          <p className="text-muted-foreground">On a device already signed in, open Quick Connect and enter</p>
          <span className="rounded-2xl bg-white px-6 py-3 font-mono text-4xl font-bold tracking-[0.3em] text-black">{quick}</span>
          <Spinner label="Waiting for approval" />
        </div>
      ) : (
        <>
          <TextField label="Username" value={user} onChange={setUser} autoFocus />
          <TextField label="Password" type="password" value={password} onChange={setPassword} onEnter={() => void login()} />
          <p className="text-[0.8125rem] leading-relaxed text-muted-foreground">
            Your password is sent once to get a sign-in token. It is never stored; the token is kept in the system keychain.
          </p>
          <div className="flex flex-wrap gap-3">
            <Button variant="primary" disabled={step.busy || !user} onClick={() => void login()}>
              Sign In
            </Button>
            <Button variant="ghost" disabled={step.busy} onClick={() => void quickConnect()}>
              Use Quick Connect
            </Button>
          </div>
        </>
      )}
      {step.error && <Notice tone="error">{step.error}</Notice>}
    </>
  );
}

function PlexChoice({ server, picked, onToggle }: { server: PlexServerChoice; picked: boolean; onToggle: () => void }) {
  const tv = useTv<HTMLButtonElement>({ focusable: !!server.url });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      role="checkbox"
      aria-checked={picked}
      disabled={!server.url}
      onClick={onToggle}
      animate={{ scale: tv.showFocus ? 1.02 : 1 }}
      transition={focusSpring}
      className={cn(
        "flex w-full cursor-pointer items-center justify-between gap-4 rounded-2xl px-4 py-3 text-left transition-colors disabled:opacity-40",
        tv.showFocus ? "bg-white text-black" : "glass hover:bg-white/12",
      )}
    >
      <span className="flex min-w-0 flex-col">
        <span className="font-semibold">{server.name}</span>
        <span className={cn("truncate text-[0.8125rem]", tv.showFocus ? "text-black/60" : "text-muted-foreground")}>
          {server.url ? `${server.owned ? "Your server" : "Shared with you"} · ${server.url}` : "No reachable address from this network"}
        </span>
      </span>
      <span className={cn("grid size-6 shrink-0 place-items-center rounded-full border", picked ? "border-transparent bg-current" : "border-current/40")}>
        {picked && <Check className={cn("size-4", tv.showFocus ? "text-white" : "text-black")} strokeWidth={3} />}
      </span>
    </motion.button>
  );
}

function AddPlex({ onDone }: { onDone: () => void }) {
  const [code, setCode] = useState<string | null>(null);
  const [choices, setChoices] = useState<PlexServerChoice[] | null>(null);
  const [picked, setPicked] = useState<string[]>([]);
  const step = useStep();
  const poll = usePoll();

  const start = () =>
    step.run(async () => {
      const pin = await api.plexPinStart();
      setCode(pin.code);
      poll.start(async () => {
        try {
          const servers = await api.plexPinPoll(pin.id);
          if (servers) {
            poll.stop();
            setChoices(servers);
            setPicked(servers.filter((s) => s.url).map((s) => s.machineId));
          }
        } catch (e) {
          poll.stop();
          step.setError(errorText(e));
        }
      }, 2500);
    });
  const add = () =>
    step.run(async () => {
      await api.plexAddServers(picked);
      onDone();
    });

  return (
    <>
      {!code && (
        <>
          <p className="text-muted-foreground">You sign in on plex.tv in your browser. Flick never sees your Plex password.</p>
          <Button variant="primary" autoFocus onClick={() => void start()}>
            Open Plex Sign-In
          </Button>
        </>
      )}
      {code && !choices && (
        <div className="flex flex-col items-center gap-4 py-4 text-center">
          <p className="text-muted-foreground">Approve the request in your browser. If asked, the code is</p>
          <span className="rounded-2xl bg-white px-6 py-3 font-mono text-4xl font-bold tracking-[0.3em] text-black">{code}</span>
          <Spinner label="Waiting for Plex" />
        </div>
      )}
      {choices && (
        <>
          <p className="text-white/85">Choose the servers to add:</p>
          <div className="flex flex-col gap-2">
            {choices.map((s) => (
              <PlexChoice
                key={s.machineId}
                server={s}
                picked={picked.includes(s.machineId)}
                onToggle={() => setPicked((p) => (p.includes(s.machineId) ? p.filter((x) => x !== s.machineId) : [...p, s.machineId]))}
              />
            ))}
          </div>
          <Button variant="primary" disabled={step.busy || !picked.length} onClick={() => void add()}>
            Add {picked.length} Server{picked.length === 1 ? "" : "s"}
          </Button>
        </>
      )}
      {step.error && <Notice tone="error">{step.error}</Notice>}
    </>
  );
}

function ServerCard({ entry, status, onRemove, onToggle, index }: { entry: ServerEntry; status: ServerStatus | undefined; onRemove: () => void; onToggle: (enabled: boolean) => void; index: number }) {
  const s = entry.server;
  const off = s.disabled;
  return (
    <motion.div
      initial={{ opacity: 0, y: 16 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ ...enter, delay: index * 0.05 }}
      className="glass flex flex-wrap items-center gap-x-8 gap-y-4 rounded-3xl p-6"
    >
      <motion.span animate={{ opacity: off ? 0.45 : 1, filter: off ? "grayscale(1)" : "grayscale(0)" }} transition={{ duration: 0.35 }} className="grid size-14 shrink-0 place-items-center rounded-2xl bg-white/10">
        <ProviderLogo kind={s.kind} className="size-7" />
      </motion.span>
      <motion.div animate={{ opacity: off ? 0.5 : 1 }} transition={{ duration: 0.35 }} className="flex min-w-0 flex-1 flex-col gap-1">
        <h2 className="text-xl font-semibold">{s.name}</h2>
        <p className="text-sm text-muted-foreground">
          {s.kind === "plex" ? "Plex" : "Jellyfin"} {s.version ?? ""} · signed in as {s.user.name}
          {s.user.isAdmin ? " (administrator)" : ""}
        </p>
        <p className="truncate font-mono text-xs text-white/40">{s.baseUrl}</p>
      </motion.div>
      {off ? <Pill>Off</Pill> : entry.connected ? <StatusPill status={status} /> : <Pill tone="warn">Sign in again</Pill>}
      <Switch checked={!off} onChange={onToggle} label={off ? `Turn on ${s.name}` : `Turn off ${s.name}`} />
      <Button variant="danger" size="sm" icon={Trash2} onClick={onRemove}>
        Remove
      </Button>
    </motion.div>
  );
}

/** Servers of the active profile, with add / turn off / remove. Lives in
 * Settings › Servers. */
export function ServerManager() {
  const queryClient = useQueryClient();
  const servers = useQuery(serversQuery);
  const [adding, setAdding] = useState<null | "jellyfin" | "plex">(null);
  const connected = (servers.data ?? []).filter((s) => s.connected);
  const statuses = useQueries({
    queries: connected.map((s) => ({ queryKey: ["server-status", s.server.id], queryFn: () => api.serverStatus(s.server.id), staleTime: 15_000 })),
  });
  const statusOf = (id: string) => statuses[connected.findIndex((s) => s.server.id === id)]?.data;

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ["servers"] });
    void queryClient.invalidateQueries({ queryKey: ["libraries"] });
    void queryClient.invalidateQueries({ queryKey: ["home"] });
    void queryClient.invalidateQueries({ queryKey: ["search"] });
    void queryClient.invalidateQueries({ queryKey: ["favorites"] });
  };
  // Optimistic: the card flips at once; the catalogue follows once Rust has
  // connected or dropped the server.
  const toggle = (id: string, enabled: boolean) => {
    queryClient.setQueryData<ServerEntry[]>(["servers"], (list) => list?.map((e) => (e.server.id === id ? { ...e, server: { ...e.server, disabled: !enabled } } : e)));
    api.serverSetEnabled(id, enabled).then(refresh, (e) => {
      toast.error(errorText(e));
      refresh();
    });
  };
  const done = () => {
    setAdding(null);
    refresh();
  };

  return (
    <>
      <FocusGroup className="flex flex-wrap gap-3">
        <Button variant="primary" icon={Plus} onClick={() => setAdding("jellyfin")}>
          Add Jellyfin Server
        </Button>
        <Button icon={Plus} onClick={() => setAdding("plex")}>
          Add Plex Account
        </Button>
      </FocusGroup>
      {!servers.data ? (
        <Spinner />
      ) : servers.data.length === 0 ? (
        <p className="text-lg text-muted-foreground">No servers yet.</p>
      ) : (
        <FocusGroup className="flex flex-col gap-4">
          {servers.data.map((s, i) => (
            <ServerCard
              key={s.server.id}
              index={i}
              entry={s}
              status={statusOf(s.server.id)}
              onToggle={(enabled) => toggle(s.server.id, enabled)}
              onRemove={() => void api.serverRemove(s.server.id).then(refresh, (e) => toast.error(errorText(e)))}
            />
          ))}
        </FocusGroup>
      )}
      <TvDialog open={adding === "jellyfin"} onClose={() => setAdding(null)} title="Add a Jellyfin Server">
        <AddJellyfin onDone={done} />
      </TvDialog>
      <TvDialog open={adding === "plex"} onClose={() => setAdding(null)} title="Add Plex Servers">
        <AddPlex onDone={done} />
      </TvDialog>
    </>
  );
}
