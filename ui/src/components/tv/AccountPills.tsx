// Where a profile's accounts come from, said quietly: one small pill per
// server. Solid = signed in, dashed = to sign in, faded = offline.
import type { ProfileAccount } from "@/ipc/bindings/ProfileAccount";
import { accountTitle } from "@/lib/profiles";
import { cn } from "@/lib/utils";
import { ProviderLogo } from "./ServerBadge";

export function AccountPills({ accounts, className }: { accounts: ProfileAccount[]; className?: string }) {
  return (
    <div className={cn("flex flex-wrap justify-center gap-1.5", className)}>
      {accounts.map((a) => (
        <span
          key={`${a.kind}:${a.serverName}:${a.userName}`}
          title={accountTitle(a)}
          className={cn(
            "inline-flex max-w-[10rem] items-center gap-1 rounded-full px-2 py-0.5 text-[0.75rem] font-medium text-white/50",
            a.state === "connected" && "bg-white/[0.07]",
            a.state === "pending" && "border border-dashed border-white/25",
            a.state === "offline" && "bg-white/[0.04] opacity-50",
            a.state === "disabled" && "bg-white/[0.04] opacity-50",
          )}
        >
          <ProviderLogo kind={a.kind} className="size-3" />
          <span className="truncate">{a.serverName}</span>
          {a.state === "offline" && <span className="shrink-0">· offline</span>}
          {a.state === "disabled" && <span className="shrink-0">· off</span>}
        </span>
      ))}
    </div>
  );
}
