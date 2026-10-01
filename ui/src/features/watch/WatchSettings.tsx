// Settings › Watch Together. The Flick Server tells Flick where FlickSync is
// and issues the tokens; the manual address + key path is for a self-hosted
// FlickSync without a Flick Server. The key goes to the OS keychain from
// Rust and never comes back to the WebView.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { Notice } from "@/components/tv/Feedback";
import { InfoRow, SettingsGroup, ToggleRow } from "@/components/tv/SettingsList";
import { TextField } from "@/components/tv/TextField";
import { api, asError } from "@/ipc/api";
import type { Settings } from "@/ipc/bindings/Settings";
import { flushSettings, updateSettings } from "@/lib/settings";
import { FocusGroup } from "@/nav/Focusable";
import { messageText, refreshStatus, useWatch } from "@/features/watch/store";

const blank = (v: string) => (v.trim() === "" ? null : v.trim());

export function WatchSettings({ s }: { s: Settings }) {
  const queryClient = useQueryClient();
  const status = useWatch((w) => w.status);
  const hasKey = useQuery({ queryKey: ["flicksync-key"], queryFn: () => api.flicksyncHasKey() });
  const [server, setServer] = useState(s.flicksync.flickServerUrl ?? "");
  const [syncUrl, setSyncUrl] = useState(s.flicksync.syncUrl ?? "");
  const [name, setName] = useState(s.flicksync.displayName ?? "");
  const [key, setKey] = useState("");
  const [error, setError] = useState<string | null>(null);

  const save = (fn: (f: Settings["flicksync"]) => void) => {
    updateSettings((x) => fn(x.flicksync));
    // Rust must hold the new value before availability is re-read.
    void flushSettings().then(refreshStatus, refreshStatus);
  };

  const saveKey = async () => {
    setError(null);
    try {
      await api.flicksyncSetKey(key);
      setKey("");
      toast.success("Key saved in the system keychain");
      void queryClient.invalidateQueries({ queryKey: ["flicksync-key"] });
      void refreshStatus();
    } catch (e) {
      setError(asError(e).message);
    }
  };
  const clearKey = async () => {
    await api.flicksyncClearKey().catch((e) => toast.error(asError(e).message));
    void queryClient.invalidateQueries({ queryKey: ["flicksync-key"] });
    void refreshStatus();
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

      <SettingsGroup title="Flick Server" note={<p>Flick finds FlickSync and signs in through your Flick Server automatically.</p>}>
        <div className="flex flex-col gap-4 px-4 py-4">
          <TextField label="Flick Server address" type="url" placeholder="https://flick.example.com" value={server} onChange={setServer} onEnter={() => save((f) => (f.flickServerUrl = blank(server)))} />
          <FocusGroup className="flex">
            <Button variant="primary" size="sm" onClick={() => save((f) => (f.flickServerUrl = blank(server)))}>
              Save
            </Button>
          </FocusGroup>
        </div>
      </SettingsGroup>

      <SettingsGroup title="Self-hosted FlickSync" note={<p>Without a Flick Server: the FlickSync address and the signing key its administrator gave you (kid:server_id:secret).</p>}>
        <div className="flex flex-col gap-4 px-4 py-4">
          <TextField label="FlickSync address" type="url" placeholder="https://sync.example.com" value={syncUrl} onChange={setSyncUrl} onEnter={() => save((f) => (f.syncUrl = blank(syncUrl)))} />
          <FocusGroup className="flex">
            <Button variant="primary" size="sm" onClick={() => save((f) => (f.syncUrl = blank(syncUrl)))}>
              Save
            </Button>
          </FocusGroup>
          {hasKey.data ? (
            <>
              <InfoRow label="Signing key">Saved</InfoRow>
              <FocusGroup className="flex">
                <Button variant="danger" size="sm" onClick={() => void clearKey()}>
                  Remove Key
                </Button>
              </FocusGroup>
            </>
          ) : (
            <>
              <TextField label="Signing key" type="password" value={key} onChange={setKey} onEnter={() => void saveKey()} />
              {error && <Notice tone="error">{error}</Notice>}
              <FocusGroup className="flex">
                <Button variant="primary" size="sm" disabled={!key.trim()} onClick={() => void saveKey()}>
                  Save Key
                </Button>
              </FocusGroup>
            </>
          )}
        </div>
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
