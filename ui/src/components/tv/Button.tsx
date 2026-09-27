// tvOS-style buttons: pill shapes on glass; the focused one turns white on
// black and lifts. The only strong visual in the chrome is focus.
import type { LucideIcon } from "lucide-react";
import { type HTMLMotionProps, motion } from "motion/react";
import type { ReactNode } from "react";
import { focusSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { useTv } from "@/nav/Focusable";

export type ButtonVariant = "primary" | "glass" | "ghost" | "danger";
export type ButtonSize = "sm" | "md" | "lg" | "icon-sm" | "icon" | "icon-lg";

type Props = Omit<HTMLMotionProps<"button">, "ref" | "children"> & {
  variant?: ButtonVariant;
  size?: ButtonSize;
  icon?: LucideIcon;
  /** Fill the icon (favourite on, play). */
  iconFilled?: boolean;
  autoFocus?: boolean;
  focusKey?: string;
  label?: string;
  children?: ReactNode;
  onFocused?: () => void;
};

const sizes: Record<ButtonSize, string> = {
  sm: "h-9 gap-1.5 px-4 text-[0.8125rem] [&_svg]:size-4",
  md: "h-11 gap-2 px-5 text-[0.9375rem] [&_svg]:size-[1.15rem]",
  lg: "h-13 gap-2.5 px-7 text-base [&_svg]:size-5",
  "icon-sm": "size-9 [&_svg]:size-[1.125rem]",
  icon: "size-11 [&_svg]:size-5",
  "icon-lg": "size-16 [&_svg]:size-7",
};

const variants: Record<ButtonVariant, string> = {
  primary: "bg-white text-black hover:bg-white/90",
  glass: "glass text-white hover:bg-white/16",
  ghost: "text-white/80 hover:bg-white/10 hover:text-white",
  danger: "glass text-destructive hover:bg-destructive/15",
};

export function Button({
  variant = "glass",
  size = "md",
  icon: Icon,
  iconFilled,
  autoFocus,
  focusKey,
  label,
  children,
  className,
  disabled,
  onFocused,
  ...rest
}: Props) {
  const tv = useTv<HTMLButtonElement>({ autoFocus, focusKey, focusable: !disabled, onFocused });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      aria-label={label}
      title={size.startsWith("icon") ? label : undefined}
      disabled={disabled}
      data-tv-focus={tv.showFocus || undefined}
      animate={{ scale: tv.showFocus ? 1.08 : 1 }}
      whileTap={{ scale: 0.95 }}
      transition={focusSpring}
      className={cn(
        "relative inline-flex shrink-0 cursor-pointer items-center justify-center rounded-full font-semibold whitespace-nowrap select-none",
        "transition-[background-color,color,box-shadow] duration-200 ease-apple disabled:pointer-events-none disabled:opacity-40",
        "[&_svg]:shrink-0",
        sizes[size],
        variants[variant],
        tv.showFocus && "bg-white text-black shadow-[0_18px_40px_-12px_rgb(0_0_0/0.75)] hover:bg-white",
        className,
      )}
      {...rest}
    >
      {Icon && <Icon strokeWidth={2.2} fill={iconFilled ? "currentColor" : "none"} />}
      {children}
    </motion.button>
  );
}
