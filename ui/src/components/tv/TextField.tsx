import { cn } from "@/lib/utils";
import { useTv } from "@/nav/Focusable";

type Props = {
  label: string;
  value: string;
  onChange: (v: string) => void;
  type?: "text" | "password" | "url" | "search";
  placeholder?: string;
  autoFocus?: boolean;
  onEnter?: () => void;
  large?: boolean;
};

export function TextField({ label, value, onChange, type = "text", placeholder, autoFocus, onEnter, large }: Props) {
  const tv = useTv<HTMLInputElement>({ autoFocus });
  return (
    <label className="flex flex-col gap-2">
      <span className={cn("px-1 text-[0.8125rem] font-medium text-muted-foreground", large && "sr-only")}>{label}</span>
      <input
        ref={tv.ref}
        {...tv.props}
        type={type}
        value={value}
        placeholder={placeholder}
        spellCheck={false}
        autoComplete="off"
        onChange={(e) => onChange(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") onEnter?.();
        }}
        className={cn(
          "glass w-full rounded-2xl px-4 text-white outline-none placeholder:text-white/35 select-text",
          "transition-[box-shadow,background-color] duration-200 ease-apple focus:bg-white/14 focus:shadow-[0_0_0_2px_rgb(255_255_255/0.85)]",
          large ? "h-16 px-6 text-2xl font-medium" : "h-12 text-[0.9375rem]",
        )}
      />
    </label>
  );
}
