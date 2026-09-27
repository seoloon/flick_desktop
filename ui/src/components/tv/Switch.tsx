// A standalone on/off switch (settings rows draw their own inside the row).
// The thumb slides on a spring; focus lifts it and rings it in white.
import { motion } from "motion/react";
import { focusSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { useTv } from "@/nav/Focusable";

export function Switch({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  const tv = useTv<HTMLButtonElement>({ focusable: !disabled, scroll: false });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      role="switch"
      aria-checked={checked}
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      animate={{ scale: tv.showFocus ? 1.1 : 1 }}
      whileTap={{ scale: 0.94 }}
      transition={focusSpring}
      className={cn(
        "flex h-7 w-12 shrink-0 cursor-pointer items-center rounded-full p-0.5 transition-[background-color,box-shadow] duration-300 ease-apple disabled:cursor-default disabled:opacity-40",
        checked ? "justify-end bg-white" : "justify-start bg-white/20 hover:bg-white/28",
        tv.showFocus && "shadow-[0_0_0_3px_rgb(0_0_0/0.5),0_0_0_5px_rgb(255_255_255)]",
      )}
    >
      <motion.span layout transition={focusSpring} className={cn("size-6 rounded-full shadow-sm", checked ? "bg-black" : "bg-white")} />
    </motion.button>
  );
}
