// Settings › Profiles: turn multi-user on, choose where profiles come from,
// list and edit them. Changes that could get past a PIN ask for one.
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useNavigate } from "react-router";
import { toast } from "sonner";
import { AccountPills } from "@/components/tv/AccountPills";
import { Spinner } from "@/components/tv/Feedback";
import { type PinResult, PinPad } from "@/components/tv/PinPad";
import { LinkRow, SelectRow, SettingsGroup, ToggleRow } from "@/components/tv/SettingsList";
import { TvDialog } from "@/components/tv/TvDialog";
import { api } from "@/ipc/api";
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";
import type { ProfileMode } from "@/ipc/bindings/ProfileMode";
import { pinError, profilesQuery } from "@/lib/profiles";
import { queryClient } from "@/lib/queryClient";
import { loadSettings } from "@/lib/settings";
import { ProfileEditor } from "./ProfileEditor";
import { errorText } from "@/lib/errors";

const MODES: { value: ProfileMode; label: string }[] = [
  { value: "serverUsers", label: "Server users" },
  { value: "local", label: "Flick profiles" },
  { value: "linked", label: "Linked profiles" },
];

const MODE_NOTES: Record<ProfileMode, string> = {
  serverUsers: "Everyone who has an account on your servers gets a profile. The same name on several servers is one person.",
  local: "Profiles made here. Each one signs in to its own servers.",
  linked: "Profiles made here, each using some of the accounts already signed in.",
};

export function ProfilesSettings() {
  const navigate = useNavigate();
  const { data } = useQuery(profilesQuery);
  const [editing, setEditing] = useState<ProfileCard | "new" | null>(null);
  const [showHidden, setShowHidden] = useState(false);
  const [retry, setRetry] = useState<((pin: string) => Promise<PinResult>) | null>(null);

  if (!data) return <Spinner />;

  const configure = async (enabled: boolean, mode: ProfileMode, ask: boolean, pin: string | null = null): Promise<PinResult> => {
    try {
      const s = await api.profilesConfigure(enabled, mode, ask, pin);
      queryClient.setQueryData(profilesQuery.queryKey, s);
      await loadSettings();
      void queryClient.invalidateQueries();
      // On, but nobody holds what was loaded: someone has to be picked.
      if (s.enabled && !s.active) navigate("/profiles");
      return "ok";
    } catch (e) {
      const p = pinError(e);
      if (p && pin === null) {
        setRetry(() => (typed: string) => configure(enabled, mode, ask, typed));
        return "ok";
      }
      if (p) return p;
      toast.error(errorText(e));
      return "ok";
    }
  };

  const hidden = data.profiles.filter((p) => p.hidden).length;
  const listed = data.profiles.filter((p) => showHidden || !p.hidden);

  return (
    <>
      <SettingsGroup note={data.enabled ? <p>{MODE_NOTES[data.mode]}</p> : undefined}>
        <ToggleRow
          label="Multiple users"
          hint="Choose who is watching. Each person keeps their own history, favourites and preferences."
          checked={data.enabled}
          onChange={(v) => void configure(v, data.mode, data.askOnStartup)}
        />
        {data.enabled && (
          <>
            <SelectRow label="Profiles come from" value={data.mode} options={MODES} onChange={(m) => void configure(true, m, data.askOnStartup)} />
            <ToggleRow label="Ask at startup" hint="Show “Who’s watching?” each time Flick opens." checked={data.askOnStartup} onChange={(v) => void configure(true, data.mode, v)} />
          </>
        )}
      </SettingsGroup>

      {data.enabled && (
        <SettingsGroup title="Profiles">
          {listed.map((p) => (
            <LinkRow key={p.id} label={`${p.name}${p.hidden ? " (hidden)" : ""}${p.id === data.active ? " · you" : ""}`} value={<AccountPills accounts={p.accounts} className="justify-end" />} onClick={() => setEditing(p)} />
          ))}
          {data.mode !== "serverUsers" && <LinkRow label="Add Profile" onClick={() => setEditing("new")} />}
          {data.mode === "serverUsers" && hidden > 0 && (
            <LinkRow label={showHidden ? "Hide Hidden Profiles" : `Show Hidden Profiles (${hidden})`} onClick={() => setShowHidden(!showHidden)} />
          )}
          <LinkRow label="Choose Who’s Watching…" onClick={() => navigate("/profiles")} />
        </SettingsGroup>
      )}

      <ProfileEditor target={editing} onClose={() => setEditing(null)} />
      <TvDialog open={!!retry} onClose={() => setRetry(null)} title="PIN Required">
        <PinPad
          title="Enter a profile PIN"
          hint="This change needs the PIN of a protected profile."
          onSubmit={async (pin) => {
            const r = (await retry?.(pin)) ?? "ok";
            if (r === "ok") setRetry(null);
            return r;
          }}
          onCancel={() => setRetry(null)}
        />
      </TvDialog>
    </>
  );
}
