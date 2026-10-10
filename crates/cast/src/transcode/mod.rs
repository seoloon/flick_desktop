//! Converting a title on this computer for an AirPlay receiver that cannot play it as it is.

mod args;
mod job;
mod locate;
mod plan;
mod receiver;
mod session;

pub(crate) use receiver::Converting;
pub(crate) use session::Session;
pub use args::{PLAYLIST, SEGMENT_SECS, args};
pub use job::Job;
pub use locate::{burnable, has_filter, locate};
pub use plan::{AudioPlan, Burn, Convert, NoVideo, Plan, VideoPlan, plan};

/// ffmpeg is not installed with Flick: raised before the local player is stopped.
pub fn missing_error() -> oneshot_core::Error {
    oneshot_core::Error::Playback(oneshot_core::codes::CAST_FFMPEG_MISSING.tag("ffmpeg, which converts this video for AirPlay, was not found. Reinstall Flick."))
}
