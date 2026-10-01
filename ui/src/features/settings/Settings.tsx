// Settings: sections on the left, grouped rows on the right (tvOS Settings).
// Every change is applied at once and saved in the background.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { SpatialNavigation } from "@noriginmedia/norigin-spatial-navigation";
import { motion } from "motion/react";
import { type ReactNode, useEffect, useMemo } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { toast } from "sonner";
import { Notice } from "@/components/tv/Feedback";
import { Pill } from "@/components/tv/Page";
import type { Choice } from "@/components/tv/Segmented";
import { InfoRow, LinkRow, SelectRow, SettingsGroup, SliderRow, ToggleRow } from "@/components/tv/SettingsList";
import { ProfilesSettings } from "@/features/profiles/ProfilesSettings";
import { ServerManager } from "@/features/servers/Servers";
import { WatchSettings } from "@/features/watch/WatchSettings";
import { TmdbSettings } from "./TmdbSettings";
import { UpdateSettings } from "./UpdateSettings";
import { api, unwrap } from "@/ipc/api";
import type { BitstreamFormat } from "@/ipc/bindings/BitstreamFormat";
import type { CapabilityReport } from "@/ipc/bindings/CapabilityReport";
import type { HdrState } from "@/ipc/bindings/HdrState";
import type { Settings as SettingsModel } from "@/ipc/bindings/Settings";
import { defaultSubtitleFont, installedSubtitleFonts } from "@/lib/fonts";
import { enter, focusSpring, pillSpring } from "@/lib/motion";
import { QUALITY_TIERS, qualityText } from "@/lib/quality";
import { updateSettings, useSettings, useSettingsStore } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { FocusGroup, Screen, useTv } from "@/nav/Focusable";
import { onAction } from "@/nav/input";
import { focusKey } from "@/nav/spatial";

const sections = [
  ["general", "General"],
  ["appearance", "Appearance"],
  ["playback", "Playback"],
  ["audio", "Audio"],
  ["video", "Video & HDR"],
  ["subtitles", "Subtitles"],
  ["downloads", "Downloads"],
  ["servers", "Servers"],
  ["metadata", "Metadata"],
  ["watch", "Watch Together"],
  ["profiles", "Profiles"],
  ["network", "Network & Cache"],
  ["controls", "Controls"],
  ["privacy", "Notifications & Privacy"],
  ["advanced", "Advanced"],
] as const;
type Section = (typeof sections)[number][0];

// Sections that were merged into another; old links and saved URLs still land.
const MOVED: Record<string, Section> = { hdr: "video", cache: "network", performance: "appearance", keyboard: "controls", controller: "controls", notifications: "privacy", debug: "advanced" };
const sectionFromParam = (raw: string | null): Section => {
  if (!raw) return "general";
  if (sections.some(([id]) => id === raw)) return raw as Section;
  return MOVED[raw] ?? "general";
};

const PERSONAL = new Set<Section>(["appearance", "playback", "subtitles"]);

const NAV = "settings-nav";
const CONTENT = "settings-content";

const formatNames: Record<BitstreamFormat, string> = {
  ac3: "Dolby Digital (AC3)",
  eac3: "Dolby Digital Plus (E-AC3, incl. Atmos)",
  dts: "DTS",
  "dts-hd": "DTS-HD MA / DTS:X",
  truehd: "Dolby TrueHD (incl. Atmos)",
};

function hdrText(h: HdrState): string {
  switch (h.state) {
    case "active":
      return `HDR on${h.maxLuminance ? `, peak ${Math.round(h.maxLuminance)} nits` : ""}`;
    case "supportedButOff":
      return "HDR capable, but off in the system settings";
    case "unsupported":
      return "SDR display";
    case "unknown":
      return `Unknown (${h.reason})`;
  }
}

const bitrates: Choice<string>[] = QUALITY_TIERS.map((t) => ({ value: String(t.bitrate ?? 0), label: qualityText(t) }));

const pct = (v: number) => `${Math.round(v * 100)} %`;
const set = (fn: (s: SettingsModel) => void) => updateSettings(fn);

function SectionButton({ id, label, active, onSelect }: { id: Section; label: string; active: boolean; onSelect: () => void }) {
  const tv = useTv<HTMLButtonElement>({ focusKey: `settings:${id}` });
  return (
    <motion.button
      ref={tv.ref}
      type="button"
      {...tv.props}
      aria-current={active ? "page" : undefined}
      onClick={onSelect}
      animate={{ scale: tv.showFocus ? 1.03 : 1 }}
      transition={focusSpring}
      className={cn(
        "relative flex h-10 w-full shrink-0 cursor-pointer items-center rounded-xl px-4 text-left text-[0.9375rem] font-medium transition-colors scroll-my-20",
        tv.showFocus ? "text-black" : active ? "text-white" : "text-white/60 hover:bg-white/[0.06] hover:text-white",
      )}
    >
      {active && !tv.showFocus && <motion.span layoutId="settings-active" className="absolute inset-0 rounded-xl bg-white/14" transition={pillSpring} />}
      {tv.showFocus && <motion.span layoutId="settings-focus" className="absolute inset-0 rounded-xl bg-white shadow-lg" transition={pillSpring} />}
      <span className="relative">{label}</span>
    </motion.button>
  );
}

export function Settings() {
  const [params, setParams] = useSearchParams();
  const section = sectionFromParam(params.get("s"));
  const settings = useSettings();
  const saveError = useSettingsStore((s) => s.saveError);
  const title = sections.find(([id]) => id === section)?.[1] ?? "Settings";
  const personal = params.get("personal") === "1";
  const shown = personal ? sections.filter(([id]) => PERSONAL.has(id)) : sections;

  // Back from the rows returns to the section list first.
  useEffect(() => {
    return onAction((a) => {
      if (a.type !== "back") return false;
      const current = SpatialNavigation.getCurrentFocusKey();
      if (current && SpatialNavigation.isDescendantOf(current, CONTENT)) {
        focusKey(`settings:${section}`);
        return true;
      }
      return false;
    });
  }, [section]);

  return (
    <Screen ready={!!settings}>
      <div className="grid grid-cols-[15rem_minmax(0,1fr)] gap-10 px-[var(--gutter)] pt-[var(--page-top)] pb-24">
        <aside className="sticky top-[var(--page-top)] flex max-h-[calc(100vh-var(--page-top)-2rem)] flex-col gap-4 self-start">
          <h1 className="px-4 text-[2.75rem] leading-none font-bold tracking-tight">{personal ? "Your Preferences" : "Settings"}</h1>
          {personal && <p className="px-4 text-sm text-white/55">Saved for the current profile only. Some rows here are shared by everyone on this computer.</p>}
          <FocusGroup focusKey={NAV} preferredChildFocusKey={`settings:${section}`} fade="y" className="[--fade-size:1.5rem] no-scrollbar -mx-2 flex flex-col gap-0.5 overflow-y-auto px-2 py-2">
            {shown.map(([id, label]) => (
              <SectionButton key={id} id={id} label={label} active={section === id} onSelect={() => setParams(personal ? { s: id, personal: "1" } : { s: id }, { replace: true })} />
            ))}
          </FocusGroup>
        </aside>
        <FocusGroup focusKey={CONTENT} className="flex max-w-3xl min-w-0 flex-col gap-8">
          <motion.h2 key={`t-${section}`} initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={enter} className="pt-3 text-2xl font-bold tracking-tight">
            {title}
          </motion.h2>
          {saveError && <Notice tone="error">Settings could not be saved: {saveError}</Notice>}
          {settings && (
            <motion.div key={section} initial={{ y: 12 }} animate={{ y: 0 }} transition={enter} className="flex flex-col gap-8">
              <SectionBody section={section} s={settings} />
            </motion.div>
          )}
        </FocusGroup>
      </div>
    </Screen>
  );
}

function SectionBody({ section, s }: { section: Section; s: SettingsModel }): ReactNode {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const caps = useQuery({ queryKey: ["capabilities"], queryFn: () => api.capabilities(false) });
  const about = useQuery({ queryKey: ["about"], queryFn: () => api.about() });
  const reprobe = () =>
    api.capabilities(true).then(
      (c) => {
        queryClient.setQueryData(["capabilities"], c);
        toast.success("Displays and audio checked again");
      },
      () => toast.error("The check failed"),
    );

  switch (section) {
    case "general":
      return (
        <>
          <SettingsGroup>
            <ToggleRow label="Start in Flick Frame" hint="Open full screen in TV mode." checked={s.general.startInMaxiFrame} onChange={(v) => set((x) => (x.general.startInMaxiFrame = v))} />
          </SettingsGroup>
          <UpdateSettings s={s} version={about.data?.version} />
        </>
      );
    case "appearance":
      return (
        <>
          <SettingsGroup title="Look" note={<p>The interface stays neutral: colour comes from the artwork you are looking at.</p>}>
            <SliderRow label="Artwork colour in background" value={s.appearance.backgroundIntensity} min={0} max={1} step={0.05} format={pct} onChange={(v) => set((x) => (x.appearance.backgroundIntensity = v))} />
            <SelectRow
              label="Density"
              value={s.appearance.density}
              options={[
                { value: "comfortable", label: "Comfortable" },
                { value: "compact", label: "Compact" },
              ]}
              onChange={(v) => set((x) => (x.appearance.density = v))}
            />
          </SettingsGroup>
          <SettingsGroup title="Motion and effects">
            <SliderRow label="Animations" hint="0 turns motion off. The system's reduced-motion setting always wins." value={s.appearance.animationIntensity} min={0} max={1} step={0.25} format={pct} onChange={(v) => set((x) => (x.appearance.animationIntensity = v))} />
            <ToggleRow label="Frosted glass" hint="Blur behind the sidebar and panels. Costs GPU time on large or 4K screens." checked={s.appearance.blur} onChange={(v) => set((x) => (x.appearance.blur = v))} />
          </SettingsGroup>
        </>
      );
    case "playback":
      return (
        <>
          <LanguageDefaults s={s} />
          <SettingsGroup title="Quality">
            <SelectRow
              label="Streaming quality"
              hint="Heavier files are converted by the server to this bitrate, at the resolution shown. Original keeps Direct Play. Also in the player's menu."
              value={String(s.playback.maxBitrate ?? 0)}
              options={bitrates}
              onChange={(v) => set((x) => (x.playback.maxBitrate = Number(v) || null))}
            />
            <ToggleRow label="Allow Direct Stream" hint="Let the server repackage files it will not send as-is." checked={s.playback.allowDirectStream} onChange={(v) => set((x) => (x.playback.allowDirectStream = v))} />
            <ToggleRow label="Allow server transcoding" hint="When off, files this device cannot play are refused instead of converted." checked={s.playback.allowTranscode} onChange={(v) => set((x) => (x.playback.allowTranscode = v))} />
            <SelectRow
              label="Without a graphics decoder"
              hint="What to do when this computer would decode a stream on the processor."
              value={String(s.playback.transcodeWithoutHwdecMinHeight ?? 0)}
              options={[
                { value: "0", label: "Decode on the processor" },
                { value: "2160", label: "Transcode 4K on the server" },
                { value: "1080", label: "Transcode 1080p+ on the server" },
              ]}
              onChange={(v) => set((x) => (x.playback.transcodeWithoutHwdecMinHeight = Number(v) || null))}
            />
          </SettingsGroup>
          <SettingsGroup title="Behaviour">
            <SelectRow
              label="Resume"
              value={s.playback.resume}
              options={[
                { value: "ask", label: "Ask" },
                { value: "resume", label: "Always resume" },
                { value: "startOver", label: "Always start over" },
              ]}
              onChange={(v) => set((x) => (x.playback.resume = v))}
            />
            <ToggleRow label="Fullscreen on play" hint="Go fullscreen whenever a playback starts." checked={s.playback.fullscreenOnPlay} onChange={(v) => set((x) => (x.playback.fullscreenOnPlay = v))} />
            <ToggleRow label="Play next episode automatically" checked={s.playback.autoplayNext} onChange={(v) => set((x) => (x.playback.autoplayNext = v))} />
            <SliderRow label="Countdown" value={s.playback.autoplayCountdownSecs} min={3} max={30} step={1} format={(v) => `${v} s`} disabled={!s.playback.autoplayNext} onChange={(v) => set((x) => (x.playback.autoplayCountdownSecs = v))} />
            <SelectRow
              label="Intros and recaps"
              hint="Needs markers from the server (Plex, or Jellyfin with a segment plugin)."
              value={s.playback.skipIntro}
              options={[
                { value: "button", label: "Show a skip button" },
                { value: "auto", label: "Skip automatically" },
                { value: "off", label: "Never" },
              ]}
              onChange={(v) => set((x) => (x.playback.skipIntro = v))}
            />
            <SelectRow
              label="Credits"
              hint="Shows on the last episode and on movies. With a next episode, the Up next card appears instead."
              value={s.playback.skipCredits}
              options={[
                { value: "button", label: "Show a skip button" },
                { value: "auto", label: "Skip automatically" },
                { value: "off", label: "Never" },
              ]}
              onChange={(v) => set((x) => (x.playback.skipCredits = v))}
            />
          </SettingsGroup>
        </>
      );
    case "audio":
      return <AudioSection s={s} caps={caps.data} />;
    case "video":
      return (
        <>
          <SettingsGroup
            title="Decoding"
            note={
              caps.data && (
                <p>
                  Graphics decoders on this computer:{" "}
                  {caps.data.video.hardwareProbeOk ? caps.data.video.hardwareDecoders.map((d) => d.profile).join(", ") || "none" : "not detectable on this platform yet"}
                </p>
              )
            }
          >
            <SelectRow
              label="Hardware decoding"
              value={s.video.hardwareDecoding}
              options={[
                { value: "auto", label: "Automatic" },
                { value: "off", label: "Off (processor only)" },
              ]}
              onChange={(v) => set((x) => (x.video.hardwareDecoding = v))}
            />
            <SelectRow
              label="Deinterlacing"
              value={s.video.deinterlace}
              options={[
                { value: "auto", label: "When flagged" },
                { value: "on", label: "Always" },
                { value: "off", label: "Never" },
              ]}
              onChange={(v) => set((x) => (x.video.deinterlace = v))}
            />
            <SelectRow
              label="Frame synchronisation"
              hint="Match to display reduces judder when the refresh rate differs from the film's."
              value={s.video.frameSync}
              options={[
                { value: "audio", label: "Follow audio clock" },
                { value: "displayResample", label: "Match to display" },
              ]}
              onChange={(v) => set((x) => (x.video.frameSync = v))}
            />
            <ToggleRow label="Motion smoothing" hint="Requires Match to display. Some viewers dislike the look." checked={s.video.interpolation} disabled={s.video.frameSync !== "displayResample"} onChange={(v) => set((x) => (x.video.interpolation = v))} />
          </SettingsGroup>
          {caps.data && caps.data.displays.length > 0 && (
            <SettingsGroup title="Displays">
              {caps.data.displays.map((d, i) => (
                <InfoRow key={i} label={d.name}>
                  <span className="inline-flex flex-wrap items-center justify-end gap-2">
                    {d.width}×{d.height}
                    {d.refreshHz ? ` · ${Math.round(d.refreshHz)} Hz` : ""}
                    <Pill tone={d.hdr.state === "supportedButOff" ? "warn" : "plain"}>{hdrText(d.hdr)}</Pill>
                  </span>
                </InfoRow>
              ))}
            </SettingsGroup>
          )}
          <SettingsGroup
            title="HDR"
            note={<p>HDR is sent to the display only when the system itself runs in HDR. Dolby Vision is processed locally and shown as HDR10 or SDR: computers cannot send a Dolby Vision signal to a TV.</p>}
          >
            <SelectRow
              label="HDR output"
              value={s.video.hdr}
              options={[
                { value: "auto", label: "When the display is in HDR" },
                { value: "forceSdr", label: "Always convert to SDR" },
              ]}
              onChange={(v) => set((x) => (x.video.hdr = v))}
            />
            <SelectRow
              label="Tone mapping"
              hint="How HDR is converted for SDR screens."
              value={s.video.toneMapping}
              options={[
                { value: "auto", label: "Automatic" },
                { value: "bt2390", label: "BT.2390" },
                { value: "spline", label: "Spline" },
                { value: "hable", label: "Hable" },
                { value: "mobius", label: "Möbius" },
                { value: "clip", label: "Clip" },
              ]}
              onChange={(v) => set((x) => (x.video.toneMapping = v))}
            />
            <ToggleRow label="Scene-by-scene brightness analysis" hint="Better tone mapping at a small GPU cost." checked={s.video.hdrPeakDetection} onChange={(v) => set((x) => (x.video.hdrPeakDetection = v))} />
            <LinkRow label="Check displays and audio again" onClick={() => void reprobe()} />
          </SettingsGroup>
        </>
      );
    case "subtitles":
      return (
        <>
          <SubtitlePreview s={s} />
          <SettingsGroup note={<p>Applies to plain-text subtitles (SRT, WebVTT). Picture-based subtitles (Blu-ray, DVD) are shown as authored.</p>}>
            <LinkRow label="Default subtitle language" hint="Chosen in Playback, with the default audio." onClick={() => navigate("/settings?s=playback", { replace: true })} />
            <FontRow s={s} />
            <ToggleRow label="Bold" checked={s.subtitles.bold} onChange={(v) => set((x) => (x.subtitles.bold = v))} />
            <SliderRow label="Size" value={s.subtitles.scale} min={0.6} max={2} step={0.05} format={pct} onChange={(v) => set((x) => (x.subtitles.scale = v))} />
            <SelectRow
              label="Colour"
              value={s.subtitles.color}
              options={[
                { value: "#ffffff", label: "White" },
                { value: "#ffe066", label: "Yellow" },
                { value: "#cfe8ff", label: "Pale blue" },
              ]}
              onChange={(v) => set((x) => (x.subtitles.color = v))}
            />
            <SliderRow label="Background" value={s.subtitles.backgroundOpacity} min={0} max={1} step={0.05} format={(v) => (v === 0 ? "None" : pct(v))} onChange={(v) => set((x) => (x.subtitles.backgroundOpacity = v))} />
            <SliderRow label="Outline" value={s.subtitles.outline} min={0} max={6} step={0.5} onChange={(v) => set((x) => (x.subtitles.outline = v))} />
            <SliderRow label="Position" value={s.subtitles.position} min={50} max={100} step={1} format={(v) => `${v} %`} onChange={(v) => set((x) => (x.subtitles.position = v))} />
            <ToggleRow label="Apply to styled subtitles" hint="Override the look of ASS/SSA subtitles too." checked={s.subtitles.overrideAss} onChange={(v) => set((x) => (x.subtitles.overrideAss = v))} />
          </SettingsGroup>
        </>
      );
    case "downloads":
      return <Notice>Offline downloads are not available in this version. Playback always streams from your servers.</Notice>;
    case "servers":
      return (
        <>
          <p className="text-[0.9375rem] leading-relaxed text-muted-foreground">
            Every server you add joins one library. Titles found on several servers appear once and play from the best source. Sign-in tokens are stored in the system keychain
            {about.data && !about.data.credentialStore ? ", which is unavailable: you will need to sign in at each launch" : ""}.
          </p>
          <ServerManager />
        </>
      );
    case "profiles":
      return <ProfilesSettings />;
    case "metadata":
      return <TmdbSettings />;
    case "watch":
      return <WatchSettings s={s} />;
    case "network":
      return (
        <>
          <SettingsGroup title="Connection">
            <SliderRow label="Parallel requests per server" value={s.network.concurrentRequests} min={1} max={16} step={1} onChange={(v) => set((x) => (x.network.concurrentRequests = v))} />
            <SliderRow label="Request timeout" value={s.network.timeoutSecs} min={5} max={60} step={1} format={(v) => `${v} s`} onChange={(v) => set((x) => (x.network.timeoutSecs = v))} />
            <SliderRow label="Playback buffer" value={s.network.bufferMib} min={32} max={1024} step={16} format={(v) => `${v} MiB`} onChange={(v) => set((x) => (x.network.bufferMib = v))} />
            <SelectRow
              label="IP version"
              value={s.network.ipFamily}
              options={[
                { value: "any", label: "Automatic" },
                { value: "v4Only", label: "IPv4 only" },
                { value: "v6Only", label: "IPv6 only" },
              ]}
              onChange={(v) => set((x) => (x.network.ipFamily = v))}
            />
            <ToggleRow
              label="Accept self-signed certificates"
              hint="For home servers with their own certificate. Applies to every server while enabled."
              checked={s.network.allowInvalidCertificates}
              onChange={(v) => set((x) => (x.network.allowInvalidCertificates = v))}
            />
          </SettingsGroup>
          {s.network.allowInvalidCertificates && <Notice tone="warn">Certificate checks are off. Connections could be intercepted on untrusted networks.</Notice>}
          <SettingsGroup title="Cache" note={<p>Your servers stay the source of truth. Cached details refresh after the delay below or when you change something.</p>}>
            <SliderRow label="Artwork cache" value={s.cache.imageCacheMib} min={128} max={8192} step={128} format={(v) => `${(v / 1024).toFixed(1)} GiB`} onChange={(v) => set((x) => (x.cache.imageCacheMib = v))} />
            <SliderRow label="Details refresh after" value={s.cache.metadataTtlSecs} min={30} max={3600} step={30} format={(v) => `${Math.round(v / 60)} min`} onChange={(v) => set((x) => (x.cache.metadataTtlSecs = v))} />
            <LinkRow
              label="Clear artwork cache"
              onClick={() =>
                void api.cacheClear().then(
                  () => toast.success("Artwork cache cleared"),
                  () => toast.error("The cache could not be cleared"),
                )
              }
            />
          </SettingsGroup>
        </>
      );
    case "controls":
      return (
        <>
          <SettingsGroup title="Keyboard & Remote">
            <InfoRow label="Arrows">Move between items. In the player, left and right skip 10 s when controls are hidden.</InfoRow>
            <InfoRow label="Enter">Open or activate</InfoRow>
            <InfoRow label="Escape / Backspace">Back</InfoRow>
            <InfoRow label="Space">Play or pause</InfoRow>
            <InfoRow label="Menu key">Toggle Flick Frame</InfoRow>
            <InfoRow label="Media keys">Play/pause, fast forward and rewind from remotes that send them.</InfoRow>
          </SettingsGroup>
          <SettingsGroup title="Game Controller" note={<p>D-pad or left stick moves, A opens, B goes back, Start plays or pauses, bumpers skip 10 seconds, View toggles Flick Frame.</p>}>
            <ToggleRow label="Use game controllers" checked={s.controller.enabled} onChange={(v) => set((x) => (x.controller.enabled = v))} />
            <SliderRow label="Stick dead zone" value={s.controller.deadzone} min={0.1} max={0.8} step={0.05} format={pct} onChange={(v) => set((x) => (x.controller.deadzone = v))} />
            <ToggleRow label="Swap A and B" hint="Nintendo-style confirm button." checked={s.controller.swapConfirm} onChange={(v) => set((x) => (x.controller.swapConfirm = v))} />
          </SettingsGroup>
        </>
      );
    case "privacy":
      return (
        <>
          <SettingsGroup title="Notifications">
            <ToggleRow label="Next episode" checked={s.notifications.nextEpisode} onChange={(v) => set((x) => (x.notifications.nextEpisode = v))} />
            <ToggleRow label="Server offline" checked={s.notifications.serverOffline} onChange={(v) => set((x) => (x.notifications.serverOffline = v))} />
          </SettingsGroup>
          <SettingsGroup title="Privacy">
            <ToggleRow label="Report progress to servers" hint="Needed for resume points and watched status on your other devices." checked={s.privacy.reportProgress} onChange={(v) => set((x) => (x.privacy.reportProgress = v))} />
          </SettingsGroup>
        </>
      );
    case "advanced": {
      const lib = about.data ? unwrap(about.data.libmpv) : null;
      return (
        <>
          {about.data && (
            <SettingsGroup title="About">
              <InfoRow label="Version">Flick {about.data.version}</InfoRow>
              <InfoRow label="libmpv">{lib?.ok ? `${lib.value[0]} (client API ${lib.value[1][0]}.${lib.value[1][1]})` : lib?.error}</InfoRow>
              <InfoRow label="Display">{about.data.display ?? "—"}</InfoRow>
              <InfoRow label="Settings">{about.data.configDir}</InfoRow>
              <InfoRow label="Cache">{about.data.cacheDir}</InfoRow>
            </SettingsGroup>
          )}
          <SettingsGroup title="Playback engine">
            <SelectRow
              label="Video presentation"
              hint="How video is placed under the interface. Applies to the next playback."
              value={s.advanced.presenter}
              options={[
                { value: "auto", label: "Automatic" },
                { value: "composition", label: "Composition (Windows)" },
                { value: "childWindow", label: "Child window" },
                { value: "dedicatedWindow", label: "Separate window" },
              ]}
              onChange={(v) => set((x) => (x.advanced.presenter = v))}
            />
          </SettingsGroup>
          <SettingsGroup title="Diagnostics">
            <SelectRow
              label="Log detail"
              value={s.advanced.logLevel}
              options={[
                { value: "info", label: "Normal" },
                { value: "debug", label: "Detailed" },
                { value: "trace", label: "Everything" },
              ]}
              onChange={(v) => set((x) => (x.advanced.logLevel = v))}
            />
            <LinkRow label="Open diagnostics" onClick={() => navigate("/debug")} />
          </SettingsGroup>
        </>
      );
    }
  }
}

/** Installed fonts only: libass finds a family by the same system lookup as the WebView. */
function FontRow({ s }: { s: SettingsModel }) {
  const fonts = useMemo(installedSubtitleFonts, []);
  const current = s.subtitles.fontFamily;
  const options: Choice<string>[] = [
    { value: "", label: `Automatic (${defaultSubtitleFont()})` },
    ...fonts.filter((f) => f !== defaultSubtitleFont()).map((f) => ({ value: f, label: f })),
    // A family typed in the settings file stays selectable.
    ...(current && !fonts.includes(current) && current !== defaultSubtitleFont() ? [{ value: current, label: current }] : []),
  ];
  return <SelectRow label="Font" value={current} options={options} onChange={(v) => set((x) => (x.subtitles.fontFamily = v))} />;
}

/**
 * A still frame with a subtitle drawn from the current settings. CSS stands
 * in for libass: same family, weight, colour, relative size, edge and box,
 * close enough to judge a change without starting a video.
 */
function SubtitlePreview({ s }: { s: SettingsModel }) {
  const sub = s.subtitles;
  const family = sub.fontFamily || defaultSubtitleFont();
  // mpv measures in lines of a 720-line frame; the frame is 21:9, so one of
  // those units is (9 / 21 / 720) of the container width.
  const u = (v: number) => `${((100 * 9) / 21) * (v / 720)}cqw`;
  // An ASS font size is the font's full line height (≈ 1.15 em for sans faces).
  const size = u((40 * sub.scale) / 1.15);
  const box = sub.backgroundOpacity > 0;
  return (
    <div
      aria-hidden
      className="relative aspect-[21/9] w-full overflow-hidden rounded-2xl bg-[radial-gradient(120%_90%_at_30%_20%,#3b4a5c,transparent_60%),radial-gradient(90%_80%_at_85%_80%,#6b4a2e,transparent_60%),linear-gradient(#1b2027,#0d0f12)] [container-type:inline-size]"
    >
      {/* sub-pos 100 = bottom, above mpv's 22-unit margin. */}
      <div className="absolute inset-x-0 flex justify-center px-6" style={{ bottom: `calc(${100 - sub.position}% + ${u(22)})` }}>
        <span
          className="text-center leading-[1.15]"
          style={{
            fontFamily: `"${family}", sans-serif`,
            fontWeight: sub.bold ? 700 : 400,
            fontSize: size,
            color: sub.color,
            padding: box ? "0.05em 0.3em" : undefined,
            background: box ? `color-mix(in srgb, ${sub.background} ${Math.round(sub.backgroundOpacity * 100)}%, transparent)` : undefined,
            // A centred stroke painted under the fill: an outer edge of `outline`.
            WebkitTextStroke: !box && sub.outline > 0 ? `${u(sub.outline * 2)} #000` : undefined,
            paintOrder: "stroke fill",
            textShadow: box ? undefined : `${u(1.2)} ${u(1.2)} ${u(1.5)} rgb(0 0 0 / 0.55)`,
          }}
        >
          We'll find her before nightfall,
          <br />
          and then we all go home.
        </span>
      </div>
    </div>
  );
}

// Languages the engine matches against file tags (ISO 639-1/2, see
// crates/playback/src/tracks.rs `same_language`), named in their own language.
const LANGUAGE_CODES = ["fr", "en", "de", "es", "it", "pt", "nl", "ru", "ja", "ko", "zh"];
function nativeName(code: string): string {
  try {
    const name = new Intl.DisplayNames([code], { type: "language" }).of(code) ?? code;
    return name.charAt(0).toLocaleUpperCase(code) + name.slice(1);
  } catch {
    return code.toUpperCase();
  }
}
const languages: Choice<string>[] = LANGUAGE_CODES.map((code) => ({ value: code, label: nativeName(code) }));

/**
 * Default audio and subtitle languages. Subtitles map onto the engine's
 * modes: a language with "only when audio differs" is `smart` (subtitles
 * appear when the audio is in another language), without it `always`.
 */
function LanguageDefaults({ s }: { s: SettingsModel }) {
  const mode = s.subtitles.mode;
  const subLang = s.subtitles.languages[0];
  const subValue = mode === "off" ? "off" : mode === "forcedOnly" || !subLang ? "forced" : subLang;
  const hasLanguage = subValue !== "off" && subValue !== "forced";
  return (
    <SettingsGroup title="Languages" note={<p>Used when a title offers the language; otherwise the file's own default track plays. You can always switch in the player.</p>}>
      <SelectRow
        label="Default audio"
        value={s.playback.preferredAudioLanguages[0] ?? ""}
        options={[{ value: "", label: "Original" }, ...languages]}
        onChange={(v) => set((x) => (x.playback.preferredAudioLanguages = v ? [v] : []))}
      />
      <SelectRow
        label="Default subtitles"
        hint={subValue === "forced" ? "Only signs and foreign dialogue." : undefined}
        value={subValue}
        options={[{ value: "off", label: "Off" }, { value: "forced", label: "Forced only" }, ...languages]}
        onChange={(v) =>
          set((x) => {
            if (v === "off") x.subtitles.mode = "off";
            else if (v === "forced") x.subtitles.mode = "forcedOnly";
            else {
              // Keep "only when audio differs" as it was; on by default.
              if (x.subtitles.mode !== "always") x.subtitles.mode = "smart";
              x.subtitles.languages = [v];
            }
          })
        }
      />
      <ToggleRow
        label="Only when audio differs"
        hint={hasLanguage ? `Hide ${nativeName(subLang!)} subtitles when the audio is already in ${nativeName(subLang!)}.` : "Choose a subtitle language first."}
        checked={hasLanguage && mode === "smart"}
        disabled={!hasLanguage}
        onChange={(v) => set((x) => (x.subtitles.mode = v ? "smart" : "always"))}
      />
    </SettingsGroup>
  );
}

function AudioSection({ s, caps }: { s: SettingsModel; caps: CapabilityReport | undefined }) {
  const a = s.audio;
  const device = caps?.audio.devices.find((d) => d.id === (a.device ?? caps?.audio.defaultDevice));
  const accepted = (f: BitstreamFormat) => {
    const p = device?.passthrough;
    return p?.state === "probed" && p.formats.includes(f);
  };
  const devices: Choice<string>[] = [{ value: "", label: "System default" }, ...(caps?.audio.devices ?? []).map((d) => ({ value: d.id, label: d.name }))];
  return (
    <>
      <SettingsGroup title="Output">
        <SelectRow label="Device" value={a.device ?? ""} options={devices} onChange={(v) => set((x) => (x.audio.device = v || null))} />
        <SelectRow
          label="Channels"
          hint={device ? `The system mixes to ${device.channels} channels (${device.channelLayout ?? "unknown layout"}) on this device.` : undefined}
          value={a.channels}
          options={[
            { value: "auto", label: "Follow system" },
            { value: "stereo", label: "Stereo" },
            { value: "surround51", label: "5.1" },
            { value: "surround71", label: "7.1" },
          ]}
          onChange={(v) => set((x) => (x.audio.channels = v))}
        />
        <SliderRow label="Volume" value={a.volume} min={0} max={100} step={1} format={(v) => `${v} %`} onChange={(v) => set((x) => (x.audio.volume = v))} />
        <ToggleRow
          label="Volume boost"
          hint="Amplifies quiet mixes past 100 %; a limiter keeps loud scenes from clipping. Not applied to passthrough."
          checked={a.volumeBoost}
          onChange={(v) => set((x) => (x.audio.volumeBoost = v))}
        />
        <SliderRow label="Boost" value={a.volumeBoostPercent} min={110} max={300} step={10} format={(v) => `${v} %`} disabled={!a.volumeBoost} onChange={(v) => set((x) => (x.audio.volumeBoostPercent = v))} />
        <SelectRow
          label="Volume levelling"
          value={a.normalization}
          options={[
            { value: "off", label: "Off" },
            { value: "nightMode", label: "Night mode" },
            { value: "loudness", label: "Loudness (EBU R128)" },
          ]}
          onChange={(v) => set((x) => (x.audio.normalization = v))}
        />
        <ToggleRow label="Exclusive mode" hint="Bit-perfect output. Other apps are silent while playing." checked={a.exclusive} onChange={(v) => set((x) => (x.audio.exclusive = v))} />
      </SettingsGroup>
      <SettingsGroup
        title="Passthrough to an AV receiver"
        note={
          <p>
            Sends Dolby and DTS tracks untouched so your receiver decodes them. Atmos and DTS:X are only reproduced this way. Formats your device refused during the last check are marked; the player never forces them, and falls
            back to decoding if the receiver rejects a stream.
          </p>
        }
      >
        <ToggleRow label="Enable passthrough" checked={a.passthrough} onChange={(v) => set((x) => (x.audio.passthrough = v))} />
        {(Object.keys(formatNames) as BitstreamFormat[]).map((f) => (
          <ToggleRow
            key={f}
            label={formatNames[f]}
            hint={accepted(f) ? "Accepted by this device" : "Not accepted by this device"}
            checked={a.passthroughFormats.includes(f)}
            disabled={!a.passthrough}
            onChange={(on) => set((x) => (x.audio.passthroughFormats = on ? [...x.audio.passthroughFormats, f] : x.audio.passthroughFormats.filter((y) => y !== f)))}
          />
        ))}
        <ToggleRow
          label="Re-encode surround to Dolby Digital"
          hint="For optical (S/PDIF) receivers: turns any 5.1/7.1 track into Dolby Digital 5.1. Lossy."
          checked={a.ac3Reencode}
          onChange={(v) => set((x) => (x.audio.ac3Reencode = v))}
        />
      </SettingsGroup>
    </>
  );
}
