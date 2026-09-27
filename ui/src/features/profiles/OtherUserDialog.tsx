// "Other User": sign in to a Jellyfin server as someone its sign-in screen
// does not list. The new connection forms (or joins) a profile by name.
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "@/components/tv/Button";
import { Notice } from "@/components/tv/Feedback";
import { Segmented } from "@/components/tv/Segmented";
import { TextField } from "@/components/tv/TextField";
import { TvDialog } from "@/components/tv/TvDialog";
import { api, asError } from "@/ipc/api";
import { allServersQuery } from "@/lib/profiles";

export function OtherUserDialog({ open, onClose, onAdded }: { open: boolean; onClose: () => void; onAdded: () => void }) {
  const servers = useQuery(allServersQuery).data ?? [];
  const jellyfins = servers.map((e) => e.server).filter((s, i, all) => s.kind === "jellyfin" && all.findIndex((o) => o.remoteId === s.remoteId) === i);
  const [server, setServer] = useState<string | null>(null);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const chosen = jellyfins.find((s) => s.id === server) ?? jellyfins[0];

  const submit = async () => {
    if (!chosen) return;
    setError(null);
    try {
      await api.jellyfinLogin(chosen.baseUrl, username, password);
      onAdded();
      onClose();
    } catch (e) {
      setError(asError(e).message);
    }
  };

  return (
    <TvDialog open={open} onClose={onClose} title="Other User" description="Sign in as someone the server does not list. Plex Home members are listed already.">
      {jellyfins.length === 0 ? (
        <Notice>Add a Jellyfin server first.</Notice>
      ) : (
        <>
          {jellyfins.length > 1 && (
            <Segmented label="Server" value={chosen?.id ?? ""} options={jellyfins.map((s) => ({ value: s.id, label: s.name }))} onChange={setServer} />
          )}
          <TextField label="Username" value={username} onChange={setUsername} autoFocus />
          <TextField label="Password" type="password" value={password} onChange={setPassword} onEnter={() => void submit()} />
          {error && <Notice tone="error">{error}</Notice>}
          <Button variant="primary" disabled={!username} onClick={() => void submit()}>
            Sign In
          </Button>
        </>
      )}
    </TvDialog>
  );
}
