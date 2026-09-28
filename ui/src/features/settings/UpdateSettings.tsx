// Settings › General › Updates: the installed version, a manual check, and
// the update put off with "Later", ready to install.
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Spinner } from "@/components/tv/Feedback";
import { Pill } from "@/components/tv/Page";
import { InfoRow, LinkRow, SettingsGroup, ToggleRow } from "@/components/tv/SettingsList";
import type { Settings } from "@/ipc/bindings/Settings";
import { updateSettings } from "@/lib/settings";
import { ago, checkForUpdates, openUpdatePrompt, useUpdates } from "@/lib/updates";

/** Re-renders every minute, for "checked 5 minutes ago". */
function useMinuteTick() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const t = window.setInterval(() => setTick((n) => n + 1), 60_000);
    return () => window.clearInterval(t);
  }, []);
}

export function UpdateSettings({ s, version }: { s: Settings; version: string | undefined }) {
  const { status, update, checkedAt } = useUpdates();
  useMinuteTick();
  const checked = checkedAt ? `Checked ${ago(checkedAt)}` : null;

  const hint = (() => {
    switch (status.phase) {
      case "checking":
        return "Looking for a new version…";
      case "upToDate":
        return `Flick is up to date. ${checked}.`;
      case "available":
      case "downloading":
      case "installing":
        return update ? `Flick ${update.version} is available. ${checked}.` : checked;
      case "error":
        return status.during === "check" ? `Could not check: ${status.message}` : `The last install failed: ${status.message}`;
      case "idle":
        return "New versions come from Flick's releases on GitHub.";
    }
  })();

  const check = async () => {
    const found = await checkForUpdates({ prompt: true });
    const now = useUpdates.getState().status;
    if (!found && now.phase === "upToDate") toast.success("Flick is up to date");
  };

  return (
    <SettingsGroup
      title="Updates"
      note={<p>Updates are signed by the Flick project and verified before they are installed. Flick restarts to finish.</p>}
    >
      <InfoRow label="Installed version">{version ? `Flick ${version}` : "—"}</InfoRow>
      {update && (
        <LinkRow
          label={`Install Flick ${update.version}`}
          hint="See what's new, then update and restart."
          value={<Pill tone="strong">New</Pill>}
          onClick={openUpdatePrompt}
        />
      )}
      <LinkRow
        label="Check for Updates"
        hint={hint}
        value={status.phase === "checking" ? <Spinner className="[&_svg]:size-4" /> : undefined}
        onClick={() => void check()}
      />
      <ToggleRow
        label="Check at launch"
        hint="Offer new versions when Flick starts."
        checked={s.general.checkForUpdates}
        onChange={(v) => updateSettings((x) => (x.general.checkForUpdates = v))}
      />
    </SettingsGroup>
  );
}
