// The update prompt: what's new in the release, then "Update Now" or
// "Later". It opens after the launch check, or from Settings › General, and
// waits while a video plays or the profile picker is up.
import { Download, RotateCcw } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo } from "react";
import { useLocation } from "react-router";
import { Notice, Spinner } from "@/components/tv/Feedback";
import { Button } from "@/components/tv/Button";
import { FlickMark } from "@/components/tv/FlickMark";
import { TvDialog } from "@/components/tv/TvDialog";
import { enter } from "@/lib/motion";
import { useSettings } from "@/lib/settings";
import { checkForUpdates, closeUpdatePrompt, formatBytes, installUpdate, parseNotes, type UpdatePhase, useUpdates } from "@/lib/updates";
import { cn } from "@/lib/utils";

/** Screens the prompt never covers: it waits until they are left. */
const QUIET_ROUTES = ["/play", "/profiles"];
/** After the launch intro, let the home screen settle first. */
const LAUNCH_DELAY_MS = 2500;

let launchChecked = false;

/** `launchReady`: the launch intro is over. */
export function UpdatePrompt({ launchReady }: { launchReady: boolean }) {
  const enabled = useSettings()?.general.checkForUpdates;
  const { pathname } = useLocation();
  const { promptOpen, update, status } = useUpdates();

  useEffect(() => {
    // A development build is always behind the latest release.
    if (!launchReady || enabled === undefined || launchChecked || import.meta.env.DEV) return;
    launchChecked = true;
    if (!enabled) return;
    const t = window.setTimeout(() => void checkForUpdates({ prompt: true }), LAUNCH_DELAY_MS);
    return () => window.clearTimeout(t);
  }, [launchReady, enabled]);

  const working = status.phase === "downloading" || status.phase === "installing";
  const open = promptOpen && !!update && (working || !QUIET_ROUTES.includes(pathname));
  const released = update?.date ? new Date(update.date).toLocaleDateString("en", { day: "numeric", month: "long", year: "numeric" }) : null;

  return (
    <TvDialog
      open={open}
      onClose={closeUpdatePrompt}
      dismissible={!working}
      icon={
        <div className="mb-3 flex items-center gap-3">
          <span className="grid size-12 place-items-center rounded-[0.9rem] bg-white text-black shadow-[0_14px_36px_-10px_rgb(255_255_255/0.45)]">
            <FlickMark className="size-6" />
          </span>
          <span className="text-[0.8125rem] font-semibold tracking-wide text-muted-foreground uppercase">Update available</span>
        </div>
      }
      title={update ? `Flick ${update.version}` : "Flick"}
      description={update && <>You have Flick {update.currentVersion}.{released && ` This version was released on ${released}.`}</>}
    >
      {update && <Notes markdown={update.notes} />}
      <AnimatePresence mode="wait" initial={false}>
        <motion.div key={working ? "work" : "choose"} initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -6 }} transition={enter}>
          {working ? <Progress status={status} /> : <Choice status={status} />}
        </motion.div>
      </AnimatePresence>
    </TvDialog>
  );
}

function Choice({ status }: { status: UpdatePhase }) {
  const failed = status.phase === "error" && status.during === "install";
  return (
    <div className="flex flex-col gap-4">
      {failed && <Notice tone="error">The update could not be installed: {status.message}</Notice>}
      <div className="flex items-center justify-end gap-3">
        <Button variant="glass" onClick={closeUpdatePrompt}>
          Later
        </Button>
        <Button variant="primary" icon={failed ? RotateCcw : Download} autoFocus onClick={() => void installUpdate()}>
          {failed ? "Try Again" : "Update Now"}
        </Button>
      </div>
      <p className="text-[0.8125rem] leading-relaxed text-muted-foreground">Flick restarts once the update is installed. Later keeps it ready in Settings › General.</p>
    </div>
  );
}

function Progress({ status }: { status: UpdatePhase }) {
  const installing = status.phase === "installing";
  const total = status.phase === "downloading" ? status.total : null;
  const done = status.phase === "downloading" ? status.downloaded : 0;
  const ratio = installing ? 1 : total ? Math.min(1, done / total) : null;
  return (
    <div className="flex flex-col gap-3" role="status" aria-live="polite">
      <div className="relative h-1.5 overflow-hidden rounded-full bg-white/12">
        {ratio === null ? (
          <motion.span
            className="absolute inset-y-0 w-1/3 rounded-full bg-white/80"
            initial={{ left: "-33%" }}
            animate={{ left: "100%" }}
            transition={{ duration: 1.2, ease: "easeInOut", repeat: Infinity }}
          />
        ) : (
          <motion.span className="absolute inset-y-0 left-0 rounded-full bg-white" animate={{ width: `${ratio * 100}%` }} transition={{ type: "spring", stiffness: 120, damping: 24 }} />
        )}
      </div>
      <div className="flex items-center justify-between gap-4 text-[0.875rem]">
        {installing ? (
          <Spinner label="Installing. Flick will restart in a moment." className="text-white/85 [&_svg]:size-4" />
        ) : (
          <span className="text-white/85">Downloading…</span>
        )}
        {!installing && (
          <span className="text-muted-foreground tabular-nums">
            {ratio !== null && total ? `${formatBytes(done)} of ${formatBytes(total)} · ${Math.round(ratio * 100)} %` : done > 0 ? formatBytes(done) : ""}
          </span>
        )}
      </div>
    </div>
  );
}

function Notes({ markdown }: { markdown: string | null }) {
  const blocks = useMemo(() => (markdown ? parseNotes(markdown) : []), [markdown]);
  return (
    <div className="glass relative rounded-2xl">
      <div className="max-h-[min(16rem,40vh)] overflow-y-auto px-5 py-4 [mask-image:linear-gradient(to_bottom,black_calc(100%-1.5rem),transparent)] pb-6">
        {blocks.length === 0 ? (
          <p className="text-[0.9375rem] text-white/80">Improvements and fixes.</p>
        ) : (
          <div className="flex flex-col gap-1.5 text-[0.9375rem] leading-relaxed [overflow-wrap:anywhere] text-white/85">
            {blocks.map((b, i) =>
              b.kind === "heading" ? (
                <h3 key={i} className={cn("text-[0.8125rem] font-semibold tracking-wide text-white uppercase", i > 0 && "mt-3")}>
                  {b.text}
                </h3>
              ) : b.kind === "item" ? (
                <p key={i} className="relative pl-4 before:absolute before:top-[0.7em] before:left-0 before:size-1.5 before:rounded-full before:bg-white/50">
                  {b.text}
                </p>
              ) : (
                <p key={i} className="text-white/70">
                  {b.text}
                </p>
              ),
            )}
          </div>
        )}
      </div>
    </div>
  );
}
