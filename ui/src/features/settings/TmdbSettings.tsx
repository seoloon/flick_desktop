// Settings › Metadata: the TMDB key behind person pages (biographies,
// photos, filmographies). Rust keeps it in the OS keychain and only tells
// the UI whether one is saved.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { Notice } from "@/components/tv/Feedback";
import { InfoRow, SettingsGroup } from "@/components/tv/SettingsList";
import { TextField } from "@/components/tv/TextField";
import { TmdbLogo } from "@/components/tv/TmdbLogo";
import { api, asError } from "@/ipc/api";
import { FocusGroup } from "@/nav/Focusable";

/** TMDB's attribution, required by its terms. */
function Attribution() {
  return (
    <div className="flex flex-col gap-2">
      <TmdbLogo />
      <p>This product uses the TMDB API but is not endorsed or certified by TMDB.</p>
    </div>
  );
}

export function TmdbSettings() {
  const queryClient = useQueryClient();
  const status = useQuery({ queryKey: ["tmdb-status"], queryFn: () => api.tmdbStatus() });
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ["tmdb-status"] });
    void queryClient.invalidateQueries({ queryKey: ["person"] });
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.tmdbSetKey(key);
      setKey("");
      toast.success("TMDB connected");
      refresh();
    } catch (e) {
      const err = asError(e);
      setError(err.kind === "unauthorized" ? "The key was refused by TMDB." : err.message);
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    try {
      await api.tmdbRemoveKey();
      refresh();
    } catch (e) {
      toast.error(asError(e).message);
    }
  };

  return (
    <SettingsGroup title="TMDB" note={<Attribution />}>
      {status.data ? (
        <>
          <InfoRow label="TMDB">Connected</InfoRow>
          <FocusGroup className="flex px-4 py-3">
            <Button variant="danger" size="sm" onClick={() => void remove()}>
              Remove Key
            </Button>
          </FocusGroup>
        </>
      ) : (
        <div className="flex flex-col gap-4 px-4 py-4">
          <p className="text-[0.9375rem] leading-relaxed text-muted-foreground">
            Free key at themoviedb.org › Settings › API. Used for biographies, photos and filmographies on person pages.
          </p>
          <TextField label="API key or read access token" type="password" value={key} onChange={setKey} onEnter={() => void save()} />
          {error && <Notice tone="error">{error}</Notice>}
          <FocusGroup className="flex">
            <Button variant="primary" disabled={busy || !key.trim()} onClick={() => void save()}>
              Test &amp; Save
            </Button>
          </FocusGroup>
        </div>
      )}
    </SettingsGroup>
  );
}
