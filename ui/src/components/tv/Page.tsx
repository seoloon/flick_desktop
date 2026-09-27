// Page scaffolding shared by the non-media screens.
import { motion } from "motion/react";
import type { ReactNode } from "react";
import { enter } from "@/lib/motion";
import { cn } from "@/lib/utils";

export function Page({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("flex flex-col gap-8 px-[var(--gutter)] pt-[var(--page-top)] pb-24", className)}>{children}</div>;
}

export function PageHeader({ title, lead, actions }: { title: ReactNode; lead?: ReactNode; actions?: ReactNode }) {
  return (
    <header className="flex flex-wrap items-end justify-between gap-x-8 gap-y-4">
      <motion.div initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={enter} className="flex max-w-3xl flex-col gap-2">
        <h1 className="text-[2.75rem] leading-none font-bold tracking-tight">{title}</h1>
        {lead && <p className="text-[1.0625rem] leading-relaxed text-muted-foreground">{lead}</p>}
      </motion.div>
      {/* Glass controls must not sit under a fading ancestor: slide only. */}
      {actions && (
        <motion.div initial={{ y: 12 }} animate={{ y: 0 }} transition={enter} className="flex flex-wrap items-center gap-3">
          {actions}
        </motion.div>
      )}
    </header>
  );
}

/** A frosted card grouping related content. */
export function Panel({ title, children, className, actions }: { title?: ReactNode; children: ReactNode; className?: string; actions?: ReactNode }) {
  return (
    <section className={cn("glass flex flex-col gap-4 rounded-3xl p-6", className)}>
      {(title || actions) && (
        <div className="flex items-center justify-between gap-4">
          {title && <h2 className="text-lg font-semibold">{title}</h2>}
          {actions}
        </div>
      )}
      {children}
    </section>
  );
}

/** Label/value facts, e.g. technical details. */
export function Facts({ rows, mono }: { rows: [ReactNode, ReactNode][]; mono?: boolean }) {
  return (
    <dl className={cn("grid grid-cols-[minmax(8rem,max-content)_1fr] gap-x-6 gap-y-2 text-sm", mono && "font-mono text-xs")}>
      {rows.map(([k, v], i) => (
        <div key={i} className="contents">
          <dt className="text-muted-foreground">{k}</dt>
          <dd className="min-w-0 break-words text-white/90 select-text">{v}</dd>
        </div>
      ))}
    </dl>
  );
}

export function Pill({ children, tone = "plain" }: { children: ReactNode; tone?: "plain" | "strong" | "warn" }) {
  return (
    <span
      className={cn(
        "inline-flex h-6 w-fit items-center rounded-md px-2 text-xs font-semibold tracking-wide whitespace-nowrap",
        tone === "plain" && "bg-white/12 text-white/85 ring-1 ring-white/10 ring-inset",
        tone === "strong" && "bg-white text-black",
        tone === "warn" && "bg-amber-300/15 text-amber-200 ring-1 ring-amber-200/20 ring-inset",
      )}
    >
      {children}
    </span>
  );
}
