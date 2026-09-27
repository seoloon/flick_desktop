// Four-digit PIN on glass. Digits are focusable keys (remote, controller);
// the keyboard's digits and Backspace work too. A wrong PIN shakes the dots
// and clears them; a lockout shows its countdown.
import { Delete, type LucideIcon } from "lucide-react";
import { motion, useAnimationControls } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { focusSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";
import { onAction, onKey } from "@/nav/input";

export type PinResult = "ok" | "wrong" | { locked: number };
type Props = { title: string; hint?: string; onSubmit: (pin: string) => Promise<PinResult>; onCancel: () => void };

const LENGTH = 4;

export function PinPad({ title, hint, onSubmit, onCancel }: Props) {
  const [pin, setPin] = useState("");
  const [busy, setBusy] = useState(false);
  const [locked, setLocked] = useState(0);
  const shake = useAnimationControls();
  const submit = useRef(onSubmit);
  submit.current = onSubmit;
  const cancel = useRef(onCancel);
  cancel.current = onCancel;
  const blocked = busy || locked > 0;

  useEffect(() => {
    if (locked <= 0) return;
    const t = setTimeout(() => setLocked((s) => s - 1), 1000);
    return () => clearTimeout(t);
  }, [locked]);

  const press = (d: string) => !blocked && setPin((p) => (p.length < LENGTH ? p + d : p));
  const erase = () => !blocked && setPin((p) => p.slice(0, -1));

  // Keyboard digits and Backspace are claimed before the app's key routing,
  // where Backspace means Back; Escape and B still cancel.
  const keys = useRef({ press, erase });
  keys.current = { press, erase };
  useEffect(
    () =>
      onKey((e) => {
        if (/^[0-9]$/.test(e.key)) keys.current.press(e.key);
        else if (e.key === "Backspace") keys.current.erase();
        else return false;
        return true;
      }),
    [],
  );

  useEffect(
    () =>
      onAction((a) => {
        if (a.type !== "back") return false;
        cancel.current();
        return true;
      }),
    [],
  );

  useEffect(() => {
    if (pin.length !== LENGTH) return;
    setBusy(true);
    void submit.current(pin).then(async (r) => {
      if (r === "ok") return; // the parent moves on
      if (r !== "wrong") setLocked(r.locked);
      await shake.start({ x: [0, -14, 12, -8, 6, 0], transition: { duration: 0.42 } });
      setPin("");
      setBusy(false);
    });
  }, [pin, shake]);

  return (
    <FocusGroup focusKey="pin-pad" boundary autoFocus className="flex flex-col items-center gap-7">
      <div className="flex flex-col items-center gap-1.5 text-center">
        <h2 className="text-2xl font-bold tracking-tight">{title}</h2>
        <p className="min-h-[1.4em] text-[0.9375rem] text-white/60">{locked > 0 ? `Too many attempts. Try again in ${locked} s.` : (hint ?? "")}</p>
      </div>
      <motion.div animate={shake} className="flex gap-4" role="status" aria-label={`${pin.length} of ${LENGTH} digits`}>
        {Array.from({ length: LENGTH }, (_, i) => (
          <motion.span
            key={i}
            animate={{ scale: i < pin.length ? 1 : 0.7, backgroundColor: i < pin.length ? "rgb(255 255 255)" : "rgb(255 255 255 / 0.2)" }}
            transition={focusSpring}
            className="size-3.5 rounded-full"
          />
        ))}
      </motion.div>
      <div className="grid grid-cols-3 gap-3">
        {["1", "2", "3", "4", "5", "6", "7", "8", "9"].map((d) => (
          <PinKey key={d} label={d} autoFocus={d === "5"} disabled={blocked} onPress={() => press(d)} />
        ))}
        <span />
        <PinKey label="0" disabled={blocked} onPress={() => press("0")} />
        <PinKey label="Delete" icon={Delete} disabled={blocked} onPress={erase} />
      </div>
    </FocusGroup>
  );
}

function PinKey({ label, icon: Icon, autoFocus, disabled, onPress }: { label: string; icon?: LucideIcon; autoFocus?: boolean; disabled?: boolean; onPress: () => void }) {
  const tv = useTv<HTMLButtonElement>({ autoFocus, scroll: false });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      aria-label={label}
      disabled={disabled}
      onClick={onPress}
      animate={{ scale: tv.showFocus ? 1.1 : 1 }}
      whileTap={{ scale: 0.92 }}
      transition={focusSpring}
      className={cn(
        "grid size-[4.5rem] cursor-pointer place-items-center rounded-full bg-white/[0.08] text-2xl font-semibold tabular-nums transition-colors hover:bg-white/15 disabled:opacity-40",
        tv.showFocus && "bg-white text-black hover:bg-white",
      )}
    >
      {Icon ? <Icon className="size-6" /> : label}
    </motion.button>
  );
}
