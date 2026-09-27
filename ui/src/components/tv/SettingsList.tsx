// Settings rows in the tvOS style: an inset grouped list on glass; the
// focused row turns white. Every row is one focus target: pickers and sliders
// change with left/right while focused, so a remote never needs a popup.
//
// Shape: a group is a 2xl panel with 1.5 (6 px) padding, so its rows use xl —
// the panel radius minus the padding — and sit concentric with its corners.
// `CompactRows` renders the same rows smaller for the player menu, which is
// built on the same panel geometry.
import { ChevronLeft, ChevronRight } from "lucide-react";
import { motion } from "motion/react";
import { createContext, type PointerEvent, type ReactNode, useContext, useEffect, useRef } from "react";
import { focusSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { useTv } from "@/nav/Focusable";
import { onAction } from "@/nav/input";
import type { Choice } from "./Segmented";

const Compact = createContext(false);

export function CompactRows({ children }: { children: ReactNode }) {
  return <Compact.Provider value>{children}</Compact.Provider>;
}

export function SettingsGroup({ title, note, children }: { title?: string; note?: ReactNode; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2.5">
      {title && <h2 className="px-5 text-[0.8125rem] font-semibold tracking-wide text-muted-foreground uppercase">{title}</h2>}
      <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: 0.4 }} className="glass flex flex-col rounded-2xl p-1.5">
        {children}
      </motion.div>
      {note && <div className="flex flex-col gap-2 px-5 text-[0.8125rem] leading-relaxed text-muted-foreground">{note}</div>}
    </section>
  );
}

/** Left/right while this row has focus. */
function useArrows(focused: boolean, fn: (step: -1 | 1) => void) {
  const ref = useRef(fn);
  ref.current = fn;
  useEffect(() => {
    if (!focused) return;
    return onAction((a) => {
      if (a.type !== "move" || (a.dir !== "left" && a.dir !== "right")) return false;
      ref.current(a.dir === "left" ? -1 : 1);
      return true;
    });
  }, [focused]);
}

type ShellProps = {
  label: string;
  hint?: ReactNode;
  disabled?: boolean;
  onClick?: () => void;
  children?: ReactNode;
  role?: string;
  ariaChecked?: boolean;
  onArrow?: (step: -1 | 1) => void;
  autoFocus?: boolean;
};

function RowShell({ label, hint, disabled, onClick, children, role, ariaChecked, onArrow, autoFocus }: ShellProps) {
  const tv = useTv<HTMLButtonElement>({ focusable: !disabled, autoFocus, scroll: "nearest" });
  const compact = useContext(Compact);
  useArrows(tv.focused && !disabled && !!onArrow, (s) => onArrow?.(s));
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      role={role}
      aria-checked={ariaChecked}
      disabled={disabled}
      onClick={onClick}
      data-tv-focus={tv.showFocus || undefined}
      animate={{ scale: tv.showFocus ? (compact ? 1.02 : 1.025) : 1 }}
      transition={focusSpring}
      className={cn(
        "group/row relative flex w-full cursor-pointer items-center justify-between rounded-xl text-left",
        compact ? "min-h-10 gap-4 px-3 py-2 scroll-my-4" : "min-h-14 gap-6 px-4 py-3 scroll-my-24",
        "transition-colors duration-200 ease-apple disabled:cursor-default disabled:opacity-40",
        tv.showFocus ? "z-10 bg-white text-black shadow-[0_16px_36px_-14px_rgb(0_0_0/0.8)]" : "hover:bg-white/[0.07]",
      )}
    >
      <span className="flex min-w-0 flex-col gap-0.5">
        <span className={cn("font-medium", compact ? "text-[0.8125rem]" : "text-[0.9375rem]")}>{label}</span>
        {hint && <span className={cn("leading-snug", compact ? "text-[0.6875rem]" : "text-[0.8125rem]", tv.showFocus ? "text-black/60" : "text-muted-foreground")}>{hint}</span>}
      </span>
      <span className={cn("flex shrink-0 items-center gap-2", compact ? "text-[0.8125rem]" : "text-[0.9375rem]", tv.showFocus ? "text-black/70" : "text-white/60")}>{children}</span>
    </motion.button>
  );
}

export function ToggleRow({ label, hint, checked, onChange, disabled }: { label: string; hint?: ReactNode; checked: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  const compact = useContext(Compact);
  return (
    <RowShell label={label} hint={hint} disabled={disabled} role="switch" ariaChecked={checked} onClick={() => onChange(!checked)}>
      <span
        className={cn(
          "relative flex items-center rounded-full p-0.5 transition-colors duration-300 ease-apple",
          compact ? "h-6 w-10" : "h-7 w-12",
          checked ? "justify-end bg-white group-data-tv-focus/row:bg-black" : "justify-start bg-white/20 group-data-tv-focus/row:bg-black/15",
        )}
      >
        <motion.span
          layout
          transition={focusSpring}
          className={cn("rounded-full shadow-sm", compact ? "size-5" : "size-6", checked ? "bg-black group-data-tv-focus/row:bg-white" : "bg-white")}
        />
      </span>
    </RowShell>
  );
}

export function SelectRow<T extends string>({ label, hint, value, options, onChange, disabled }: { label: string; hint?: ReactNode; value: T; options: Choice<T>[]; onChange: (v: T) => void; disabled?: boolean }) {
  const current = options.find((o) => o.value === value)?.label ?? String(value);
  const cycle = (step: number) => {
    const i = options.findIndex((o) => o.value === value);
    const next = options[(i + step + options.length) % options.length];
    if (next) onChange(next.value);
  };
  return (
    <RowShell label={label} hint={hint} disabled={disabled} onClick={() => cycle(1)} onArrow={cycle}>
      <span
        role="presentation"
        className="grid size-6 place-items-center rounded-full opacity-0 transition-opacity group-hover/row:opacity-100 group-data-tv-focus/row:opacity-100 hover:bg-black/10"
        onClick={(e) => {
          e.stopPropagation();
          cycle(-1);
        }}
      >
        <ChevronLeft className="size-4" />
      </span>
      <motion.span key={current} initial={{ opacity: 0, y: 4 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.18 }} className="min-w-[4rem] text-right" aria-live="polite">
        {current}
      </motion.span>
      <ChevronRight className="size-4 opacity-60" />
    </RowShell>
  );
}

export function SliderRow({
  label,
  hint,
  value,
  min,
  max,
  step,
  format,
  onChange,
  disabled,
}: {
  label: string;
  hint?: ReactNode;
  value: number;
  min: number;
  max: number;
  step: number;
  format?: (v: number) => string;
  onChange: (v: number) => void;
  disabled?: boolean;
}) {
  const clamp = (v: number) => {
    const snapped = Math.round((v - min) / step) * step + min;
    return Number(Math.min(max, Math.max(min, snapped)).toFixed(4));
  };
  const pct = ((value - min) / (max - min)) * 100;
  const compact = useContext(Compact);
  const drag = (e: PointerEvent<HTMLSpanElement>) => {
    e.stopPropagation();
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    const at = (x: number) => {
      const r = el.getBoundingClientRect();
      return clamp(min + ((x - r.left) / r.width) * (max - min));
    };
    onChange(at(e.clientX));
    const move = (ev: globalThis.PointerEvent) => onChange(at(ev.clientX));
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  };
  return (
    <RowShell label={label} hint={hint} disabled={disabled} onArrow={(s) => onChange(clamp(value + s * step))}>
      <span className={cn("relative flex h-6 cursor-pointer items-center", compact ? "w-28" : "w-44")} onPointerDown={drag} onClick={(e) => e.stopPropagation()} role="presentation">
        <span className="h-1.5 w-full overflow-hidden rounded-full bg-white/20 group-data-tv-focus/row:bg-black/15">
          <span className="block h-full rounded-full bg-white group-data-tv-focus/row:bg-black" style={{ width: `${pct}%` }} />
        </span>
        <span className="absolute size-4 -translate-x-1/2 rounded-full bg-white shadow ring-1 ring-black/10" style={{ left: `${pct}%` }} />
      </span>
      <span className={cn("text-right tabular-nums", compact ? "w-12" : "w-20")}>{format ? format(value) : value}</span>
    </RowShell>
  );
}

export function LinkRow({ label, hint, value, onClick, autoFocus }: { label: string; hint?: ReactNode; value?: ReactNode; onClick: () => void; autoFocus?: boolean }) {
  return (
    <RowShell label={label} hint={hint} onClick={onClick} autoFocus={autoFocus}>
      {value}
      <ChevronRight className="size-4 opacity-60" />
    </RowShell>
  );
}

/** Read-only fact inside a group. */
export function InfoRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex min-h-12 items-center justify-between gap-6 px-4 py-2.5 text-[0.9375rem]">
      <span className="text-white/90">{label}</span>
      <span className="min-w-0 text-right break-words text-muted-foreground select-text">{children}</span>
    </div>
  );
}
