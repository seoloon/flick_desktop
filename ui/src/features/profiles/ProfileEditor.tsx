// A profile's sheet: name, colour, picture, PIN, accounts, delete/hide.
// A protected profile is unlocked with its PIN first; that PIN then
// authorises every change in the sheet (Rust checks it again). Linking an
// account another protected profile uses asks for that profile's PIN.
import { useQuery } from "@tanstack/react-query";
import { Check } from "lucide-react";
import { motion } from "motion/react";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { Notice } from "@/components/tv/Feedback";
import { type PinResult, PinPad } from "@/components/tv/PinPad";
import { ProfileAvatar } from "@/components/tv/ProfileAvatar";
import { ProviderLogo } from "@/components/tv/ServerBadge";
import { Segmented } from "@/components/tv/Segmented";
import { Switch } from "@/components/tv/Switch";
import { TextField } from "@/components/tv/TextField";
import { TvDialog } from "@/components/tv/TvDialog";
import { api, asError } from "@/ipc/api";
import type { ProfileEdit } from "@/ipc/app-types";
import type { AvatarStyle } from "@/ipc/bindings/AvatarStyle";
import type { ProfileCard } from "@/ipc/bindings/ProfileCard";
import type { ProfileId } from "@/ipc/bindings/ProfileId";
import type { ServerId } from "@/ipc/bindings/ServerId";
import { focusSpring } from "@/lib/motion";
import { allServersQuery, PROFILE_COLORS, pinError, profilesQuery } from "@/lib/profiles";
import { queryClient } from "@/lib/queryClient";
import { cn } from "@/lib/utils";
import { FocusGroup, useTv } from "@/nav/Focusable";

type Target = ProfileCard | "new" | null;
type PinStep = null | { stage: "new" } | { stage: "confirm"; first: string };

const refresh = () => Promise.all([queryClient.invalidateQueries({ queryKey: profilesQuery.queryKey }), queryClient.invalidateQueries({ queryKey: ["servers"] })]);

export function ProfileEditor({ target, onClose }: { target: Target; onClose: () => void }) {
  const navigate = useNavigate();
  const state = useQuery(profilesQuery).data;
  // Every connection on this computer, not just the active profile's.
  const servers = useQuery(allServersQuery).data ?? [];
  const card = target && target !== "new" ? target : null;
  const [unlock, setUnlock] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [color, setColor] = useState(PROFILE_COLORS[0]!);
  const [avatar, setAvatar] = useState<AvatarStyle>("server");
  // Only a changed picture is sent: a missing one must not flip to Initials.
  const [avatarTouched, setAvatarTouched] = useState(false);
  const [links, setLinks] = useState<ServerId[]>([]);
  const [pinStep, setPinStep] = useState<PinStep>(null);
  const [orphans, setOrphans] = useState<ServerId[]>([]);
  const [leaveAfter, setLeaveAfter] = useState(false);
  // A new profile already created, so a retried save never creates it twice.
  const [created, setCreated] = useState<ProfileId | null>(null);
  const [ownerRetry, setOwnerRetry] = useState<((pin: string) => Promise<PinResult>) | null>(null);

  useEffect(() => {
    setUnlock(null);
    setPinStep(null);
    setOrphans([]);
    setLeaveAfter(false);
    setAvatarTouched(false);
    setCreated(null);
    setOwnerRetry(null);
    if (card) {
      setName(card.name);
      setColor(card.color);
      setAvatar(card.avatarKey ? "server" : "initials");
      setLinks(card.accounts.flatMap((a) => (a.connection ? [a.connection] : [])));
    } else if (target === "new") {
      setName("");
      setColor(PROFILE_COLORS[(state?.profiles.length ?? 0) % PROFILE_COLORS.length]!);
      setAvatar("server");
      setLinks([]);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target]);

  if (!target || !state) return null;
  const mode = state.mode;
  const derived = mode === "serverUsers";
  const locked = !!card?.locked && unlock === null;
  const isActive = card?.id === state.active;

  const fail = (e: unknown) => toast.error(asError(e).message);

  const leaveTo = (path: string) => {
    onClose();
    navigate(path);
  };

  /** Saves `edit`; a PIN refusal (the profile itself is unlocked) means an
   * account belongs to a protected profile: ask for its PIN and retry. */
  const update = async (id: ProfileId, edit: ProfileEdit, ownerPin: string | null = null): Promise<PinResult> => {
    try {
      await api.profileUpdate(id, edit, unlock, ownerPin);
    } catch (e) {
      const p = pinError(e);
      if (p && ownerPin === null) {
        setOwnerRetry(() => (typed: string) => update(id, edit, typed));
        return "ok";
      }
      if (p) return p;
      setOwnerRetry(null);
      fail(e);
      return "ok";
    }
    setOwnerRetry(null);
    await refresh();
    onClose();
    return "ok";
  };

  const save = async () => {
    try {
      if (!card) {
        const id = created ?? (await api.profileCreate(name, color));
        setCreated(id);
        if (mode === "linked" && links.length) {
          await update(id, { name: null, color: null, avatar: null, connections: links, hidden: null });
        } else {
          await refresh();
          onClose();
        }
      } else {
        await update(card.id, { name, color, avatar: avatarTouched ? avatar : null, connections: mode === "linked" ? links : null, hidden: null });
      }
    } catch (e) {
      fail(e);
    }
  };

  const setHidden = async (hidden: boolean) => {
    if (!card) return;
    try {
      await api.profileUpdate(card.id, { name: null, color: null, avatar: null, connections: null, hidden }, unlock, null);
      await refresh();
      onClose();
    } catch (e) {
      fail(e);
    }
  };

  const remove = async () => {
    if (!card) return;
    try {
      const left = await api.profileDelete(card.id, unlock);
      await refresh();
      if (left.length) {
        setLeaveAfter(isActive);
        setOrphans(left);
      } else {
        onClose();
        if (isActive) navigate("/profiles");
      }
    } catch (e) {
      fail(e);
    }
  };

  const finishDelete = () => {
    onClose();
    if (leaveAfter) navigate("/profiles");
  };

  const detach = async (connection: ServerId) => {
    if (!card) return;
    try {
      await api.profileDetach(card.id, connection, unlock);
      await refresh();
      onClose();
    } catch (e) {
      fail(e);
    }
  };

  const onUnlock = async (pin: string): Promise<PinResult> => {
    if (!card) return "wrong";
    try {
      await api.profileCheckPin(card.id, pin);
      setUnlock(pin);
      return "ok";
    } catch (e) {
      return pinError(e) ?? "wrong";
    }
  };

  const onNewPin = async (pin: string): Promise<PinResult> => {
    if (!card || !pinStep) return "wrong";
    if (pinStep.stage === "new") {
      setPinStep({ stage: "confirm", first: pin });
      return "ok";
    }
    if (pin !== pinStep.first) {
      setPinStep({ stage: "new" });
      return "wrong";
    }
    try {
      await api.profileSetPin(card.id, unlock, pin);
      setUnlock(pin);
      setPinStep(null);
      await refresh();
      toast.success("PIN set");
      return "ok";
    } catch (e) {
      return pinError(e) ?? "wrong";
    }
  };

  const removePin = async () => {
    if (!card) return;
    try {
      await api.profileSetPin(card.id, unlock, null);
      await refresh();
      toast.success("PIN removed");
      onClose();
    } catch (e) {
      fail(e);
    }
  };

  const title = card ? card.name : "New Profile";

  return (
    <TvDialog open onClose={onClose} title={title}>
      {locked ? (
        <PinPad title="Enter PIN" hint="This profile is protected." onSubmit={onUnlock} onCancel={onClose} />
      ) : ownerRetry ? (
        <PinPad
          title="Enter PIN"
          hint="This account belongs to a protected profile."
          onSubmit={ownerRetry}
          onCancel={() => {
            setOwnerRetry(null);
            if (created) void refresh();
          }}
        />
      ) : pinStep ? (
        <PinPad key={pinStep.stage} title={pinStep.stage === "new" ? "Choose a PIN" : "Confirm the PIN"} hint="4 digits." onSubmit={onNewPin} onCancel={() => setPinStep(null)} />
      ) : orphans.length ? (
        <>
          <Notice>
            {orphans.length === 1 ? "One sign-in is" : `${orphans.length} sign-ins are`} no longer used by any profile. Remove {orphans.length === 1 ? "it" : "them"} from this computer?
          </Notice>
          <FocusGroup className="flex gap-3">
            <Button
              variant="danger"
              onClick={() =>
                void Promise.all(orphans.map((id) => api.serverRemove(id)))
                  .then(refresh)
                  .then(finishDelete, fail)
              }
            >
              Remove
            </Button>
            <Button onClick={finishDelete}>Keep</Button>
          </FocusGroup>
        </>
      ) : (
        <>
          <div className="flex items-center gap-5">
            <ProfileAvatar profile={{ id: card?.id ?? "new", name: name || "?", color, avatarKey: avatar === "server" ? (card?.avatarKey ?? null) : null }} className="size-20 text-3xl" />
            <div className="min-w-0 flex-1">
              <TextField label="Name" value={name} onChange={setName} autoFocus={!card} />
            </div>
          </div>

          <FocusGroup className="flex flex-wrap gap-3" aria-label="Colour">
            {PROFILE_COLORS.map((c) => (
              <Swatch key={c} color={c} selected={c === color} onSelect={() => setColor(c)} />
            ))}
          </FocusGroup>

          {card && card.accounts.length > 0 ? (
            <Segmented
              label="Picture"
              value={avatar}
              options={[
                { value: "server", label: "From server" },
                { value: "initials", label: "Initials" },
              ]}
              onChange={(v) => {
                setAvatar(v);
                setAvatarTouched(true);
              }}
            />
          ) : null}

          {card && (
            <section className="flex flex-col gap-2">
              <h3 className="text-sm font-semibold text-white/60">Accounts</h3>
              {mode === "linked"
                ? servers.map((e) => (
                    <div key={e.server.id} className="flex items-center gap-3 rounded-2xl bg-white/[0.05] px-4 py-2.5">
                      <ProviderLogo kind={e.server.kind} />
                      <span className="flex-1 truncate">
                        {e.server.user.name} · {e.server.name}
                      </span>
                      <Switch
                        label={`Use ${e.server.name} as ${e.server.user.name}`}
                        checked={links.includes(e.server.id)}
                        onChange={(on) => setLinks((l) => (on ? [...l, e.server.id] : l.filter((x) => x !== e.server.id)))}
                      />
                    </div>
                  ))
                : card.accounts.map((a) => (
                    <div key={`${a.kind}:${a.serverName}:${a.userName}`} className="flex items-center gap-3 rounded-2xl bg-white/[0.05] px-4 py-2.5">
                      <ProviderLogo kind={a.kind} />
                      <span className="flex-1 truncate">
                        {a.userName} · {a.serverName}
                        {a.state !== "connected" && <span className="text-white/50"> · {a.state === "pending" ? "sign-in needed" : a.state === "disabled" ? "off" : "offline"}</span>}
                      </span>
                      {derived && a.connection && card.accounts.length > 1 && (
                        <Button size="sm" variant="ghost" onClick={() => void detach(a.connection!)}>
                          Detach
                        </Button>
                      )}
                    </div>
                  ))}
              {mode === "local" &&
                (isActive ? (
                  <Button size="sm" onClick={() => leaveTo("/servers")}>
                    Add a Server
                  </Button>
                ) : (
                  <p className="text-sm text-white/50">Switch to this profile to add its servers.</p>
                ))}
            </section>
          )}

          <FocusGroup className="flex flex-wrap gap-3 pt-2">
            <Button variant="primary" disabled={!name.trim()} onClick={() => void save()}>
              {card ? "Save" : "Create"}
            </Button>
            {card && <Button onClick={() => setPinStep({ stage: "new" })}>{card.locked ? "Change PIN" : "Set PIN"}</Button>}
            {card?.locked && <Button onClick={() => void removePin()}>Remove PIN</Button>}
            {card && isActive && (
              <Button variant="ghost" onClick={() => leaveTo("/settings?personal=1&s=playback")}>
                Preferences
              </Button>
            )}
            {card && derived && (
              <Button variant="ghost" onClick={() => void setHidden(!card.hidden)}>
                {card.hidden ? "Show" : "Hide"}
              </Button>
            )}
            {card && !derived && (
              <Button variant="danger" onClick={() => void remove()}>
                Delete
              </Button>
            )}
          </FocusGroup>
        </>
      )}
    </TvDialog>
  );
}

function Swatch({ color, selected, onSelect }: { color: string; selected: boolean; onSelect: () => void }) {
  const tv = useTv<HTMLButtonElement>({ scroll: false });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      aria-label={`Colour ${color}`}
      aria-pressed={selected}
      onClick={onSelect}
      animate={{ scale: tv.showFocus ? 1.15 : 1 }}
      transition={focusSpring}
      className={cn("grid size-10 cursor-pointer place-items-center rounded-full", (selected || tv.showFocus) && "ring-2 ring-white ring-offset-2 ring-offset-black/40")}
      style={{ background: color }}
    >
      {selected && <Check className="size-5 text-white drop-shadow" />}
    </motion.button>
  );
}
