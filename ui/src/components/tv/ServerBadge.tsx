// Where a title comes from: the provider's logo and the server's name. The
// small logos keep their brand colours, like channel badges on Apple TV:
// they are content, not chrome.
import { HardDrive } from "lucide-react";
import { useId } from "react";
import type { ProviderKind } from "@/ipc/bindings/ProviderKind";
import type { ServerDescriptor } from "@/ipc/bindings/ServerDescriptor";
import { cn } from "@/lib/utils";

export function ProviderLogo({ kind, className }: { kind: ProviderKind; className?: string }) {
  const id = useId();
  if (kind === "plex") {
    return (
      <svg viewBox="0 0 24 24" className={cn("size-4 shrink-0", className)} aria-label="Plex" role="img">
        <path d="M5.5 2h6.2l6.8 10-6.8 10H5.5l6.8-10z" fill="#EBAF00" />
      </svg>
    );
  }
  return (
    <svg viewBox="0 0 24 24" className={cn("size-4 shrink-0", className)} aria-label="Jellyfin" role="img">
      <defs>
        <linearGradient id={id} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#AA5CC3" />
          <stop offset="1" stopColor="#00A4DC" />
        </linearGradient>
      </defs>
      <path d="M12 2.8 22 20.2H2z" fill="none" stroke={`url(#${id})`} strokeWidth="2.6" strokeLinejoin="round" />
      <path d="M12 10.2 16 17.2H8z" fill={`url(#${id})`} />
    </svg>
  );
}

/** Logo + server name; `quiet` for secondary mentions ("also on"). */
export function ServerBadge({ server, quiet, className }: { server: ServerDescriptor; quiet?: boolean; className?: string }) {
  return (
    <span className={cn("inline-flex min-w-0 items-center gap-1.5", quiet ? "text-white/60" : "text-white/90", className)} title={`${server.name} · ${server.kind === "plex" ? "Plex" : "Jellyfin"}`}>
      <ProviderLogo kind={server.kind} className="size-[1.05em]" />
      <span className="truncate font-medium">{server.name}</span>
    </span>
  );
}

/** A title played from the files downloaded on this computer. */
export function LocalBadge({ quiet, className }: { quiet?: boolean; className?: string }) {
  return (
    <span className={cn("inline-flex min-w-0 items-center gap-1.5", quiet ? "text-white/60" : "text-white/90", className)} title="Downloaded on this computer">
      <HardDrive className="size-[1.05em] shrink-0" />
      <span className="truncate font-medium">Local</span>
    </span>
  );
}
