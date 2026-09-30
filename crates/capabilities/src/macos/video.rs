//! Hardware decoder enumeration through VideoToolbox.
//!
//! mpv's `videotoolbox` hwdec (picked by `hwdec=auto-safe`) decodes exactly
//! what `VTIsHardwareDecodeSupported` reports, so this predicts when mpv falls
//! back to the CPU. The call answers per codec, not per profile: HEVC Main10
//! is reported on Apple silicon only, where every generation decodes it, and
//! left out on Intel, where it depends on the GPU generation.

use oneshot_core::capabilities::{HardwareDecoder, VideoCapabilities};
use oneshot_core::stream::VideoCodec;

#[link(name = "VideoToolbox", kind = "framework")]
unsafe extern "C" {
    fn VTIsHardwareDecodeSupported(codec_type: u32) -> u8;
}

/// `CMVideoCodecType` four-character codes.
const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

struct Known {
    fourcc: u32,
    codec: VideoCodec,
    profile: &'static str,
    bit_depth: u8,
}

fn known_codecs() -> Vec<Known> {
    let k = |code: &[u8; 4], codec, profile, bit_depth| Known { fourcc: fourcc(code), codec, profile, bit_depth };
    vec![
        k(b"avc1", VideoCodec::H264, "H.264 High", 8),
        k(b"hvc1", VideoCodec::Hevc, "HEVC Main", 8),
        k(b"av01", VideoCodec::Av1, "AV1 Main", 10),
        k(b"vp09", VideoCodec::Vp9, "VP9 Profile 0", 8),
    ]
}

pub fn probe(notes: &mut Vec<String>) -> VideoCapabilities {
    let mut decoders: Vec<HardwareDecoder> = Vec::new();
    for k in known_codecs() {
        // SAFETY: pure query taking a codec four-character code.
        if unsafe { VTIsHardwareDecodeSupported(k.fourcc) } == 0 {
            continue;
        }
        let mut add = |profile: &str, bit_depth: u8| {
            decoders.push(HardwareDecoder {
                codec: k.codec.clone(),
                profile: profile.to_owned(),
                max_bit_depth: bit_depth,
                api: "videotoolbox".into(),
                max_width: None,
                max_height: None,
            });
        };
        add(k.profile, k.bit_depth);
        if k.codec == VideoCodec::Hevc && cfg!(target_arch = "aarch64") {
            add("HEVC Main10", 10);
        }
    }
    notes.push("Hardware decoders probed through VideoToolbox (per codec; HEVC Main10 assumed on Apple silicon).".into());
    decoders.sort_by(|a, b| a.profile.cmp(&b.profile));
    VideoCapabilities { hardware_decoders: decoders, hardware_probe_ok: true, software_codecs: Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fourcc_is_big_endian_ascii() {
        assert_eq!(fourcc(b"avc1"), 0x6176_6331);
    }

    #[test]
    fn every_mac_decodes_h264_in_hardware() {
        let caps = probe(&mut Vec::new());
        assert!(caps.hw_decoder(&VideoCodec::H264, 8).is_some(), "{:?}", caps.hardware_decoders);
    }
}
