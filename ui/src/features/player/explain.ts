// Plain-language explanations of what the playback pipeline does. Wording is
// deliberately literal: it never claims more than the decision states.
import type { AudioOutputPlan } from "@/ipc/bindings/AudioOutputPlan";
import type { PlaybackDecision } from "@/ipc/bindings/PlaybackDecision";
import type { StrategyLabel } from "@/ipc/bindings/StrategyLabel";
import type { VideoOutputPlan } from "@/ipc/bindings/VideoOutputPlan";
import { channelsLabel } from "@/lib/format";

export const strategyText: Record<StrategyLabel, { title: string; body: string }> = {
  directPlay: { title: "Direct Play", body: "The original file is streamed untouched and played as-is." },
  directStream: { title: "Direct Stream", body: "The server repackages the file without re-encoding. Quality is unchanged." },
  localDecode: {
    title: "Local Decode",
    body: "The original file is streamed untouched; this device converts it for your screen or speakers.",
  },
  serverTranscode: { title: "Server Transcode", body: "The server re-encodes the video or audio before sending it." },
};

export function videoText(v: VideoOutputPlan): string {
  switch (v.mode) {
    case "sdr":
      return "SDR";
    case "hdrPassthrough":
      return `${v.format} sent to the display`;
    case "toneMapToSdr":
      return `HDR converted to SDR (${v.reason})`;
    case "dolbyVisionReshape":
      return `Dolby Vision processed locally, output as ${v.output}`;
    case "serverDetermined":
      return "Chosen by the server's transcoder";
  }
}

const formatNames: Record<string, string> = {
  ac3: "Dolby Digital",
  eac3: "Dolby Digital+",
  dts: "DTS",
  "dts-hd": "DTS-HD",
  truehd: "Dolby TrueHD",
};

export function audioText(a: AudioOutputPlan): string {
  switch (a.mode) {
    case "bitstream":
      return a.reencoded
        ? `Re-encoded to Dolby Digital for ${a.device}`
        : `${formatNames[a.format] ?? a.format} passed through to ${a.device}`;
    case "pcm": {
      const out = channelsLabel(a.outputChannels);
      const src = channelsLabel(a.sourceChannels);
      const base = a.downmix ? `${src} mixed down to ${out}` : `${src} decoded`;
      return a.spatialLost ? `${base} (Atmos/DTS:X objects not reproduced)` : base;
    }
    case "serverDetermined":
      return "Chosen by the server's transcoder";
    case "none":
      return "No audio";
  }
}

export function summary(d: PlaybackDecision) {
  return { strategy: strategyText[d.label], video: videoText(d.video), audio: audioText(d.audio) };
}
