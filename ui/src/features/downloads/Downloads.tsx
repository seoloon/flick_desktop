// Settings › Downloads: what is downloading, what is on disk. Downloads go
// through Flick Server (FlickDD), the same server as Watch Together.
import { FolderOpen, Pause, Play, Trash2, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { Notice } from "@/components/tv/Feedback";
import { InfoRow, SettingsGroup } from "@/components/tv/SettingsList";
import { api, asError } from "@/ipc/api";
import type { DownloadItem } from "@/ipc/app-types";
import { FocusGroup } from "@/nav/Focusable";
import { etaText, percent, sortDownloads, stateText, totalRemainingSecs } from "./format";
import { useDownloads } from "./store";

const run = (p: Promise<unknown>) => void p.catch((e) => toast.error(asError(e).message));

function Row({ d }: { d: DownloadItem }) {
  const finished = d.state === "done";
  const pct = percent(d);
  return (
    <div className="flex flex-col gap-2.5 px-4 py-3.5">
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <div className="truncate text-[0.9375rem] text-white/90">{d.title}</div>
          {d.subtitle && <div className="truncate text-sm text-muted-foreground">{d.subtitle}</div>}
        </div>
        <FocusGroup className="flex shrink-0 items-center gap-2">
          {finished && !d.missing && (
            <>
              <Button size="sm" icon={Play} onClick={() => run(api.downloadsOpen(d.id))}>
                Play
              </Button>
              <Button size="icon-sm" icon={FolderOpen} label="Show in folder" onClick={() => run(api.downloadsReveal(d.id))} />
            </>
          )}
          {(d.state === "active" || d.state === "queued") && <Button size="icon-sm" icon={Pause} label="Pause" onClick={() => run(api.downloadsPause(d.id))} />}
          {(d.state === "paused" || d.state === "failed") && (
            <Button size="icon-sm" icon={Play} label={d.state === "failed" ? "Try again" : "Resume"} onClick={() => run(api.downloadsResume(d.id))} />
          )}
          {finished ? (
            <Button size="icon-sm" variant="danger" icon={Trash2} label="Delete the file" onClick={() => run(api.downloadsRemove(d.id, true))} />
          ) : (
            <Button size="icon-sm" variant="danger" icon={X} label="Cancel and delete" onClick={() => run(api.downloadsRemove(d.id, false))} />
          )}
        </FocusGroup>
      </div>
      {!finished && (
        <div className="h-1.5 overflow-hidden rounded-full bg-white/12" role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
          <div className={`h-full rounded-full ${d.state === "failed" ? "bg-red-400" : "bg-white"} transition-[width] duration-300`} style={{ width: `${pct}%` }} />
        </div>
      )}
      <div className={`text-sm ${d.state === "failed" || d.missing ? "text-red-400" : "text-muted-foreground"}`}>{stateText(d)}</div>
    </div>
  );
}

export function DownloadsSettings() {
  const items = useDownloads((s) => s.items);
  const status = useDownloads((s) => s.status);
  const list = sortDownloads(Object.values(items));
  const left = totalRemainingSecs(list);
  return (
    <>
      {status && !status.configured && (
        <Notice tone="warn">Downloads go through your Flick Server. Add its invitation link in Settings › Watch Together first.</Notice>
      )}
      <SettingsGroup
        title="Downloads"
        note={
          <p>
            Files are fetched through your Flick Server, a piece at a time, and resume by themselves after a network drop or a restart. Open a title and use the download button to add it; for a series,
            every episode is added.
          </p>
        }
      >
        {left != null && <InfoRow label="Time left for everything">{etaText(left)}</InfoRow>}
        {list.length === 0 ? <div className="px-4 py-4 text-[0.9375rem] text-muted-foreground">Nothing downloaded yet.</div> : list.map((d) => <Row key={d.id} d={d} />)}
      </SettingsGroup>
      {status && (
        <SettingsGroup title="Storage">
          <InfoRow label="Folder">{status.directory}</InfoRow>
        </SettingsGroup>
      )}
    </>
  );
}
