//! Hardware decoder enumeration through `ID3D11VideoDevice`.
//!
//! mpv's `d3d11va` hwdec uses exactly these decoder profiles, so this is an
//! accurate prediction of what mpv will hardware-decode on this GPU. When a
//! profile is missing mpv silently falls back to software decoding; the
//! decision engine uses this list to warn (or prefer a server transcode by
//! policy) when e.g. 4K HEVC would have to be decoded on the CPU.

use oneshot_core::capabilities::{HardwareDecoder, VideoCapabilities};
use oneshot_core::stream::VideoCodec;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_DECODER_PROFILE_AV1_VLD_PROFILE0, D3D11_DECODER_PROFILE_H264_VLD_NOFGT,
    D3D11_DECODER_PROFILE_HEVC_VLD_MAIN, D3D11_DECODER_PROFILE_HEVC_VLD_MAIN10, D3D11_DECODER_PROFILE_MPEG2_VLD,
    D3D11_DECODER_PROFILE_VC1_D2010, D3D11_DECODER_PROFILE_VP8_VLD, D3D11_DECODER_PROFILE_VP9_VLD_10BIT_PROFILE2,
    D3D11_DECODER_PROFILE_VP9_VLD_PROFILE0, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device, ID3D11VideoDevice,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_NV12, DXGI_FORMAT_P010};
use windows::core::{GUID, Interface};

use super::{hardware_adapters, wide_to_string};

struct Known {
    guid: GUID,
    codec: VideoCodec,
    profile: &'static str,
    bit_depth: u8,
    format: DXGI_FORMAT,
}

fn known_profiles() -> Vec<Known> {
    let k = |guid, codec, profile, bit_depth, format| Known { guid, codec, profile, bit_depth, format };
    vec![
        k(D3D11_DECODER_PROFILE_H264_VLD_NOFGT, VideoCodec::H264, "H.264 High", 8, DXGI_FORMAT_NV12),
        k(D3D11_DECODER_PROFILE_HEVC_VLD_MAIN, VideoCodec::Hevc, "HEVC Main", 8, DXGI_FORMAT_NV12),
        k(D3D11_DECODER_PROFILE_HEVC_VLD_MAIN10, VideoCodec::Hevc, "HEVC Main10", 10, DXGI_FORMAT_P010),
        k(D3D11_DECODER_PROFILE_VP9_VLD_PROFILE0, VideoCodec::Vp9, "VP9 Profile 0", 8, DXGI_FORMAT_NV12),
        k(D3D11_DECODER_PROFILE_VP9_VLD_10BIT_PROFILE2, VideoCodec::Vp9, "VP9 Profile 2 (10-bit)", 10, DXGI_FORMAT_P010),
        k(D3D11_DECODER_PROFILE_AV1_VLD_PROFILE0, VideoCodec::Av1, "AV1 Main", 10, DXGI_FORMAT_P010),
        k(D3D11_DECODER_PROFILE_VP8_VLD, VideoCodec::Vp8, "VP8", 8, DXGI_FORMAT_NV12),
        k(D3D11_DECODER_PROFILE_MPEG2_VLD, VideoCodec::Mpeg2, "MPEG-2", 8, DXGI_FORMAT_NV12),
        k(D3D11_DECODER_PROFILE_VC1_D2010, VideoCodec::Vc1, "VC-1", 8, DXGI_FORMAT_NV12),
    ]
}

pub fn probe(notes: &mut Vec<String>) -> VideoCapabilities {
    let known = known_profiles();
    let mut decoders: Vec<HardwareDecoder> = Vec::new();
    let mut ok = false;
    // mpv's d3d11 context (and thus d3d11va) uses the default adapter unless
    // `--d3d11-adapter` is set, so only that adapter's decoders are relevant.
    for adapter in hardware_adapters().into_iter().take(1) {
        // SAFETY: valid adapter.
        let name = unsafe { adapter.GetDesc1() }.map(|d| wide_to_string(&d.Description)).unwrap_or_default();
        let mut device: Option<ID3D11Device> = None;
        // SAFETY: creating a D3D11 device on an explicit adapter (driver type
        // must then be UNKNOWN), with video support for the video interfaces.
        let created = unsafe {
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )
        };
        let Some(video) = created.ok().and(device).and_then(|d| d.cast::<ID3D11VideoDevice>().ok()) else {
            notes.push(format!("{name}: no D3D11 video device; hardware decoding unavailable on this adapter"));
            continue;
        };
        ok = true;
        notes.push(format!("Hardware decoders probed on default adapter: {name}"));
        // SAFETY: valid video device.
        let count = unsafe { video.GetVideoDecoderProfileCount() };
        for i in 0..count {
            // SAFETY: i < count.
            let Ok(guid) = (unsafe { video.GetVideoDecoderProfile(i) }) else { continue };
            let Some(k) = known.iter().find(|k| k.guid == guid) else { continue };
            // SAFETY: valid profile GUID and DXGI format.
            let format_ok = unsafe { video.CheckVideoDecoderFormat(&guid, k.format) }.is_ok_and(|b| b.as_bool());
            if !format_ok || decoders.iter().any(|d| d.profile == k.profile) {
                continue;
            }
            decoders.push(HardwareDecoder {
                codec: k.codec.clone(),
                profile: k.profile.to_owned(),
                max_bit_depth: k.bit_depth,
                api: "d3d11va".into(),
                max_width: None,
                max_height: None,
            });
        }
    }
    decoders.sort_by(|a, b| a.profile.cmp(&b.profile));
    VideoCapabilities { hardware_decoders: decoders, hardware_probe_ok: ok, software_codecs: Vec::new() }
}
