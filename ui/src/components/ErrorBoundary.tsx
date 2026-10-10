// Without a boundary, one exception while rendering unmounts the whole app and
// leaves a blank window (only a reload brings it back). This keeps the damage
// local, says what happened, and lets the user carry on.
import { Component, type ReactNode } from "react";
import { UI_CODE } from "@/lib/errors";

type Props = {
  children: ReactNode;
  /** Where the boundary sits, for the message and the log. */
  area?: string;
  /** A change resets the boundary (e.g. the route), so navigating recovers. */
  resetKey?: unknown;
  /** Smaller fallback for a panel inside a screen. */
  inline?: boolean;
};
type State = { error: Error | null };

export class ErrorBoundary extends Component<Props, State> {
  override state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  override componentDidCatch(error: Error, info: { componentStack?: string | null }) {
    console.error(`UI error in ${this.props.area ?? "app"}:`, error, info.componentStack);
  }

  override componentDidUpdate(prev: Props) {
    if (this.state.error && prev.resetKey !== this.props.resetKey) this.setState({ error: null });
  }

  override render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    const retry = () => this.setState({ error: null });
    if (this.props.inline) {
      return (
        <div role="alert" className="flex flex-col gap-2 rounded-xl bg-black/60 p-3 text-sm text-white/85">
          <span>Something went wrong here ({UI_CODE.crash}).</span>
          <code className="text-xs break-words text-white/55 select-text">{error.message}</code>
          <button type="button" onClick={retry} className="w-fit cursor-pointer rounded-full bg-white/15 px-3 py-1 text-xs font-semibold">
            Try again
          </button>
        </div>
      );
    }
    return (
      <div role="alert" className="fixed inset-0 z-[100] grid place-items-center bg-black p-8 text-white">
        <div className="flex max-w-xl flex-col gap-4">
          <h1 className="text-2xl font-bold tracking-tight">Something went wrong</h1>
          <p className="text-white/70">The screen hit an unexpected error ({UI_CODE.crash}). Your servers, room and playback are untouched.</p>
          <code className="rounded-lg bg-white/10 p-3 text-xs break-words whitespace-pre-wrap text-white/70 select-text">{error.message}</code>
          <div className="flex gap-3">
            <button type="button" onClick={retry} className="cursor-pointer rounded-full bg-white px-5 py-2 text-sm font-semibold text-black">
              Continue
            </button>
            <button type="button" onClick={() => window.location.reload()} className="cursor-pointer rounded-full bg-white/15 px-5 py-2 text-sm font-semibold">
              Reload
            </button>
          </div>
        </div>
      </div>
    );
  }
}
