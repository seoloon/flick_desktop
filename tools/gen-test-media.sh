#!/usr/bin/env bash
# Generates the synthetic playback-validation corpus (see docs/PLAYBACK_VALIDATION.md).
#
# The files are *synthetic*: the "HDR" clip is an SDR test pattern tagged with
# PQ/BT.2020 + HDR10 static metadata. It validates the signalling path
# (decoder -> renderer -> swapchain colourspace), not picture quality.
# Real-world validation must also be done with genuine mastered content.
#
# Each audio channel carries a distinct sine frequency so channel mapping can be
# verified with a spectrum analyser / AVR channel indicator.
set -euo pipefail

OUT="${1:-test-media}"
DUR="${DUR:-6}"
mkdir -p "$OUT"
FF=(ffmpeg -hide_banner -loglevel error -y)

# Per-channel tones: FL 220, FR 330, FC 440, LFE 55, BL 550, BR 660, SL 770, SR 880
tone() { # $1 = channel layout, $2.. = frequencies
  local layout=$1; shift
  local expr="" f
  for f in "$@"; do expr+="0.25*sin(${f}*2*PI*t)|"; done
  echo "aevalsrc=${expr%|}:s=48000:d=${DUR}:c=${layout}"
}
T20=$(tone stereo 220 330)
T51=$(tone 5.1 220 330 440 55 550 660)
T71=$(tone 7.1 220 330 440 55 550 660 770 880)

pattern() { echo "testsrc2=size=$1:rate=$2:duration=${DUR}"; }

cat > "$OUT/en.srt" <<'EOF'
1
00:00:00,500 --> 00:00:03,000
English subtitle line one

2
00:00:03,200 --> 00:00:05,800
English subtitle line two
EOF

cat > "$OUT/fr_forced.ass" <<'EOF'
[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,64,&H00FFFFFF,&H000000FF,&H00000000,&H64000000,0,0,0,0,100,100,0,0,1,3,1,8,40,40,60,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:01.00,0:00:05.00,Default,,0,0,0,,{\fad(200,200)}Sous-titre ASS forcé (haut)
EOF

echo "[1/8] H.264 1080p + AAC stereo (mp4)"
"${FF[@]}" -f lavfi -i "$(pattern 1920x1080 24000/1001)" -f lavfi -i "$T20" \
  -c:v libx264 -preset veryfast -profile:v high -pix_fmt yuv420p \
  -c:a aac -b:a 192k -movflags +faststart -shortest "$OUT/01_h264_1080p_aac20.mp4"

echo "[2/8] HEVC Main10 2160p HDR10 (PQ/BT.2020 + mastering metadata) + E-AC3 5.1"
"${FF[@]}" -f lavfi -i "$(pattern 3840x2160 24000/1001)" -f lavfi -i "$T51" \
  -c:v libx265 -preset ultrafast -pix_fmt yuv420p10le \
  -x265-params "log-level=error:hdr10=1:repeat-headers=1:colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1):max-cll=1000,400" \
  -color_primaries bt2020 -color_trc smpte2084 -colorspace bt2020nc \
  -c:a eac3 -b:a 640k -shortest "$OUT/02_hevc10_2160p_hdr10_eac3_51.mkv"

echo "[3/8] HEVC 1080p + FLAC 7.1 (en) + TrueHD 5.1 (en) + AC3 5.1 (fr) + SRT (en) + ASS forced (fr)"
# NB: FFmpeg's TrueHD encoder cannot produce 7.1 (it silently maps to 5.1), and
# its E-AC3/DTS encoders top out at 5.1 too. Real 7.1 lossy/lossless bitstreams
# (TrueHD 7.1, DTS-HD MA 7.1, Atmos) must come from genuine sample files.
"${FF[@]}" -f lavfi -i "$(pattern 1920x1080 24)" -f lavfi -i "$T71" -f lavfi -i "$T51"   -i "$OUT/en.srt" -i "$OUT/fr_forced.ass"   -map 0:v -map 1:a -map 2:a -map 2:a -map 3:s -map 4:s   -c:v libx265 -preset ultrafast -x265-params log-level=error -pix_fmt yuv420p   -c:a:0 flac -c:a:1 truehd -strict -2 -c:a:2 ac3 -b:a:2 448k -c:s:0 srt -c:s:1 ass   -metadata:s:a:0 language=eng -metadata:s:a:0 title="FLAC 7.1"   -metadata:s:a:1 language=eng -metadata:s:a:1 title="TrueHD 5.1"   -metadata:s:a:2 language=fre -metadata:s:a:2 title="AC3 5.1"   -metadata:s:s:0 language=eng -metadata:s:s:1 language=fre -disposition:s:1 forced   -shortest "$OUT/03_hevc_1080p_flac71_truehd51_ac351_subs.mkv"

echo "[4/8] AV1 10-bit 1080p + FLAC 5.1"
"${FF[@]}" -f lavfi -i "$(pattern 1920x1080 30)" -f lavfi -i "$T51" \
  -c:v libsvtav1 -preset 12 -svtav1-params log-level=0 -pix_fmt yuv420p10le -c:a flac -shortest "$OUT/04_av1_1080p_flac51.mkv"

echo "[5/8] VP9 1080p + DTS 5.1"
"${FF[@]}" -f lavfi -i "$(pattern 1920x1080 25)" -f lavfi -i "$T51" \
  -c:v libvpx-vp9 -deadline realtime -cpu-used 8 -b:v 4M -c:a dca -strict -2 -b:a 1509k \
  -shortest "$OUT/05_vp9_1080p_dts51.mkv"

echo "[6/8] Two video tracks (H.264 1080p + H.264 720p) + AAC"
"${FF[@]}" -f lavfi -i "$(pattern 1920x1080 24)" -f lavfi -i "smptehdbars=size=1280x720:rate=24:duration=${DUR}" -f lavfi -i "$T20" \
  -map 0:v -map 1:v -map 2:a -c:v libx264 -preset veryfast -pix_fmt yuv420p -c:a aac \
  -metadata:s:v:0 title="Main" -metadata:s:v:1 title="Alt angle" -shortest "$OUT/06_multi_video.mkv"

echo "[7/8] H.264 2160p60 (high frame rate) + AC3 5.1"
"${FF[@]}" -f lavfi -i "$(pattern 3840x2160 60)" -f lavfi -i "$T51" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p -c:a ac3 -b:a 448k -shortest "$OUT/07_h264_2160p60_ac351.mkv"

echo "[8/8] Long H.264 (7 min) for resume/progress tests (servers ignore resume points on short media)"
"${FF[@]}" -f lavfi -i "testsrc2=size=640x360:rate=5:duration=420" -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=420"   -c:v libx264 -preset veryfast -crf 35 -pix_fmt yuv420p -c:a aac -b:a 64k -shortest "$OUT/08_h264_long_7min.mkv"

rm -f "$OUT/en.srt" "$OUT/fr_forced.ass"
echo "done -> $OUT"
