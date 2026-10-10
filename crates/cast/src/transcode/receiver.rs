//! The AirPlay receiver as seen by the rest of Flick while a conversion feeds it.

use async_trait::async_trait;
use oneshot_core::Result;

use super::Session;
use crate::airplay::AirPlay;
use crate::{CastState, Receiver, Remote};

/// Seeks this close to the end of what is produced restart the conversion instead: the receiver
/// would stall at the edge.
const EDGE_MS: u64 = 12_000;
/// Less than this is produced: do not trust it for a scrub.
const MIN_MS: u64 = 12_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SeekPlan {
    /// A scrub on the receiver, at this position of its own (shifted) timeline.
    Within(u64),
    /// Convert again from the asked position and reload the stream.
    Restart,
}

/// The playlist covers `base..base + produced` of the title.
pub(crate) fn seek_plan(base: u64, produced: u64, ms: u64) -> SeekPlan {
    if produced >= MIN_MS && ms >= base && ms + EDGE_MS <= base + produced { SeekPlan::Within(ms - base) } else { SeekPlan::Restart }
}

/// A position of the receiver's timeline as a position in the title.
pub(crate) fn title_position(base: u64, receiver: u64, total: Option<u64>) -> u64 {
    let ms = base + receiver;
    total.map_or(ms, |t| ms.min(t))
}

pub(crate) struct Converting {
    airplay: AirPlay,
    session: tokio::sync::Mutex<Session>,
    total_ms: Option<u64>,
}

impl Converting {
    pub fn new(airplay: AirPlay, session: Session, total_ms: Option<u64>) -> Self {
        Self { airplay, session: tokio::sync::Mutex::new(session), total_ms }
    }
}

#[async_trait]
impl Receiver for Converting {
    async fn pause(&self) -> Result<()> {
        self.airplay.pause().await
    }

    async fn resume(&self) -> Result<()> {
        self.airplay.resume().await
    }

    async fn seek(&self, ms: u64) -> Result<()> {
        let mut session = self.session.lock().await;
        match seek_plan(session.base_ms(), session.produced_ms(), ms) {
            SeekPlan::Within(at) => self.airplay.seek(at).await,
            SeekPlan::Restart => {
                session.restart_at(ms).await?;
                self.airplay.load(&session.playlist_url(), 0, None).await
            }
        }
    }

    async fn set_volume(&self, volume: f32) -> Result<()> {
        self.airplay.set_volume(volume).await
    }

    async fn status(&self) -> Remote {
        let mut r = self.airplay.status().await;
        let mut session = self.session.lock().await;
        r.position_ms = title_position(session.base_ms(), r.position_ms, self.total_ms);
        r.duration_ms = self.total_ms.or(r.duration_ms.map(|d| d + session.base_ms()));
        // The receiver reaching the edge of a playlist still being written is not the end of the title.
        if r.state == CastState::Ended && !session.finished() {
            r.state = CastState::Buffering;
        }
        r
    }

    async fn stop(&self) {
        self.airplay.stop().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seek_inside_the_produced_part_is_a_plain_scrub_relative_to_the_start() {
        // The job began at 60 s and has 100 s ready: 60..160 s of the title.
        assert_eq!(seek_plan(60_000, 100_000, 90_000), SeekPlan::Within(30_000));
        assert_eq!(seek_plan(60_000, 100_000, 60_000), SeekPlan::Within(0));
    }

    #[test]
    fn a_seek_before_the_start_or_near_or_past_the_edge_restarts_the_job() {
        assert_eq!(seek_plan(60_000, 100_000, 10_000), SeekPlan::Restart);
        assert_eq!(seek_plan(60_000, 100_000, 155_000), SeekPlan::Restart); // within 12 s of the edge
        assert_eq!(seek_plan(60_000, 100_000, 900_000), SeekPlan::Restart);
        assert_eq!(seek_plan(0, 5_000, 1_000), SeekPlan::Restart); // too little produced to trust
    }

    #[test]
    fn the_receivers_position_is_shifted_by_the_start_and_capped_by_the_title() {
        assert_eq!(title_position(60_000, 30_000, Some(3_600_000)), 90_000);
        assert_eq!(title_position(3_599_000, 5_000, Some(3_600_000)), 3_600_000);
        assert_eq!(title_position(0, 5_000, None), 5_000);
    }
}
