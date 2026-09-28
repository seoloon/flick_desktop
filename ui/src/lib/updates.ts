// App updates: checked at launch (when enabled) and from Settings › General.
// "Later" only closes the prompt; the update stays ready to install from
// Settings until the next launch offers it again.
import { create } from "zustand";
import { api, asError } from "@/ipc/api";
import type { UpdateInfo } from "@/ipc/app-types";

export type UpdatePhase =
  | { phase: "idle" }
  | { phase: "checking" }
  | { phase: "upToDate" }
  | { phase: "available" }
  | { phase: "downloading"; downloaded: number; total: number | null }
  | { phase: "installing" }
  | { phase: "error"; during: "check" | "install"; message: string };

type UpdateState = {
  status: UpdatePhase;
  /** The newer release found by the last check. */
  update: UpdateInfo | null;
  checkedAt: number | null;
  promptOpen: boolean;
};

export const useUpdates = create<UpdateState>(() => ({ status: { phase: "idle" }, update: null, checkedAt: null, promptOpen: false }));

const busy = () => {
  const p = useUpdates.getState().status.phase;
  return p === "checking" || p === "downloading" || p === "installing";
};

/** `prompt`: open the update prompt when a newer version is found. */
export async function checkForUpdates({ prompt }: { prompt: boolean }): Promise<UpdateInfo | null> {
  if (busy()) return useUpdates.getState().update;
  useUpdates.setState({ status: { phase: "checking" } });
  try {
    const update = await api.updateCheck();
    useUpdates.setState({ update, checkedAt: Date.now(), status: { phase: update ? "available" : "upToDate" }, promptOpen: !!update && prompt });
    return update;
  } catch (e) {
    useUpdates.setState({ checkedAt: Date.now(), status: { phase: "error", during: "check", message: asError(e).message } });
    return null;
  }
}

/** Downloads and installs the update found by the last check; Flick restarts. */
export async function installUpdate() {
  if (busy() || !useUpdates.getState().update) return;
  useUpdates.setState({ status: { phase: "downloading", downloaded: 0, total: null } });
  try {
    await api.updateInstall((p) => {
      if (p.event === "installing") useUpdates.setState({ status: { phase: "installing" } });
      else
        useUpdates.setState((s) => ({
          status: {
            phase: "downloading",
            total: p.event === "started" ? p.total : s.status.phase === "downloading" ? s.status.total : null,
            downloaded: p.event === "progress" ? p.downloaded : 0,
          },
        }));
    });
  } catch (e) {
    useUpdates.setState({ status: { phase: "error", during: "install", message: asError(e).message } });
  }
}

export function openUpdatePrompt() {
  if (useUpdates.getState().update) useUpdates.setState({ promptOpen: true });
}

/** "Later". Refused while the update is being installed. */
export function closeUpdatePrompt() {
  const p = useUpdates.getState().status.phase;
  if (p === "downloading" || p === "installing") return;
  useUpdates.setState((s) => ({ promptOpen: false, status: s.status.phase === "error" && s.update ? { phase: "available" } : s.status }));
}

// ── Presentation helpers ────────────────────────────────────────────────

export function formatBytes(n: number): string {
  if (n < 1024 * 1024) return `${Math.max(1, Math.round(n / 1024))} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

const relative = new Intl.RelativeTimeFormat("en", { numeric: "auto" });

/** "just now", "5 minutes ago", "yesterday". */
export function ago(then: number, now = Date.now()): string {
  const s = Math.round((then - now) / 1000);
  if (s > -45) return "just now";
  const m = Math.round(s / 60);
  if (m > -60) return relative.format(m, "minute");
  const h = Math.round(m / 60);
  if (h > -24) return relative.format(h, "hour");
  return relative.format(Math.round(h / 24), "day");
}

export type NoteBlock = { kind: "heading" | "item" | "text"; text: string };

/**
 * Release notes as a few blocks: headings, list items, paragraphs. Enough
 * for GitHub's Markdown (generated notes included); links keep their label,
 * emphasis and code marks are dropped.
 */
export function parseNotes(markdown: string): NoteBlock[] {
  const inline = (t: string) =>
    t
      .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
      .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
      .replace(/(\*\*|__|`)/g, "")
      .replace(/<[^>]+>/g, "")
      .trim();
  const blocks: NoteBlock[] = [];
  // A line right after a paragraph line continues it.
  let inParagraph = false;
  for (const raw of markdown.split(/\r?\n/)) {
    const line = raw.trim();
    const heading = /^#{1,6}\s+(.*)$/.exec(line);
    const item = /^(?:[-*+]|\d+\.)\s+(.*)$/.exec(line);
    const rule = /^(-{3,}|\*{3,}|_{3,})$/.test(line);
    const last = blocks.at(-1);
    if (!line || rule) {
      // A blank line or a rule ends the paragraph.
    } else if (heading) blocks.push({ kind: "heading", text: inline(heading[1]!) });
    else if (item) blocks.push({ kind: "item", text: inline(item[1]!) });
    else if (inParagraph && last) last.text = `${last.text} ${inline(line)}`;
    else blocks.push({ kind: "text", text: inline(line) });
    inParagraph = !!line && !rule && !heading && !item;
  }
  return blocks.filter((b) => b.text);
}
