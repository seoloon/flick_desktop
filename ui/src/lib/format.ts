import type { AudioStream } from "@/ipc/bindings/AudioStream";
import type { MediaItem } from "@/ipc/bindings/MediaItem";
import type { MediaSource } from "@/ipc/bindings/MediaSource";
import type { VideoCodec } from "@/ipc/bindings/VideoCodec";
import type { AudioCodec } from "@/ipc/bindings/AudioCodec";

export function duration(ms: number | null | undefined): string {
  if (!ms || ms <= 0) return "";
  const m = Math.round(ms / 60000);
  const h = Math.floor(m / 60);
  return h ? `${h} h ${String(m % 60).padStart(2, "0")}` : `${m} min`;
}

export function clock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const mm = h ? String(m).padStart(2, "0") : String(m);
  return `${h ? `${h}:` : ""}${mm}:${String(s).padStart(2, "0")}`;
}

export function remaining(item: MediaItem): string {
  const rt = item.runtimeMs ?? 0;
  const left = rt - item.user.positionMs;
  return left > 60000 ? `${duration(left)} left` : "";
}

export function progress(item: MediaItem): number {
  const rt = item.runtimeMs ?? 0;
  return rt > 0 && item.user.positionMs > 0 ? Math.min(1, item.user.positionMs / rt) : 0;
}

export function episodeLabel(item: MediaItem): string {
  const e = item.episode;
  if (!e) return "";
  if (e.seasonNumber != null && e.episodeNumber != null) return `S${e.seasonNumber} E${e.episodeNumber}`;
  if (e.seasonNumber != null) return `Season ${e.seasonNumber}`;
  return "";
}

export function videoCodecLabel(c: VideoCodec): string {
  if (typeof c === "object") return c.other.toUpperCase();
  return { h264: "H.264", hevc: "HEVC", av1: "AV1", vp9: "VP9", vp8: "VP8", mpeg2: "MPEG-2", mpeg4: "MPEG-4", vc1: "VC-1" }[c];
}

export function audioCodecLabel(c: AudioCodec): string {
  if (typeof c === "object") return c.other.toUpperCase();
  return {
    aac: "AAC",
    ac3: "Dolby Digital",
    eac3: "Dolby Digital+",
    dts: "DTS",
    dtshd: "DTS-HD",
    truehd: "TrueHD",
    flac: "FLAC",
    alac: "ALAC",
    opus: "Opus",
    vorbis: "Vorbis",
    mp3: "MP3",
    pcm: "PCM",
  }[c];
}

export function channelsLabel(n: number): string {
  return ({ 1: "Mono", 2: "Stereo", 6: "5.1", 7: "6.1", 8: "7.1" } as Record<number, string>)[n] ?? `${n} ch`;
}

export function resolutionLabel(w: number, h: number): string {
  if (w >= 7000) return "8K";
  if (w >= 3200 || h >= 2000) return "4K";
  if (w >= 1700 || h >= 1000) return "1080p";
  if (w >= 1100 || h >= 700) return "720p";
  return "SD";
}

/** Technical badges from *server metadata*: never claims what the file lacks. */
export function badges(source: MediaSource | undefined): string[] {
  if (!source) return [];
  const out: string[] = [];
  const v = source.video.find((x) => x.isDefault) ?? source.video[0];
  if (v) {
    out.push(resolutionLabel(v.width, v.height));
    const range = v.range.kind;
    if (range === "dolby-vision") out.push("Dolby Vision");
    else if (range === "hdr10") out.push("HDR10");
    else if (range === "hdr10-plus") out.push("HDR10+");
    else if (range === "hlg") out.push("HLG");
  }
  const a: AudioStream | undefined = source.audio.find((x) => x.isDefault) ?? source.audio[0];
  if (a) {
    if (a.spatial === "dolby-atmos") out.push("Atmos");
    else if (a.spatial === "dts-x") out.push("DTS:X");
    out.push(channelsLabel(a.channels));
  }
  return out;
}

export function bitrate(bps: number | null | undefined): string {
  if (!bps) return "";
  return bps >= 1e6 ? `${(bps / 1e6).toFixed(1)} Mb/s` : `${Math.round(bps / 1e3)} kb/s`;
}
