// Casting from the player: the device menu, and the screen that replaces the
// video while a Chromecast or an AirPlay receiver plays it.
//
// The receiver pulls the stream itself (Rust relays it, see `oneshot-cast`),
// so the local player is stopped and this window only remote-controls it.
import { useQuery } from "@tanstack/react-query";
import { Airplay, Cast, Pause, Play, RotateCcw, RotateCw, Tv } from "lucide-react";
import { motion } from "motion/react";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { Spinner } from "@/components/tv/Feedback";
import { type PinResult, PinPad } from "@/components/tv/PinPad";
import { api, asError } from "@/ipc/api";
import type { CastDevice } from "@/ipc/bindings/CastDevice";
import type { CastStatus } from "@/ipc/bindings/CastStatus";
import type { ItemRef } from "@/ipc/bindings/ItemRef";
import { clock } from "@/lib/format";
import { focusSpring, panelSpring } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";
import { errorText } from "@/lib/errors";

/** The cast in progress, polled from Rust while there is one. */
export function useCastSession(resume: boolean) {
  const [status, setStatus] = useState<CastStatus | null>(null);
  const [starting, setStarting] = useState(false);
  /** An AirPlay receiver asked for a PIN: what to cast once it is paired. */
  const [pairing, setPairing] = useState<{ device: CastDevice; item: ItemRef; startMs: number } | null>(null);
  const active = status?.device != null;

  // Arriving mid-cast (the next episode of one): pick it up.
  useEffect(() => {
    if (resume) void api.castStatus().then((s) => setStatus(s.device ? s : null), () => undefined);
  }, [resume]);
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => void api.castStatus().then((s) => setStatus(s.device ? s : null), () => undefined), 1000);
    return () => window.clearInterval(timer);
  }, [active]);

  const start = useCallback(async (device: CastDevice, item: ItemRef, startMs: number) => {
    setStarting(true);
    try {
      await api.castStart(device.id, item, Math.round(startMs));
      const s = await api.castStatus();
      setStatus(s.device ? s : null);
      return true;
    } catch (e) {
      if (asError(e).code === PIN_NEEDED) {
        // The receiver shows a code on its screen: ask for it.
        try {
          await api.castPairBegin(device.id);
          setPairing({ device, item, startMs });
        } catch (begin) {
          toast(errorText(begin));
        }
        return false;
      }
      toast(errorText(e));
      return false;
    } finally {
      setStarting(false);
    }
  }, []);
  /** Ends the cast: where it had got to (ms), to carry on locally. */
  const stop = useCallback(async () => {
    const position = await api.castStop().catch(() => null);
    setStatus(null);
    return position;
  }, []);
  const command = useCallback((c: Parameters<typeof api.castCommand>[0]) => void api.castCommand(c).catch((e) => toast(errorText(e))), []);

  /** The PIN read on the receiver's screen; casting carries on once it is accepted. */
  const submitPin = useCallback(
    async (pin: string): Promise<PinResult> => {
      if (!pairing) return "wrong";
      try {
        await api.castPairFinish(pairing.device.id, pin);
      } catch (e) {
        const { code } = asError(e);
        if (code === "FLK-AUTH-008") return "wrong";
        toast(errorText(e));
        if (code === "FLK-CAST-010") return { locked: 30 };
        setPairing(null);
        return "wrong";
      }
      const { device, item, startMs } = pairing;
      setPairing(null);
      void start(device, item, startMs);
      return "ok";
    },
    [pairing, start],
  );
  const cancelPin = useCallback(() => setPairing(null), []);

  return { status, active, starting, start, stop, command, pairing, submitPin, cancelPin };
}

/** The code an AirPlay receiver wants (see `AUTH_AIRPLAY_PAIRING` in Rust). */
const PIN_NEEDED = "FLK-AUTH-007";

/** Asks for the PIN shown on the receiver's screen. */
export function CastPairing({ device, onSubmit, onCancel }: { device: CastDevice; onSubmit: (pin: string) => Promise<PinResult>; onCancel: () => void }) {
  return (
    <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="absolute inset-0 z-40 grid place-items-center bg-black/85 backdrop-blur-xl">
      <FocusGroup focusKey="cast-pairing" boundary autoFocus>
        <PinPad title={`PIN for ${device.name}`} hint="Type the 4-digit code shown on the TV. It is asked once; the device is remembered." onSubmit={onSubmit} onCancel={onCancel} />
      </FocusGroup>
    </motion.div>
  );
}

function DeviceRow({ device, onPick }: { device: CastDevice; onPick: () => void }) {
  const tv = useTv<HTMLButtonElement>({ scroll: "nearest" });
  const Icon = device.kind === "airPlay" ? Airplay : Cast;
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      onClick={onPick}
      animate={{ scale: tv.showFocus ? 1.02 : 1 }}
      transition={focusSpring}
      className={cn(
        "flex min-h-11 w-full cursor-pointer items-center gap-3 rounded-xl px-3 py-2 text-left transition-colors duration-200 ease-apple",
        tv.showFocus ? "z-10 bg-white text-black" : "hover:bg-white/[0.08]",
      )}
    >
      <Icon className="size-[1.125rem] shrink-0" />
      <span className="flex min-w-0 flex-col">
        <span className="truncate font-medium">{device.name}</span>
        <span className={cn("truncate text-[0.6875rem]", tv.showFocus ? "text-black/60" : "text-white/55")}>{device.kind === "airPlay" ? "AirPlay" : "Chromecast"}{device.model ? ` · ${device.model}` : ""}</span>
      </span>
    </motion.button>
  );
}

/** Receivers on the network (looked for while this is open). */
export function CastMenu({ busy, onPick }: { busy: boolean; onPick: (d: CastDevice) => void }) {
  const devices = useQuery({ queryKey: ["cast-devices"], queryFn: api.castDevices, refetchInterval: 2000, gcTime: 0 });
  const list = devices.data ?? [];
  return (
    <motion.aside
      aria-label="Cast to a device"
      initial={{ opacity: 0, y: 12, scale: 0.94 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 8, scale: 0.96 }}
      transition={panelSpring}
      style={{ transformOrigin: "bottom right" }}
      className="absolute right-8 bottom-[calc(100%+0.25rem)] w-[min(22rem,calc(100vw-4rem))] overflow-hidden rounded-2xl bg-black/75 p-1.5 text-[0.8125rem] text-white shadow-[inset_0_1px_0_rgb(255_255_255/0.12),inset_0_0_0_1px_rgb(255_255_255/0.08),0_30px_80px_-20px_rgb(0_0_0/0.8)]"
    >
      <FocusGroup focusKey="cast-menu" boundary autoFocus className="flex max-h-80 flex-col gap-0.5 overflow-y-auto">
        <h2 className="px-3 pt-2 pb-1 text-[0.6875rem] font-semibold tracking-wide text-white/55 uppercase">Cast to</h2>
        {list.map((d) => (
          <DeviceRow key={d.id} device={d} onPick={() => !busy && onPick(d)} />
        ))}
        {list.length === 0 && (
          <p className="flex items-center gap-2.5 px-3 py-3 text-white/60">
            <Spinner className="size-4" /> Looking for Chromecast and AirPlay devices…
          </p>
        )}
        {busy && <p className="px-3 pb-2 text-white/60">Connecting… (converting if needed)</p>}
      </FocusGroup>
    </motion.aside>
  );
}

const stateText: Record<string, string> = { loading: "Connecting…", buffering: "Buffering…", paused: "Paused", playing: "Playing", ended: "Finished", error: "Something went wrong", idle: "Ready" };

/** Replaces the player while a receiver plays: what, where, and the controls. */
export function CastOverlay({ status, title, subtitle, onToggle, onSkip, onStop }: { status: CastStatus; title: string; subtitle: string; onToggle: () => void; onSkip: (deltaMs: number) => void; onStop: () => void }) {
  const playing = status.state === "playing" || status.state === "buffering";
  const pct = status.durationMs ? Math.min(100, (status.positionMs / status.durationMs) * 100) : 0;
  return (
    <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="absolute inset-0 z-30 grid place-items-center bg-black/90 backdrop-blur-xl">
      <FocusGroup focusKey="cast-overlay" boundary autoFocus className="flex w-[min(34rem,calc(100vw-4rem))] flex-col items-center gap-6 text-center">
        <span className="grid size-16 place-items-center rounded-full bg-white/10">{status.device?.kind === "airPlay" ? <Airplay className="size-7" /> : <Tv className="size-7" />}</span>
        <div className="flex flex-col gap-1">
          <span className="text-[0.8125rem] font-medium text-white/60">Casting to {status.device?.name}</span>
          <h2 className="text-2xl font-bold tracking-tight text-balance">{title}</h2>
          {subtitle && <span className="text-[0.9375rem] text-white/60">{subtitle}</span>}
        </div>
        <div className="flex w-full flex-col gap-2">
          <div className="h-1 overflow-hidden rounded-full bg-white/20">
            <div className="h-full rounded-full bg-white transition-[width] duration-700 ease-linear" style={{ width: `${pct}%` }} />
          </div>
          <div className="flex justify-between text-xs font-medium text-white/60 tabular-nums">
            <span>{clock(status.positionMs)}</span>
            <span>{status.error ?? stateText[status.state] ?? ""}</span>
            <span>{status.durationMs ? clock(status.durationMs) : ""}</span>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="icon-sm" icon={RotateCcw} label="Back 10 seconds" onClick={() => onSkip(-10_000)} />
          <Button variant="glass" size="icon-lg" icon={playing ? Pause : Play} iconFilled label={playing ? "Pause" : "Play"} onClick={onToggle} autoFocus />
          <Button variant="ghost" size="icon-sm" icon={RotateCw} label="Forward 10 seconds" onClick={() => onSkip(10_000)} />
        </div>
        <Button variant="glass" size="sm" onClick={onStop}>
          Stop casting
        </Button>
      </FocusGroup>
    </motion.div>
  );
}
