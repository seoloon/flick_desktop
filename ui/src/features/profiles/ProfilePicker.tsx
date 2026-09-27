// "Who's watching?": full screen, outside the Shell. The Flick mark breathes
// while cached profiles load, rises, then the people cascade in. Choosing
// one flies its avatar to the centre, asks what must be typed (Flick PIN,
// Plex PIN, one-time sign-ins), rings while the profile loads, and lands on
// Home with the avatar flying into the sidebar.
import { useQuery } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type ReactNode, useCallback, useEffect, useState } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { FlickMark } from "@/components/tv/FlickMark";
import { type PinResult, PinPad } from "@/components/tv/PinPad";
import { ProfileAvatar } from "@/components/tv/ProfileAvatar";
import { api, asError } from "@/ipc/api";
import type { ProfileAccount } from "@/ipc/bindings/ProfileAccount";
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";
import type { ProfileId } from "@/ipc/bindings/ProfileId";
import { ambientColor, ambientReset } from "@/lib/ambient";
import { focusSpring, panelSpring } from "@/lib/motion";
import { nextStage, type PickStage, pendingSignIns, pinError, profilesQuery, switchProfile, visibleProfiles } from "@/lib/profiles";
import { queryClient } from "@/lib/queryClient";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";
import { onAction } from "@/nav/input";
import { AmbientBackdrop } from "@/shell/AmbientBackdrop";
import { TitleBar } from "@/shell/TitleBar";
import { AccountSignIn } from "./AccountSignIn";
import { OtherUserDialog } from "./OtherUserDialog";
import { ProfileEditor } from "./ProfileEditor";
import { ProfileTile } from "./ProfileTile";

const INTRO_MS = 700;

// The chosen card and the tiles are kept here: a switch clears the query
// cache, and nothing on screen may move while the profile list reloads.
// `direct`: nothing to type, so the avatar stays on its tile and flies from
// there to the sidebar; otherwise it rises to the centre for the pad.
type Step = { id: ProfileId; card: ProfileCard; cards: ProfileCard[]; direct: boolean; stage: PickStage; pin?: string; plexPin?: string; pending: ProfileAccount[]; signIn: number; plexWrong?: boolean };

export function ProfilePicker() {
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const query = useQuery(profilesQuery);
  const [intro, setIntro] = useState(true);
  const [focused, setFocused] = useState<ProfileId | null>(null);
  const [step, setStep] = useState<Step | null>(null);
  const [progress, setProgress] = useState(0);
  const [otherUser, setOtherUser] = useState(false);
  const [editing, setEditing] = useState<ProfileCard | "new" | null>(null);

  const data = query.data;
  const cards = step?.cards ?? visibleProfiles(data?.profiles ?? []);
  const selected = step?.card;
  const focusedCard = cards.find((c) => c.id === focused);
  const ready = !intro && (!!data || !!step);
  const onTiles = !step || step.direct;

  useEffect(() => {
    const t = setTimeout(() => setIntro(false), INTRO_MS);
    return () => clearTimeout(t);
  }, []);

  // Silent refresh of server users; the cached list is already on screen.
  useEffect(() => {
    api.profilesDiscover().then(
      (s) => queryClient.setQueryData(profilesQuery.queryKey, s),
      () => undefined,
    );
  }, []);

  // The light follows the focused (or chosen) profile's colour.
  const light = (selected ?? focusedCard ?? cards[0])?.color;
  useEffect(() => {
    if (light) ambientColor(light);
  }, [light]);
  useEffect(() => () => ambientReset(), []);

  const choose = useCallback((card: ProfileCard) => {
    setProgress(0);
    setStep({ id: card.id, card, cards, direct: nextStage(card, null) === "loading", stage: nextStage(card, null), pending: pendingSignIns(card), signIn: 0 });
  }, [cards]);

  // ?pick=<id>: arriving from the sidebar for a profile that needs typing.
  useEffect(() => {
    const pick = params.get("pick");
    const card = pick ? cards.find((c) => c.id === pick) : undefined;
    if (ready && card && !step) choose(card);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready]);

  // Digits 1–9 choose the nth profile.
  useEffect(() => {
    if (step || !ready || otherUser || editing) return;
    const onKey = (e: KeyboardEvent) => {
      const n = Number(e.key);
      const card = Number.isInteger(n) && n >= 1 ? cards[n - 1] : undefined;
      if (!card) return;
      e.preventDefault();
      choose(card);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [step, ready, cards, choose, otherUser, editing]);

  // Back cancels a choice; on the tiles there is nowhere to go back to.
  // Menu (or right click) on a focused tile opens its sheet.
  useEffect(
    () =>
      onAction((a) => {
        if (a.type === "back") {
          setStep((s) => (s && s.stage !== "loading" ? null : s));
          return true;
        }
        if (a.type === "menu" && !step && !otherUser && !editing && focusedCard) {
          setEditing(focusedCard);
          return true;
        }
        return false;
      }),
    [step, focusedCard, otherUser, editing],
  );

  const advance = (s: Step, card: ProfileCard, patch: Partial<Step> = {}) => setStep({ ...s, ...patch, stage: nextStage(card, s.stage) });

  const onPin = async (pin: string): Promise<PinResult> => {
    if (!step || !selected) return "wrong";
    try {
      await api.profileCheckPin(step.id, pin);
      advance(step, selected, { pin });
      return "ok";
    } catch (e) {
      const p = pinError(e);
      if (p) return p;
      toast.error(asError(e).message);
      return "wrong";
    }
  };

  const onPlexPin = async (plexPin: string): Promise<PinResult> => {
    if (!step || !selected) return "wrong";
    advance(step, selected, { plexPin, plexWrong: false }); // plex.tv checks it while loading
    return "ok";
  };

  const onSignedIn = () => {
    if (!step || !selected) return;
    if (step.signIn + 1 < step.pending.length) setStep({ ...step, signIn: step.signIn + 1 });
    else advance(step, selected);
  };

  // Loading: switch in Rust, fill the ring, land on Home.
  useEffect(() => {
    if (step?.stage !== "loading") return;
    let alive = true;
    switchProfile(step.id, { pin: step.pin, plexPin: step.plexPin }).then(
      (outcome) => {
        if (!alive) return;
        setProgress(1);
        setTimeout(() => {
          navigate("/", { replace: true });
          if (outcome.failed.length) toast(`${outcome.failed.join(", ")} did not answer`, { description: "The rest of your library is here." });
        }, 280);
      },
      (e) => {
        if (!alive) return;
        if (pinError(e) === "wrong" && step.plexPin) {
          setStep({ ...step, stage: "plex-pin", plexPin: undefined, plexWrong: true });
          return;
        }
        toast.error(asError(e).message);
        setStep(null);
      },
    );
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step?.stage]);

  const addTile = data?.mode === "serverUsers" ? "Other User" : "Add Profile";

  return (
    <div className="relative h-full overflow-hidden">
      <AmbientBackdrop />
      <TitleBar />
      <div className="relative flex h-full flex-col items-center justify-center gap-12 px-[var(--gutter)]">
        <motion.div layout transition={panelSpring} className="flex flex-col items-center gap-6">
          <motion.div
            animate={intro ? { scale: [0.96, 1, 0.96], opacity: 1 } : { scale: 0.62, opacity: 0.9 }}
            transition={intro ? { duration: 2.4, repeat: Infinity, ease: "easeInOut" } : panelSpring}
            className="drop-shadow-[0_0_40px_rgb(255_255_255/0.25)]"
          >
            <FlickMark title="Flick" className="h-14 w-auto" />
          </motion.div>
          <AnimatePresence>
            {ready && onTiles && (
              <motion.h1
                key="title"
                initial={{ opacity: 0, y: 8 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.45, ease: [0.32, 0.72, 0, 1] }}
                className="text-[2.75rem] leading-none font-bold tracking-tight"
              >
                Who’s watching?
              </motion.h1>
            )}
          </AnimatePresence>
        </motion.div>

        {/* The avatar carries its layoutId from the tile (or the centre) to the sidebar. */}
        <AnimatePresence>
          {ready && onTiles && (
            <FocusGroup key="tiles" focusKey="profiles" autoFocus className="flex max-w-6xl flex-wrap justify-center gap-x-10 gap-y-12">
              {cards.map((card, i) => (
                <ProfileTile
                  key={card.id}
                  card={card}
                  index={i}
                  dimmed={step ? step.id !== card.id : focused !== null && focused !== card.id}
                  onFocused={() => setFocused(card.id)}
                  onSelect={() => !step && choose(card)}
                  overlay={step?.id === card.id && step.stage === "loading" ? <ProgressRing done={progress >= 1} /> : undefined}
                  onEdit={() => setEditing(card)}
                />
              ))}
              <AddTile
                label={addTile}
                index={cards.length}
                onFocused={() => setFocused(null)}
                onSelect={() => !step && (data?.mode === "serverUsers" ? setOtherUser(true) : setEditing("new"))}
              />
            </FocusGroup>
          )}
          {step && !step.direct && selected && (
            <motion.div key="chosen" className="flex flex-col items-center gap-8">
              <div className="relative">
                <ProfileAvatar profile={selected} layoutId={`profile-avatar-${selected.id}`} className="size-40 text-6xl" />
                {step.stage === "loading" && <ProgressRing done={progress >= 1} />}
              </div>
              <motion.p initial={{ opacity: 0 }} animate={{ opacity: 1 }} className="text-2xl font-semibold">
                {selected.name}
              </motion.p>
              <AnimatePresence mode="wait">
                {step.stage === "pin" && (
                  <PinPanel key="pin">
                    <PinPad title="Enter PIN" hint={`${selected.name}’s profile is protected.`} onSubmit={onPin} onCancel={() => setStep(null)} />
                  </PinPanel>
                )}
                {step.stage === "plex-pin" && (
                  <PinPanel key="plex-pin">
                    <PinPad title="Plex PIN" hint={step.plexWrong ? "Wrong Plex PIN. Try again." : "This Plex Home member is protected by Plex."} onSubmit={onPlexPin} onCancel={() => setStep(null)} />
                    {/* Without the Plex PIN the member's servers stay unavailable; the rest loads. */}
                    <div className="mt-4 flex justify-center">
                      <Button size="sm" variant="ghost" onClick={() => advance(step, selected, { plexPin: undefined, plexWrong: false })}>
                        Skip
                      </Button>
                    </div>
                  </PinPanel>
                )}
                {step.stage === "sign-in" && step.pending[step.signIn] && <AccountSignIn key={`sign-in-${step.signIn}`} account={step.pending[step.signIn]!} onDone={onSignedIn} />}
              </AnimatePresence>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
      <OtherUserDialog open={otherUser} onClose={() => setOtherUser(false)} onAdded={() => void query.refetch()} />
      <ProfileEditor target={editing} onClose={() => setEditing(null)} />
    </div>
  );
}

function PinPanel({ children }: { children: ReactNode }) {
  return (
    <motion.div initial={{ opacity: 0, y: 48 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: 24 }} transition={panelSpring} className="glass-strong rounded-[2rem] p-8">
      {children}
    </motion.div>
  );
}

/** Thin ring around the avatar: spins while loading, closes when ready. */
function ProgressRing({ done }: { done: boolean }) {
  return (
    <motion.svg
      viewBox="0 0 100 100"
      className="pointer-events-none absolute -inset-3 size-[calc(100%+1.5rem)]"
      animate={done ? { rotate: 0 } : { rotate: 360 }}
      transition={done ? focusSpring : { duration: 1.2, repeat: Infinity, ease: "linear" }}
    >
      <circle cx="50" cy="50" r="48" fill="none" stroke="rgb(255 255 255 / 0.12)" strokeWidth="1.5" />
      <motion.circle
        cx="50"
        cy="50"
        r="48"
        fill="none"
        stroke="white"
        strokeWidth="1.5"
        strokeLinecap="round"
        initial={{ pathLength: 0.22 }}
        animate={{ pathLength: done ? 1 : 0.22 }}
        transition={{ duration: 0.28, ease: [0.32, 0.72, 0, 1] }}
      />
    </motion.svg>
  );
}

function AddTile({ label, index, onSelect, onFocused }: { label: string; index: number; onSelect: () => void; onFocused: () => void }) {
  const tv = useTv<HTMLButtonElement>({ focusKey: "profile:add", scroll: false, onFocused });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      onClick={onSelect}
      initial={{ opacity: 0, y: 24, scale: 0.92 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={{ ...panelSpring, delay: index * 0.06 }}
      className="flex w-[11rem] cursor-pointer flex-col items-center gap-4 outline-none"
    >
      <motion.span
        animate={{ scale: tv.showFocus ? 1.1 : 1 }}
        transition={focusSpring}
        className={cn("grid size-36 place-items-center rounded-full border-2 border-dashed border-white/25 text-white/60 transition-colors", tv.showFocus && "border-white bg-white text-black")}
      >
        <Plus className="size-10" />
      </motion.span>
      <span className="text-lg font-semibold text-white/70">{label}</span>
    </motion.button>
  );
}
