//! CoreAudio output device probing.
//!
//! Lists the devices that have output streams, with the connection type, the
//! channel count of the output stream format and the nominal sample rate.
//! Passthrough is deliberately not probed: CoreAudio has no HBR path (no
//! TrueHD / DTS-HD / Atmos bitstream, see ARCHITECTURE.md §6) and whether mpv's
//! S/PDIF output for AC3 / E-AC3 / DTS reaches a given sink has not been
//! validated, so every device says `NotProbed` rather than promise it.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{NonNull, null};

use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectPropertyAddress, kAudioDevicePropertyDeviceUID,
    kAudioDevicePropertyNominalSampleRate, kAudioDevicePropertyStreamConfiguration, kAudioDevicePropertyTransportType,
    kAudioDeviceTransportTypeAggregate, kAudioDeviceTransportTypeBluetooth, kAudioDeviceTransportTypeBluetoothLE,
    kAudioDeviceTransportTypeBuiltIn, kAudioDeviceTransportTypeDisplayPort, kAudioDeviceTransportTypeHDMI,
    kAudioDeviceTransportTypeUSB, kAudioDeviceTransportTypeVirtual, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain, kAudioObjectPropertyName,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
};
use objc2_core_audio_types::AudioBufferList;
use objc2_core_foundation::{CFRetained, CFString};
use oneshot_core::capabilities::{AudioCapabilities, AudioConnection, AudioDevice, PassthroughProbe};

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
        passthrough: PassthroughProbe::NotProbed { reason: "compressed passthrough is not validated on macOS".into() },
    })
}

pub fn probe(notes: &mut Vec<String>) -> AudioCapabilities {
    let Some(buf) = get_bytes(SYSTEM, address(kAudioHardwarePropertyDevices, kAudioObjectPropertyScopeGlobal)) else {
        notes.push("CoreAudio device enumeration failed; audio outputs reported as unknown.".into());
        return AudioCapabilities::default();
    };
    let count = get_len(SYSTEM);
    // SAFETY: the property is an array of `AudioObjectID` (u32) of `count` entries inside `buf`.
    let ids: &[Id] = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<Id>(), count) };
    let devices: Vec<AudioDevice> = ids.iter().filter_map(|&d| describe(d)).collect();
    let default_id = get::<Id>(SYSTEM, address(kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyScopeGlobal));
    let default_device = default_id.and_then(|d| get_string(d, address(kAudioDevicePropertyDeviceUID, kAudioObjectPropertyScopeGlobal)));
    notes.push("Audio outputs probed through CoreAudio; compressed passthrough is not probed on macOS.".into());
    AudioCapabilities { devices, default_device }
}

/// Number of device ids the system object lists.
fn get_len(object: Id) -> usize {
    let mut addr = address(kAudioHardwarePropertyDevices, kAudioObjectPropertyScopeGlobal);
    let mut size = 0u32;
    // SAFETY: valid address and size pointers.
    let status = unsafe { AudioObjectGetPropertyDataSize(object, NonNull::from(&mut addr), 0, null(), NonNull::from(&mut size)) };
    if status == 0 { size as usize / size_of::<Id>() } else { 0 }
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
    fn transport_types_map_to_connections() {
        assert_eq!(connection(kAudioDeviceTransportTypeHDMI), AudioConnection::Hdmi);
        assert_eq!(connection(kAudioDeviceTransportTypeBuiltIn), AudioConnection::Builtin);
        assert_eq!(connection(0), AudioConnection::Unknown);
    }
}
