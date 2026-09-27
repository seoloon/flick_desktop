// The Flick mark (app/icons/basic/flick-mark.svg) as a component: it takes
// the surrounding text colour, so it stays monochrome with the chrome.
import { cn } from "@/lib/utils";

export function FlickMark({ className, title }: { className?: string; title?: string }) {
  return (
    <svg
      viewBox="0 0 488.64 480"
      fill="currentColor"
      className={cn("shrink-0", className)}
      role={title ? "img" : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
    >
      <polygon points="84.64,0 208.64,0 124,480 0,480" />
      <polygon points="224.64,0 488.64,0 468.89,112 204.89,112" />
      <polygon points="192.19,184 408.19,184 377.86,356 161.86,356 323.23,270" />
    </svg>
  );
}
