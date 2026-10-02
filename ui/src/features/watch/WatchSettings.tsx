// Settings › Watch Together. FlickSync is set up with one invitation link
// (address + key) that goes to the OS keychain from Rust and never comes back
// to the WebView.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Minus, X } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { Notice } from "@/components/tv/Feedback";
import { InfoRow, SettingsGroup, ToggleRow } from "@/components/tv/SettingsList";
import { TextField } from "@/components/tv/TextField";
import { api, asError } from "@/ipc/api";
import type { DiagnosisCheck, FlickSyncDiagnosis } from "@/ipc/app-types";
import type { Settings } from "@/ipc/bindings/Settings";
import { flushSettings, updateSettings } from "@/lib/settings";
import { FocusGroup } from "@/nav/Focusable";
import { messageText, refreshStatus, useWatch } from "@/features/watch/store";

const blank = (v: string) => (v.trim() === "" ? null : v.trim());

const STEP_LABELS: Record<DiagnosisCheck["step"], string> = {
  config: "Settings",
  reach: "Server reachable",
  ready: "Server ready",
  clock: "Clock",
  auth: "Sign-in key",
};

const STATUS_ICON = { ok: Check, failed: X, skipped: Minus } as const;
const STATUS_TONE = { ok: "text-emerald-400", failed: "text-red-400", skipped: "text-white/40" } as const;

// Runs the saved settings, not what is typed in the fields: save first.
function ConnectionTest() {
  const [running, setRunning] = useState(false);
  const [report, setReport] = useState<FlickSyncDiagnosis | null>(null);

  const run = async () => {
    setRunning(true);
    try {
      await flushSettings();
      setReport(await api.flicksyncDiagnose());
    } catch (e) {
      toast.error(asError(e).message);
    } finally {
      setRunning(false);
    }
  };

  return (
    <SettingsGroup title="Connection" note={<p>Checks the saved settings step by step and tells you what to fix. Nothing is created on the server.</p>}>
      <div className="flex flex-col gap-4 px-4 py-4">
        <FocusGroup className="flex">
          <Button variant="primary" size="sm" disabled={running} onClick={() => void run()}>
            {running ? "Testing…" : "Test Connection"}
          </Button>
        </FocusGroup>
        {report && <DiagnosisList report={report} />}
      </div>
    </SettingsGroup>
  );
}

/** The steps of a connection test, then the one sentence that matters. */
function DiagnosisList({ report }: { report: FlickSyncDiagnosis }) {
  const failed = report.checks.find((c) => c.status === "failed");
  return (
    <>
      <ul className="flex flex-col gap-2.5">
        {report.checks.map((c) => {
          const Icon = STATUS_ICON[c.status];
          return (
            <li key={c.step} className="flex items-start gap-3 text-[0.9375rem]">
              <Icon className={`mt-0.5 size-4 shrink-0 ${STATUS_TONE[c.status]}`} />
              <div className="min-w-0">
                <div className="text-white/90">{STEP_LABELS[c.step]}</div>
                {c.status !== "ok" && <div className="text-sm leading-relaxed text-muted-foreground select-text">{c.detail}</div>}
              </div>
            </li>
          );
        })}
      </ul>
      <Notice tone={failed ? "error" : "info"}>{failed ? failed.detail : "Everything checks out. Watch together is ready to use."}</Notice>
    </>
  );
}

// The invitation link carries the server's address and its key. It is a
// secret: it goes straight to the OS keychain from Rust, and only the host
// comes back here.
function Invitation({ enabled, onSaved }: { enabled: boolean; onSaved: () => void }) {
  const queryClient = useQueryClient();
  const saved = useQuery({ queryKey: ["flicksync-invitation"], queryFn: () => api.flicksyncInvitation() });
  const [link, setLink] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<FlickSyncDiagnosis | null>(null);

  const changed = () => {
    void queryClient.invalidateQueries({ queryKey: ["flicksync-invitation"] });
    void refreshStatus();
  };

  const connect = async () => {
    setBusy(true);
    setError(null);
    setReport(null);
    try {
      const added = await api.flicksyncAddInvitation(link);
      setReport(added.report);
      if (!added.saved) {
        setError("Nothing was saved: the server didn't answer or isn't ready yet. Check the link, then try again.");
        return;
      }
      setLink("");
      toast.success(`Connected to ${added.info.host}`);
      if (!enabled) onSaved();
      changed();
    } catch (e) {
      // A link that doesn't parse: the message is already a sentence for the user.
      setError(asError(e).message);
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    await api.flicksyncClearInvitation().catch((e) => toast.error(asError(e).message));
    setReport(null);
    changed();
  };

  const info = saved.data;
  return (
    <SettingsGroup title="Invitation" note={<p>Paste the invitation link from your FlickSync server's administrator: it holds the address and the key. It is stored in the system keychain.</p>}>
      {info && <InfoRow label="Server">{info.host}{info.tls ? "" : " (not encrypted)"}</InfoRow>}
      <div className="flex flex-col gap-4 px-4 py-4">
        {info?.insecureRemote && <Notice tone="warn">This server is reached without encryption across the Internet: your sign-in tokens travel in the clear. Ask for an https link.</Notice>}
        <TextField label={info ? "Replace with a new link" : "Invitation link"} type="password" placeholder="flicksync://…" value={link} onChange={setLink} onEnter={() => link.trim() && void connect()} />
        {error && <Notice tone="error">{error}</Notice>}
        {report && <DiagnosisList report={report} />}
        <FocusGroup className="flex gap-3">
          <Button variant="primary" size="sm" disabled={!link.trim() || busy} onClick={() => void connect()}>
            {busy ? "Connecting…" : "Connect"}
          </Button>
          {info && (
            <Button variant="danger" size="sm" onClick={() => void remove()}>
              Remove Link
            </Button>
          )}
        </FocusGroup>
      </div>
    </SettingsGroup>
  );
}

export function WatchSettings({ s }: { s: Settings }) {
  const status = useWatch((w) => w.status);
  const [name, setName] = useState(s.flicksync.displayName ?? "");

  const save = (fn: (f: Settings["flicksync"]) => void) => {
    updateSettings((x) => fn(x.flicksync));
    // Rust must hold the new value before availability is re-read.
    void flushSettings().then(refreshStatus, refreshStatus);
  };

  return (
    <>
      <SettingsGroup
        title="Watch Together"
        note={<p>Watch the same title as friends, each playing from their own server. Only the room and the playback position are shared: never your library or your sign-ins.</p>}
      >
        <ToggleRow label="Enable watch together" checked={s.flicksync.enabled} onChange={(v) => save((f) => (f.enabled = v))} />
        <InfoRow label="Status">{!s.flicksync.enabled ? "Off" : status?.available ? "Available" : status?.configured ? (status.message ? messageText(status.message) : "Unreachable") : "Not set up"}</InfoRow>
      </SettingsGroup>

      <Invitation enabled={s.flicksync.enabled} onSaved={() => save((f) => (f.enabled = true))} />

      <ConnectionTest />

      <SettingsGroup title="You">
        <div className="flex flex-col gap-4 px-4 py-4">
          <TextField label="Name shown to others" placeholder="Defaults to your profile name" value={name} onChange={setName} onEnter={() => save((f) => (f.displayName = blank(name)))} />
          <FocusGroup className="flex">
            <Button variant="primary" size="sm" onClick={() => save((f) => (f.displayName = blank(name)))}>
              Save
            </Button>
          </FocusGroup>
        </div>
      </SettingsGroup>
    </>
  );
}
