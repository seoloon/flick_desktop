//! Finding receivers with mDNS (Bonjour): `_googlecast._tcp` and `_airplay._tcp`.

use std::collections::HashMap;
use std::sync::{Arc, Once};

use mdns_sd::{ServiceDaemon, ServiceEvent};
use parking_lot::Mutex;

use crate::{CastDevice, CastKind};

const CHROMECAST: &str = "_googlecast._tcp.local.";
const AIRPLAY: &str = "_airplay._tcp.local.";

/// Receivers by device id, with the mDNS instance name they were announced
/// under (what a "gone" event names).
type Found = HashMap<String, (String, CastDevice)>;

pub struct Discovery {
    devices: Arc<Mutex<Found>>,
    started: Once,
}

impl Default for Discovery {
    fn default() -> Self {
        Self { devices: Arc::default(), started: Once::new() }
    }
}

impl std::fmt::Debug for Discovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Discovery").field("devices", &self.devices.lock().len()).finish_non_exhaustive()
    }
}

impl Discovery {
    /// Sorted by name. The first call starts the browsing.
    pub fn devices(&self) -> Vec<CastDevice> {
        self.started.call_once(|| self.start());
        let mut all: Vec<CastDevice> = self.devices.lock().values().map(|(_, d)| d.clone()).collect();
        all.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
        all
    }

    pub fn find(&self, id: &str) -> Option<CastDevice> {
        self.devices.lock().get(id).map(|(_, d)| d.clone())
    }

    fn start(&self) {
        let daemon = match ServiceDaemon::new() {
            Ok(d) => d,
            Err(e) => return tracing::warn!(target: "cast", "mDNS unavailable: {e}"),
        };
        for (service, kind) in [(CHROMECAST, CastKind::Chromecast), (AIRPLAY, CastKind::AirPlay)] {
            let events = match daemon.browse(service) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(target: "cast", "browsing {service}: {e}");
                    continue;
                }
            };
            let devices = Arc::clone(&self.devices);
            // The daemon lives as long as this thread reads its events.
            let daemon = daemon.clone();
            std::thread::Builder::new()
                .name(format!("cast-discovery-{kind:?}"))
                .spawn(move || {
                    let _daemon = daemon;
                    while let Ok(event) = events.recv() {
                        match event {
                            ServiceEvent::ServiceResolved(info) => {
                                let Some(ip) = info.get_addresses_v4().into_iter().next() else { continue };
                                let prop = |k: &str| info.get_property_val_str(k).map(str::to_owned);
                                let Some(device) = describe(kind, info.get_fullname(), &prop, ip.to_string(), info.get_port()) else { continue };
                                tracing::debug!(target: "cast", name = %device.name, kind = ?device.kind, host = %device.host, "receiver found");
                                devices.lock().insert(device.id.clone(), (info.get_fullname().to_owned(), device));
                            }
                            ServiceEvent::ServiceRemoved(_, fullname) => devices.lock().retain(|_, (name, _)| *name != fullname),
                            _ => {}
                        }
                    }
                })
                .ok();
        }
    }
}

/// A receiver from its mDNS record, or `None` when it cannot show video
/// (speakers).
pub(crate) fn describe(kind: CastKind, fullname: &str, prop: &dyn Fn(&str) -> Option<String>, host: String, port: u16) -> Option<CastDevice> {
    match kind {
        CastKind::Chromecast => {
            let id = prop("id").unwrap_or_else(|| fullname.to_owned());
            let name = prop("fn").unwrap_or_else(|| instance_name(fullname));
            // Chromecast Audio has no screen.
            let model = prop("md");
            if model.as_deref().is_some_and(|m| m.contains("Audio")) {
                return None;
            }
            Some(CastDevice { id: format!("cc:{id}"), name, kind, model, host, port })
        }
        CastKind::AirPlay => {
            // Speakers set none of the video bits: 0 (video), 3 (FairPlay video), 4 (video volume
            // control), 5 (HLS). Apple devices set bit 0; a Samsung TV only sets bit 4.
            const VIDEO: u64 = 0b11_1001;
            let features = prop("features").or_else(|| prop("ft"));
            let low = features.as_deref().and_then(|f| f.split(',').next()).and_then(|f| u64::from_str_radix(f.trim().trim_start_matches("0x"), 16).ok());
            if low.is_some_and(|low| low & VIDEO == 0) {
                return None;
            }
            let id = prop("deviceid").unwrap_or_else(|| fullname.to_owned());
            Some(CastDevice { id: format!("ap:{id}"), name: instance_name(fullname), kind, model: prop("model"), host, port })
        }
    }
}

fn instance_name(fullname: &str) -> String {
    fullname.split("._").next().unwrap_or(fullname).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k| pairs.iter().find(|(key, _)| *key == k).map(|(_, v)| (*v).to_owned())
    }

    #[test]
    fn chromecast_takes_its_friendly_name() {
        let p = props(&[("id", "abc123"), ("fn", "Living room"), ("md", "Chromecast")]);
        let d = describe(CastKind::Chromecast, "Chromecast-abc123._googlecast._tcp.local.", &p, "192.168.1.20".into(), 8009).unwrap();
        assert_eq!((d.id.as_str(), d.name.as_str(), d.port), ("cc:abc123", "Living room", 8009));
    }

    #[test]
    fn chromecast_audio_is_left_out() {
        let p = props(&[("id", "x"), ("fn", "Kitchen"), ("md", "Chromecast Audio")]);
        assert!(describe(CastKind::Chromecast, "k._googlecast._tcp.local.", &p, "10.0.0.2".into(), 8009).is_none());
    }

    #[test]
    fn a_samsung_tv_that_does_not_set_the_video_bit_is_kept_and_a_homepod_is_not() {
        // Captured with `dns-sd -Z _airplay._tcp`: Samsung says "video" with bit 4, not bit 0.
        let tv = props(&[("deviceid", "64:1C:AE:9F:61:EC"), ("features", "0x7F8AD0,0x38BCB46"), ("model", "UNU7120"), ("manufacturer", "Samsung")]);
        let d = describe(CastKind::AirPlay, "Samsung 7 Series (40)._airplay._tcp.local.", &tv, "10.0.0.5".into(), 53121).unwrap();
        assert_eq!(d.name, "Samsung 7 Series (40)");
        let homepod = props(&[("deviceid", "EE:FF"), ("features", "0x445F8A00,0x1C340"), ("model", "AudioAccessory1,1")]);
        assert!(describe(CastKind::AirPlay, "HomePod._airplay._tcp.local.", &homepod, "10.0.0.6".into(), 7000).is_none());
    }

    #[test]
    fn airplay_speakers_are_left_out() {
        let speaker = props(&[("deviceid", "AA:BB"), ("features", "0x445F8A00,0x1C340")]);
        assert!(describe(CastKind::AirPlay, "HomePod._airplay._tcp.local.", &speaker, "10.0.0.3".into(), 7000).is_none());
        let tv = props(&[("deviceid", "CC:DD"), ("features", "0x5A7FFFF7,0x1E"), ("model", "AppleTV6,2")]);
        let d = describe(CastKind::AirPlay, "Salon._airplay._tcp.local.", &tv, "10.0.0.4".into(), 7000).unwrap();
        assert_eq!((d.id.as_str(), d.name.as_str()), ("ap:CC:DD", "Salon"));
    }
}
