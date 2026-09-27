// Segmented control / tab strip on glass. The selection is a soft pill that
// slides between segments; the focused segment turns white on black.
import { motion } from "motion/react";
import { useId } from "react";
import { focusSpring, pillSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";

export type Choice<T extends string> = { value: T; label: string };

type Props<T extends string> = {
  options: Choice<T>[];
  value: T;
  onChange: (v: T) => void;
  className?: string;
  size?: "sm" | "md";
  label?: string;
};

export function Segmented<T extends string>({ options, value, onChange, className, size = "md", label }: Props<T>) {
  const id = useId();
  return (
    <FocusGroup
      fade="x"
      role="tablist"
      aria-label={label}
      className={cn("[--fade-size:1.5rem] glass inline-flex w-fit max-w-full shrink-0 items-center gap-0.5 overflow-x-auto no-scrollbar rounded-full p-1", className)}
    >
      {options.map((o) => (
        <Segment key={o.value} layoutId={`seg-${id}`} selected={o.value === value} size={size} onSelect={() => onChange(o.value)}>
          {o.label}
        </Segment>
      ))}
    </FocusGroup>
  );
}

function Segment({ selected, onSelect, children, layoutId, size }: { selected: boolean; onSelect: () => void; children: string; layoutId: string; size: "sm" | "md" }) {
  const tv = useTv<HTMLButtonElement>({ scroll: false });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      role="tab"
      aria-selected={selected}
      {...tv.props}
      onClick={onSelect}
      animate={{ scale: tv.showFocus ? 1.06 : 1 }}
      transition={focusSpring}
      className={cn(
        "relative shrink-0 cursor-pointer rounded-full font-semibold whitespace-nowrap transition-colors duration-200",
        size === "sm" ? "h-8 px-3.5 text-[0.8125rem]" : "h-9 px-4.5 text-sm",
        tv.showFocus ? "text-black" : selected ? "text-white" : "text-white/60 hover:text-white",
      )}
    >
      {selected && <motion.span layoutId={layoutId} className="absolute inset-0 rounded-full bg-white/18" transition={pillSpring} />}
      {tv.showFocus && <motion.span layoutId={`${layoutId}-focus`} className="absolute inset-0 rounded-full bg-white shadow-lg" transition={pillSpring} />}
      <span className="relative">{children}</span>
    </motion.button>
  );
}
