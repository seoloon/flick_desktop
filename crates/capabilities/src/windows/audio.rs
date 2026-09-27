//! WASAPI endpoint probing.
//!
//! * Shared-mode mixer format → how many PCM channels the OS will accept
//!   (this follows the user's "Speaker setup" in the Sound control panel).
//! * Exclusive-mode `IsFormatSupported` with IEC 61937 sub-formats → which
//!   compressed bitstreams the driver will accept. For HDMI, drivers derive
//!   this from the sink's EDID (AVR/TV), so it is a real signal. This is the
//!   same negotiation mpv performs when opening the device; probing it up
//!   front lets us set `--audio-spdif` per device instead of failing at
//!   runtime (mpv does *not* fall back to PCM when a forced passthrough
//!   format is refused — verified, see docs/PLAYBACK_VALIDATION.md).

use oneshot_core::capabilities::{AudioCapabilities, AudioConnection, AudioDevice, PassthroughProbe};
use oneshot_core::stream::BitstreamFormat;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_EXCLUSIVE, DEVICE_STATE_ACTIVE, IAudioClient, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, PKEY_AudioEndpoint_FormFactor, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, WAVEFORMATEXTENSIBLE_0,
    eConsole, eRender,
};
use windows::Win32::Media::KernelStreaming::{
    KSDATAFORMAT_SUBTYPE_IEC61937_DOLBY_DIGITAL, KSDATAFORMAT_SUBTYPE_IEC61937_DOLBY_DIGITAL_PLUS,
    KSDATAFORMAT_SUBTYPE_IEC61937_DOLBY_MLP, KSDATAFORMAT_SUBTYPE_IEC61937_DTS, KSDATAFORMAT_SUBTYPE_IEC61937_DTS_HD,
    WAVE_FORMAT_EXTENSIBLE,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToUInt32;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, STGM_READ};
use windows::core::GUID;

const SPEAKER_STEREO: u32 = 0x3;
const SPEAKER_7POINT1_SURROUND: u32 = 0x63F;

pub fn probe(notes: &mut Vec<String>) -> AudioCapabilities {
    match probe_inner() {
        Ok(caps) => caps,
        Err(e) => {
            notes.push(format!("WASAPI endpoint enumeration failed: {e}"));
            AudioCapabilities::default()
        }
    }
}

fn probe_inner() -> windows::core::Result<AudioCapabilities> {
    // SAFETY: standard COM activation; COM is initialised by the caller.
    let enumerator: IMMDeviceEnumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    // SAFETY: valid enumerator.
    let collection = unsafe { enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)? };
    // SAFETY: valid collection.
    let count = unsafe { collection.GetCount()? };
    let mut devices = Vec::with_capacity(count as usize);
    for i in 0..count {
        // SAFETY: index < count.
        let Ok(device) = (unsafe { collection.Item(i) }) else { continue };
        match describe(&device) {
            Ok(d) => devices.push(d),
            Err(e) => tracing::warn!(target: "capabilities", "skipping audio endpoint {i}: {e}"),
        }
    }
    // SAFETY: valid enumerator; fails harmlessly when there is no default.
    let default_device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
        .ok()
        .and_then(|d| device_id(&d).ok());
    Ok(AudioCapabilities { devices, default_device })
}

fn device_id(device: &IMMDevice) -> windows::core::Result<String> {
    // SAFETY: GetId returns a CoTaskMem string that we free after copying.
    unsafe {
        let p = device.GetId()?;
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as _));
        Ok(s)
    }
}

fn describe(device: &IMMDevice) -> windows::core::Result<AudioDevice> {
    let id = device_id(device)?;
    // SAFETY: valid device; read-only property store.
    let store = unsafe { device.OpenPropertyStore(STGM_READ)? };
    // SAFETY: valid store and well-known keys.
    let name = unsafe { store.GetValue(&PKEY_Device_FriendlyName) }.map(|v| v.to_string()).unwrap_or_default();
    // SAFETY: as above.
    let form_factor = unsafe { store.GetValue(&PKEY_AudioEndpoint_FormFactor) }
        .ok()
        .and_then(|v| unsafe { PropVariantToUInt32(&v) }.ok());

    // SAFETY: activation of the standard audio client interface.
    let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None)? };
    let (channels, mask, rate) = mix_format(&client).unwrap_or((2, None, None));

    let connection = connection(form_factor, &name);
    let formats: Vec<BitstreamFormat> =
        BitstreamFormat::ALL.into_iter().filter(|f| supports_bitstream(&client, *f)).collect();
    let passthrough = PassthroughProbe::Probed { formats };

    Ok(AudioDevice {
        // mpv names WASAPI devices `wasapi/<last component of the endpoint id>`.
        mpv_name: id.rsplit('.').next().map(|guid| format!("wasapi/{guid}")),
        id,
        name,
        connection,
        channels,
        channel_layout: mask.map(layout_name),
        sample_rate: rate,
        passthrough,
    })
}

fn mix_format(client: &IAudioClient) -> Option<(u8, Option<u32>, Option<u32>)> {
    // SAFETY: GetMixFormat returns a CoTaskMem WAVEFORMATEX(EXTENSIBLE) that
    // we read (respecting its tag) and free.
    unsafe {
        let p = client.GetMixFormat().ok()?;
        let fmt = &*p;
        let channels = fmt.nChannels as u8;
        let rate = Some(fmt.nSamplesPerSec);
        let mask = (fmt.wFormatTag == WAVE_FORMAT_EXTENSIBLE as u16 && fmt.cbSize >= 22)
            .then(|| (*(p as *const WAVEFORMATEXTENSIBLE)).dwChannelMask);
        CoTaskMemFree(Some(p as _));
        Some((channels, mask, rate))
    }
}

/// IEC 61937 framing parameters per format (IEC 61937 / Microsoft
/// "Supporting Dolby/DTS over HDMI" guidelines).
fn iec61937(format: BitstreamFormat) -> (GUID, u16, u32, u32) {
    match format {
        BitstreamFormat::Ac3 => (KSDATAFORMAT_SUBTYPE_IEC61937_DOLBY_DIGITAL, 2, 48_000, SPEAKER_STEREO),
        BitstreamFormat::Dts => (KSDATAFORMAT_SUBTYPE_IEC61937_DTS, 2, 48_000, SPEAKER_STEREO),
        BitstreamFormat::Eac3 => (KSDATAFORMAT_SUBTYPE_IEC61937_DOLBY_DIGITAL_PLUS, 2, 192_000, SPEAKER_STEREO),
        BitstreamFormat::DtsHd => (KSDATAFORMAT_SUBTYPE_IEC61937_DTS_HD, 8, 192_000, SPEAKER_7POINT1_SURROUND),
        BitstreamFormat::TrueHd => (KSDATAFORMAT_SUBTYPE_IEC61937_DOLBY_MLP, 8, 192_000, SPEAKER_7POINT1_SURROUND),
    }
}

fn supports_bitstream(client: &IAudioClient, format: BitstreamFormat) -> bool {
    let (sub, channels, rate, mask) = iec61937(format);
    let block_align = channels * 2;
    let wfx = WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_EXTENSIBLE as u16,
            nChannels: channels,
            nSamplesPerSec: rate,
            nAvgBytesPerSec: rate * block_align as u32,
            nBlockAlign: block_align,
            wBitsPerSample: 16,
            cbSize: 22,
        },
        Samples: WAVEFORMATEXTENSIBLE_0 { wValidBitsPerSample: 16 },
        dwChannelMask: mask,
        SubFormat: sub,
    };
    // SAFETY: `wfx` is a complete WAVEFORMATEXTENSIBLE; exclusive mode takes
    // no closest-match out-param.
    let hr = unsafe { client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &wfx.Format, None) };
    hr.is_ok() && hr.0 == 0
}

/// `EndpointFormFactor` values from mmdeviceapi.h.
fn connection(form_factor: Option<u32>, name: &str) -> AudioConnection {
    let lower = name.to_ascii_lowercase();
    // Virtual endpoints (voice processors, streaming drivers) often claim a
    // digital form factor; classify them by name first.
    if ["virtual", "steam streaming", "broadcast", "vb-audio", "droidcam", "voicemeeter"].iter().any(|v| lower.contains(v)) {
        return AudioConnection::Virtual;
    }
    match form_factor {
        Some(8) => AudioConnection::Spdif,
        Some(9) if lower.contains("displayport") => AudioConnection::DisplayPort,
        Some(9) => AudioConnection::Hdmi,
        Some(0) => AudioConnection::Virtual,
        _ if lower.contains("bluetooth") || lower.contains("hands-free") => AudioConnection::Bluetooth,
        _ if lower.contains("usb") => AudioConnection::Usb,
        Some(1 | 2 | 3 | 5) => AudioConnection::Analog,
        _ => AudioConnection::Unknown,
    }
}

fn layout_name(mask: u32) -> String {
    match mask {
        0x4 => "mono".into(),
        0x3 => "stereo".into(),
        0xB => "2.1".into(),
        0x33 => "quad".into(),
        0x3F => "5.1".into(),
        0x60F => "5.1(side)".into(),
        0x63F => "7.1".into(),
        0x2D63F => "7.1.4".into(),
        other => format!("mask 0x{other:X}"),
    }
}
