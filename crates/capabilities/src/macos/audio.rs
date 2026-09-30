//! CoreAudio output device probing.
//!
//! Lists the devices that have output streams, with the connection type, the
//! channel count of the output stream format and the nominal sample rate.
//! Passthrough follows what mpv's exclusive CoreAudio output does: it needs a
//! stream whose *physical* formats include a compressed one (`ac-3` or the
//! IEC 60958 `cac3`), which macOS only lists when the sink (AVR, TV over HDMI,
//! optical DAC) reported it. That gives AC3 and DTS core; E-AC3 additionally
//! needs a 192 kHz (4x) format. CoreAudio has no HBR path, so TrueHD, DTS-HD and
//! Atmos never bitstream (ARCHITECTURE.md §6). Acceptance is necessary, not
//! sufficient: the player still detects a refusal at runtime.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{NonNull, null};

use objc2_core_audio::{
    AudioStreamRangedDescription, kAudioDevicePropertyStreams, kAudioStreamPropertyAvailablePhysicalFormats,
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectPropertyAddress, kAudioDevicePropertyDeviceUID,
    kAudioDevicePropertyNominalSampleRate, kAudioDevicePropertyStreamConfiguration, kAudioDevicePropertyTransportType,
    kAudioDeviceTransportTypeAggregate, kAudioDeviceTransportTypeBluetooth, kAudioDeviceTransportTypeBluetoothLE,
    kAudioDeviceTransportTypeBuiltIn, kAudioDeviceTransportTypeDisplayPort, kAudioDeviceTransportTypeHDMI,
    kAudioDeviceTransportTypeUSB, kAudioDeviceTransportTypeVirtual, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain, kAudioObjectPropertyName,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
};
use objc2_core_audio_types::{AudioBufferList, kAudioFormat60958AC3, kAudioFormatAC3};
use objc2_core_foundation::{CFRetained, CFString};
use oneshot_core::capabilities::{AudioCapabilities, AudioConnection, AudioDevice, PassthroughProbe};
use oneshot_core::stream::BitstreamFormat;

type Id = u32;

const SYSTEM: Id = kAudioObjectSystemObject as Id;

fn address(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress { mSelector: selector, mScope: scope, mElement: kAudioObjectPropertyElementMain }
}

/// A fixed-size property (`u32`, `f64`, an object id…), `None` when the object lacks it.
fn get<T: Default + Copy>(object: Id, mut addr: AudioObjectPropertyAddress) -> Option<T> {
    let mut value = T::default();
    let mut size = size_of::<T>() as u32;
    // SAFETY: `value` is a live `T` of `size` bytes and the address is a valid pointer.
    let status = unsafe {
        AudioObjectGetPropertyData(object, NonNull::from(&mut addr), 0, null(), NonNull::from(&mut size), NonNull::from(&mut value).cast())
    };
    (status == 0).then_some(value)
}

/// A variable-size property as raw bytes, 8-aligned so it can hold an `AudioBufferList`.
fn get_bytes(object: Id, mut addr: AudioObjectPropertyAddress) -> Option<Vec<u64>> {
    let mut size = 0u32;
    // SAFETY: valid address and size pointers.
    let status = unsafe { AudioObjectGetPropertyDataSize(object, NonNull::from(&mut addr), 0, null(), NonNull::from(&mut size)) };
    if status != 0 || size == 0 {
        return None;
    }
    let mut buf = vec![0u64; (size as usize).div_ceil(8)];
    // SAFETY: `buf` has at least `size` writable bytes.
    let status = unsafe {
        AudioObjectGetPropertyData(object, NonNull::from(&mut addr), 0, null(), NonNull::from(&mut size), NonNull::new(buf.as_mut_ptr().cast::<c_void>())?)
    };
    (status == 0).then_some(buf)
}

fn get_string(object: Id, addr: AudioObjectPropertyAddress) -> Option<String> {
    let raw: usize = get::<usize>(object, addr)?;
    // SAFETY: CoreAudio hands over a +1 retained CFString for these properties.
    let s = unsafe { CFRetained::from_raw(NonNull::new(raw as *mut CFString)?) };
    Some(s.to_string())
}

fn output_channels(device: Id) -> u32 {
    let Some(buf) = get_bytes(device, address(kAudioDevicePropertyStreamConfiguration, kAudioObjectPropertyScopeOutput)) else {
        return 0;
    };
    let list = buf.as_ptr().cast::<AudioBufferList>();
    // SAFETY: the property is an `AudioBufferList` whose `mNumberBuffers` entries follow
    // each other from `mBuffers`, all inside `buf`.
    unsafe {
        let n = (*list).mNumberBuffers as usize;
        let first = (*list).mBuffers.as_ptr();
        (0..n).map(|i| (*first.add(i)).mNumberChannels).sum()
    }
}

/// Highest sample rate of each compressed (digital) physical format a stream offers.
fn digital_rates(stream: Id) -> Vec<f64> {
    let Some(buf) = get_bytes(stream, address(kAudioStreamPropertyAvailablePhysicalFormats, kAudioObjectPropertyScopeGlobal)) else {
        return Vec::new();
    };
    let size = property_size(stream, address(kAudioStreamPropertyAvailablePhysicalFormats, kAudioObjectPropertyScopeGlobal)).unwrap_or(0);
    let count = size as usize / size_of::<AudioStreamRangedDescription>();
    // SAFETY: the property is an array of `count` `AudioStreamRangedDescription` inside `buf`
    // (a `u64` buffer, so suitably aligned for the 8-byte fields).
    let formats = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<AudioStreamRangedDescription>(), count) };
    formats
        .iter()
        .filter(|f| f.mFormat.mFormatID == kAudioFormatAC3 || f.mFormat.mFormatID == kAudioFormat60958AC3)
        .map(|f| f.mSampleRateRange.mMaximum.max(f.mFormat.mSampleRate))
        .collect()
}

/// Formats mpv can bitstream, from the sample rates of a device's compressed physical formats.
fn formats_from_rates(rates: &[f64]) -> Vec<BitstreamFormat> {
    if rates.is_empty() {
        return Vec::new();
    }
    let mut formats = vec![BitstreamFormat::Ac3, BitstreamFormat::Dts];
    // E-AC3 rides IEC 61937 at 4x the base rate.
    if rates.iter().any(|r| *r >= 192_000.0) {
        formats.push(BitstreamFormat::Eac3);
    }
    formats
}

fn passthrough(device: Id) -> PassthroughProbe {
    let streams = get_bytes(device, address(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput));
    let count = property_size(device, address(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput)).unwrap_or(0) as usize / size_of::<Id>();
    let Some(streams) = streams else {
        return PassthroughProbe::NotProbed { reason: "the device lists no output stream".into() };
    };
    // SAFETY: the property is an array of `count` stream ids inside `streams`.
    let ids: &[Id] = unsafe { std::slice::from_raw_parts(streams.as_ptr().cast::<Id>(), count) };
    let rates: Vec<f64> = ids.iter().flat_map(|&s| digital_rates(s)).collect();
    PassthroughProbe::Probed { formats: formats_from_rates(&rates) }
}

// CoreAudio's constants keep their C names.
#[allow(non_upper_case_globals)]
fn connection(transport: u32) -> AudioConnection {
    match transport {
        kAudioDeviceTransportTypeHDMI => AudioConnection::Hdmi,
        kAudioDeviceTransportTypeDisplayPort => AudioConnection::DisplayPort,
        kAudioDeviceTransportTypeUSB => AudioConnection::Usb,
        kAudioDeviceTransportTypeBluetooth | kAudioDeviceTransportTypeBluetoothLE => AudioConnection::Bluetooth,
        kAudioDeviceTransportTypeBuiltIn => AudioConnection::Builtin,
        kAudioDeviceTransportTypeVirtual | kAudioDeviceTransportTypeAggregate => AudioConnection::Virtual,
        _ => AudioConnection::Unknown,
    }
}

fn describe(device: Id) -> Option<AudioDevice> {
    let channels = output_channels(device);
    if channels == 0 {
        return None;
    }
    let uid = get_string(device, address(kAudioDevicePropertyDeviceUID, kAudioObjectPropertyScopeGlobal))?;
    let name = get_string(device, address(kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal)).unwrap_or_else(|| uid.clone());
    let transport = get::<u32>(device, address(kAudioDevicePropertyTransportType, kAudioObjectPropertyScopeGlobal)).unwrap_or(0);
    let rate = get::<f64>(device, address(kAudioDevicePropertyNominalSampleRate, kAudioObjectPropertyScopeGlobal));
    Some(AudioDevice {
        mpv_name: Some(format!("coreaudio/{uid}")),
        id: uid,
        name,
        connection: connection(transport),
        channels: channels.min(u32::from(u8::MAX)) as u8,
        channel_layout: None,
        sample_rate: rate.filter(|r| *r > 0.0).map(|r| r.round() as u32),
        passthrough: passthrough(device),
    })
}

pub fn probe(notes: &mut Vec<String>) -> AudioCapabilities {
    let Some(buf) = get_bytes(SYSTEM, address(kAudioHardwarePropertyDevices, kAudioObjectPropertyScopeGlobal)) else {
        notes.push("CoreAudio device enumeration failed; audio outputs reported as unknown.".into());
        return AudioCapabilities::default();
    };
    let count = property_size(SYSTEM, address(kAudioHardwarePropertyDevices, kAudioObjectPropertyScopeGlobal)).unwrap_or(0) as usize / size_of::<Id>();
    // SAFETY: the property is an array of `AudioObjectID` (u32) of `count` entries inside `buf`.
    let ids: &[Id] = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<Id>(), count) };
    let devices: Vec<AudioDevice> = ids.iter().filter_map(|&d| describe(d)).collect();
    let default_id = get::<Id>(SYSTEM, address(kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyScopeGlobal));
    let default_device = default_id.and_then(|d| get_string(d, address(kAudioDevicePropertyDeviceUID, kAudioObjectPropertyScopeGlobal)));
    notes.push("Audio outputs and their compressed formats probed through CoreAudio (no HBR: TrueHD and DTS-HD never bitstream).".into());
    AudioCapabilities { devices, default_device }
}

fn property_size(object: Id, mut addr: AudioObjectPropertyAddress) -> Option<u32> {
    let mut size = 0u32;
    // SAFETY: valid address and size pointers.
    let status = unsafe { AudioObjectGetPropertyDataSize(object, NonNull::from(&mut addr), 0, null(), NonNull::from(&mut size)) };
    (status == 0).then_some(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_output_is_one_of_the_listed_devices() {
        let caps = probe(&mut Vec::new());
        if let Some(default) = &caps.default_device {
            assert!(caps.device(default).is_some(), "{default} missing from {:?}", caps.devices);
        }
        for d in &caps.devices {
            assert!(d.channels > 0 && d.mpv_name.as_deref().is_some_and(|n| n.starts_with("coreaudio/")), "{d:?}");
        }
    }

    #[test]
    fn no_digital_format_means_no_bitstream() {
        assert!(formats_from_rates(&[]).is_empty());
    }

    #[test]
    fn a_digital_format_gives_ac3_and_dts() {
        assert_eq!(formats_from_rates(&[48_000.0, 96_000.0]), vec![BitstreamFormat::Ac3, BitstreamFormat::Dts]);
    }

    #[test]
    fn a_192khz_digital_format_adds_eac3_but_never_hbr_codecs() {
        let formats = formats_from_rates(&[48_000.0, 192_000.0]);
        assert!(formats.contains(&BitstreamFormat::Eac3));
        assert!(!formats.contains(&BitstreamFormat::TrueHd) && !formats.contains(&BitstreamFormat::DtsHd));
    }

    #[test]
    fn transport_types_map_to_connections() {
        assert_eq!(connection(kAudioDeviceTransportTypeHDMI), AudioConnection::Hdmi);
        assert_eq!(connection(kAudioDeviceTransportTypeBuiltIn), AudioConnection::Builtin);
        assert_eq!(connection(0), AudioConnection::Unknown);
    }
}
