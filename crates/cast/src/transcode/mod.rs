//! Converting a title on this computer for an AirPlay receiver that cannot play it as it is.

mod args;
mod plan;

pub use args::{PLAYLIST, SEGMENT_SECS, args};
pub use plan::{AudioPlan, Burn, Convert, NoVideo, Plan, VideoPlan, plan};
