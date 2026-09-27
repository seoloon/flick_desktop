//! Playback probe: plays each file for a few seconds in a real mpv window and
//! prints what mpv *actually* did (decoder, hwdec, colourspace in/out, audio
//! format in/out, tracks). Used by docs/PLAYBACK_VALIDATION.md.
//!
//!   cargo run -p oneshot-mpv --example probe -- [--spdif ac3,eac3,dts] [--secs N] FILE...

use std::path::PathBuf;
use std::time::{Duration, Instant};

use oneshot_mpv::{Event, Node};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    let mut spdif = String::new();
    let mut secs = 3.0;
    let mut extra: Vec<(String, String)> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--spdif" => spdif = args.next().unwrap_or_default(),
            "--secs" => secs = args.next().and_then(|s| s.parse().ok()).unwrap_or(secs),
            "--opt" => {
                let kv = args.next().unwrap_or_default();
                let (k, v) = kv.split_once('=').unwrap_or((&kv, "yes"));
                extra.push((k.to_owned(), v.to_owned()));
            }
            _ => files.push(a),
        }
    }
    let dirs = [PathBuf::from("third_party/mpv/windows-x64")];
    let api = oneshot_mpv::load(std::env::var_os("ONESHOT_LIBMPV").map(Into::into), &dirs)?;
    let mut opts = vec![
        ("vo", "gpu-next"),
        ("hwdec", "auto-safe"),
        ("force-window", "yes"),
        ("idle", "yes"),
        ("keep-open", "no"),
        ("osc", "no"),
        ("input-default-bindings", "no"),
        ("terminal", "no"),
        ("volume", "15"),
    ];
    if !spdif.is_empty() {
        opts.push(("audio-spdif", spdif.as_str()));
    }
    opts.extend(extra.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let (mpv, mut events) = oneshot_mpv::Mpv::create(api, opts)?;
    mpv.request_log_messages("warn")?;

    if files.is_empty() {
        let list = mpv.get_property("audio-device-list")?;
        for d in list.as_array().unwrap_or_default() {
            println!("{:<70} {}", d.get("name").and_then(Node::as_str).unwrap_or(""), d.get("description").and_then(Node::as_str).unwrap_or(""));
        }
    }
    for file in &files {
        println!("\n=== {file}");
        mpv.command(&["loadfile", file, "replace"])?;
        let start = Instant::now();
        let (mut started, mut restarted) = (false, false);
        while start.elapsed() < Duration::from_secs_f64(secs + 8.0) {
            match events.wait(0.1) {
                Some(Event::StartFile) => started = true,
                Some(Event::PlaybackRestart) => restarted = true,
                Some(Event::EndFile { reason, error }) if started => {
                    println!("  end-file: {reason:?} {error:?}");
                    break;
                }
                Some(Event::Log { prefix, level, text }) => print!("  [{level}] {prefix}: {text}"),
                _ => {}
            }
            if restarted && start.elapsed() > Duration::from_secs_f64(secs) {
                break;
            }
        }
        report(&mpv);
    }
    mpv.command(&["quit"])?;
    Ok(())
}

fn report(mpv: &oneshot_mpv::Mpv) {
    let get = |p: &str| mpv.try_get_property(p).ok().flatten().unwrap_or(Node::None);
    let s = |n: &Node, k: &str| (if k.is_empty() { Some(n) } else { n.path(k) }).filter(|v| **v != Node::None).map(|v| match v {
        Node::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }).unwrap_or_else(|| "-".into());
    let vp = get("video-params");
    let vtp = get("video-target-params");
    let ap = get("audio-params");
    let aop = get("audio-out-params");
    println!("  vo={} ctx={} ao={}", s(&get("current-vo"), ""), s(&get("current-gpu-context"), ""), s(&get("current-ao"), ""));
    println!("  video codec={} hwdec={} {}x{} {} fps={}",
        s(&get("video-codec"), ""), s(&get("hwdec-current"), ""), s(&vp, "w"), s(&vp, "h"), s(&vp, "hw-pixelformat"),
        s(&get("container-fps"), ""));
    println!("  video in : pix={} prim={} gamma={} max-cll={} sig-peak={}", s(&vp, "pixelformat"), s(&vp, "primaries"), s(&vp, "gamma"), s(&vp, "max-cll"), s(&vp, "sig-peak"));
    println!("  video out: prim={} gamma={} max-luma={}", s(&vtp, "primaries"), s(&vtp, "gamma"), s(&vtp, "max-luma"));
    println!("  audio codec={} in: fmt={} ch={} ({}) rate={}", s(&get("audio-codec-name"), ""), s(&ap, "format"), s(&ap, "channel-count"), s(&ap, "hr-channels"), s(&ap, "samplerate"));
    println!("  audio out: fmt={} ch={} ({}) rate={}", s(&aop, "format"), s(&aop, "channel-count"), s(&aop, "hr-channels"), s(&aop, "samplerate"));
    if let Some(tracks) = get("track-list").as_array() {
        for t in tracks {
            println!("  track {}#{} {} lang={} title={} sel={} forced={}", s(t, "type"), s(t, "id"), s(t, "codec"), s(t, "lang"), s(t, "title"), s(t, "selected"), s(t, "forced"));
        }
    }
    println!("  dropped-frames={} decoder-drops={} avsync={}", s(&get("frame-drop-count"), ""), s(&get("decoder-frame-drop-count"), ""), s(&get("avsync"), ""));
}
