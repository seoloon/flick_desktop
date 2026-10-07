// Settings › Flick Server. One server, one invitation link, three things:
// the connection to it, watch together (FlickSync) and downloads (FlickDD).
// Each has its own tab, so nothing is scattered over one long page.
import { useQuery } from "@tanstack/react-query";
import { useSearchParams } from "react-router";
import { Notice } from "@/components/tv/Feedback";
import { Segmented } from "@/components/tv/Segmented";
import { DownloadsSettings } from "@/features/downloads/Downloads";
import { refreshStatus } from "@/features/watch/store";
import { WatchSettings } from "@/features/watch/WatchSettings";
import { api } from "@/ipc/api";
import type { Settings } from "@/ipc/bindings/Settings";
import { flushSettings, updateSettings } from "@/lib/settings";
import { ConnectionTest, Invitation } from "./Connection";

const TABS = [
  { value: "connection", label: "Connection" },
  { value: "watch", label: "Watch Together" },
  { value: "downloads", label: "Downloads" },
] as const;
type Tab = (typeof TABS)[number]["value"];

const tabOf = (raw: string | null): Tab => TABS.find((t) => t.value === raw)?.value ?? "connection";

export function FlickServerSettings({ s }: { s: Settings }) {
  const [params, setParams] = useSearchParams();
  const tab = tabOf(params.get("tab"));
  const invitation = useQuery({ queryKey: ["flickserver-invitation"], queryFn: () => api.flickserverInvitation() });

  const pick = (t: Tab) =>
    setParams(
      (p) => {
        const next = new URLSearchParams(p);
        next.set("tab", t);
        return next;
      },
      { replace: true },
    );

  // A first link turns watch together on: that is what people add a server for.
  const saved = () => {
    if (s.flicksync.enabled) return;
    updateSettings((x) => (x.flicksync.enabled = true));
    void flushSettings().then(refreshStatus, refreshStatus);
  };

  return (
    <>
      <Segmented options={[...TABS]} value={tab} onChange={pick} label="Flick Server" />
      {!invitation.data && invitation.isSuccess && tab !== "connection" && (
        <Notice tone="warn">No Flick Server is set up yet: add its invitation link in the Connection tab.</Notice>
      )}
      {tab === "connection" && (
        <>
          <Invitation onSaved={saved} />
          <ConnectionTest />
        </>
      )}
      {tab === "watch" && <WatchSettings s={s} />}
      {tab === "downloads" && <DownloadsSettings />}
    </>
  );
}
