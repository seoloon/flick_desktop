// Settings › Flick Server › Watch Together: the switch and the name shown to others.
// The server itself is set up in the Connection tab.
import { useState } from "react";
import { Button } from "@/components/tv/Button";
import { Notice } from "@/components/tv/Feedback";
import { InfoRow, SettingsGroup, ToggleRow } from "@/components/tv/SettingsList";
import { TextField } from "@/components/tv/TextField";
import type { Settings } from "@/ipc/bindings/Settings";
import { flushSettings, updateSettings } from "@/lib/settings";
import { FocusGroup } from "@/nav/Focusable";
import { messageText, refreshStatus, useWatch } from "@/features/watch/store";

const blank = (v: string) => (v.trim() === "" ? null : v.trim());

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
      {status && !status.configured && <Notice tone="warn">Watch together needs your Flick Server: add its invitation link in the Connection tab first.</Notice>}
      <SettingsGroup
        title="Watch Together"
        note={<p>Watch the same title as friends, each playing from their own server. Only the room and the playback position are shared: never your library or your sign-ins.</p>}
      >
        <ToggleRow label="Enable watch together" checked={s.flicksync.enabled} onChange={(v) => save((f) => (f.enabled = v))} />
        <InfoRow label="Status">{!s.flicksync.enabled ? "Off" : status?.available ? "Available" : status?.configured ? (status.message ? messageText(status.message) : "Unreachable") : "Not set up"}</InfoRow>
      </SettingsGroup>

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
