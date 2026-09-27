// Loading, notices and empty states. Neutral tone, always saying what to do.
import { CircleAlert, Info, TriangleAlert } from "lucide-react";
import { motion } from "motion/react";
import type { ReactNode } from "react";
import { enter } from "@/lib/motion";
import { cn } from "@/lib/utils";

export function Spinner({ label, className }: { label?: string; className?: string }) {
  return (
    <div role="status" className={cn("flex items-center gap-3 text-muted-foreground", className)}>
      <svg className="size-6 animate-spin" viewBox="0 0 24 24" aria-hidden>
        {Array.from({ length: 8 }, (_, i) => (
          <line key={i} x1="12" y1="3" x2="12" y2="7" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" opacity={0.25 + (i / 8) * 0.75} transform={`rotate(${i * 45} 12 12)`} />
        ))}
      </svg>
      {label && <span className="text-sm">{label}</span>}
    </div>
  );
}

export function CenteredSpinner({ label }: { label?: string }) {
  return (
    <div className="grid min-h-[50vh] place-items-center">
      <Spinner label={label} />
    </div>
  );
}

const tones = {
  info: { icon: Info, className: "text-white/85" },
  warn: { icon: TriangleAlert, className: "text-amber-200" },
  error: { icon: CircleAlert, className: "text-red-300" },
};

export function Notice({ tone = "info", children, className }: { tone?: keyof typeof tones; children: ReactNode; className?: string }) {
  const { icon: Icon, className: toneClass } = tones[tone];
  return (
    <div role={tone === "error" ? "alert" : "status"} className={cn("glass flex items-start gap-3 rounded-2xl px-4 py-3 text-sm leading-relaxed", className)}>
      <Icon className={cn("mt-0.5 size-4 shrink-0", toneClass)} />
      <div className="min-w-0 text-white/85">{children}</div>
    </div>
  );
}

export function EmptyState({ title, children, actions }: { title: string; children?: ReactNode; actions?: ReactNode }) {
  return (
    <motion.div initial={{ opacity: 0, y: 16 }} animate={{ opacity: 1, y: 0 }} transition={enter} className="flex max-w-xl flex-col items-start gap-4 px-[var(--gutter)] py-24">
      <h1 className="text-4xl font-bold tracking-tight">{title}</h1>
      {children && <div className="text-lg leading-relaxed text-muted-foreground">{children}</div>}
      {actions && <div className="mt-2 flex flex-wrap gap-3">{actions}</div>}
    </motion.div>
  );
}
