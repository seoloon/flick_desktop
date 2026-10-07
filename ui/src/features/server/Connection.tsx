// Settings › Flick Server › Connection. The server is set up with one invitation
// link (address + key) that goes to the OS keychain from Rust and never comes back
// to the WebView. FlickSync (watch together) and FlickDD (downloads) both use it.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Minus, X } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { Notice } from "@/components/tv/Feedback";
import { InfoRow, SettingsGroup } from "@/components/tv/SettingsList";
import { TextField } from "@/components/tv/TextField";
import { api, asError } from "@/ipc/api";
import type { DiagnosisCheck, FlickSyncDiagnosis } from "@/ipc/app-types";
import { flushSettings } from "@/lib/settings";
import { FocusGroup } from "@/nav/Focusable";
import { refreshStatus } from "@/features/watch/store";

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
export function ConnectionTest() {
  const [running, setRunning] = useState(false);
  const [report, setReport] = useState<FlickSyncDiagnosis | null>(null);

  const run = async () => {
    setRunning(true);
    try {
      await flushSettings();
      setReport(await api.flickserverDiagnose());
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
export function Invitation({ onSaved }: { onSaved: () => void }) {
  const queryClient = useQueryClient();
  const saved = useQuery({ queryKey: ["flickserver-invitation"], queryFn: () => api.flickserverInvitation() });
  const [link, setLink] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<FlickSyncDiagnosis | null>(null);

  const changed = () => {
    void queryClient.invalidateQueries({ queryKey: ["flickserver-invitation"] });
    void refreshStatus();
  };

  const connect = async () => {
    setBusy(true);
    setError(null);
    setReport(null);
    try {
      const added = await api.flickserverAddInvitation(link);
      setReport(added.report);
      if (!added.saved) {
        setError("Nothing was saved: the server didn't answer or isn't ready yet. Check the link, then try again.");
        return;
      }
      setLink("");
      toast.success(`Connected to ${added.info.address}`);
      onSaved();
      changed();
    } catch (e) {
      // A link that doesn't parse: the message is already a sentence for the user.
      setError(asError(e).message);
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    await api.flickserverClearInvitation().catch((e) => toast.error(asError(e).message));
    setReport(null);
    changed();
  };

  const info = saved.data;
  return (
    <SettingsGroup title="Invitation" note={<p>Paste the invitation link from your Flick Server's administrator: it holds the address and the key, and serves watch together and downloads alike. It is stored in the system keychain.</p>}>
      {info && <InfoRow label="Server">{info.address}{info.tls ? "" : " (not encrypted)"}</InfoRow>}
      <div className="flex flex-col gap-4 px-4 py-4">
        {info?.insecureRemote && <Notice tone="warn">This server is reached without encryption across the Internet: your sign-in tokens travel in the clear. Ask for an https link.</Notice>}
        <TextField label={info ? "Replace with a new link" : "Invitation link"} type="password" placeholder="flickserver://…" value={link} onChange={setLink} onEnter={() => link.trim() && void connect()} />
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
