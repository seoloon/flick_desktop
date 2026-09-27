// Back inside the app: react-router numbers its history entries (`idx`); the
// first one has nothing of ours behind it, so Back goes Home instead of
// leaving the app (a detail page opened directly, a reload).
import type { NavigateFunction } from "react-router";

export function hasAppHistory(state: unknown): boolean {
  if (typeof state !== "object" || state === null || !("idx" in state)) return false;
  const idx = (state as { idx: unknown }).idx;
  return typeof idx === "number" && idx > 0;
}

export function goBack(navigate: NavigateFunction) {
  if (hasAppHistory(window.history.state)) void navigate(-1);
  else void navigate("/");
}
