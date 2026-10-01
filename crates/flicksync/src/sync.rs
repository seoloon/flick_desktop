//! The synchronization engine: pure and deterministic (no I/O, no timers,
//! no UI, no WebSocket). Time is injected, so tests advance a manual clock
//! instead of sleeping.
//!
//! Three kinds of state are kept apart on purpose:
//!
//! * **server state**: the canonical [`PlaybackSnapshot`] (+ sequence),
//! * **local state**: what the player reports ([`LocalPlayback`]),
//! * **derived state**: drift and the correction chosen ([`Evaluation`]).
//!
//! The room's playback rate is the user's rate: a drift correction is a
//! temporary *multiplier* around it and never overwrites it.

use std::collections::VecDeque;

use crate::protocol::{ClientMessage, Correction, PlayState, PlaybackSnapshot};

/// Drift thresholds and timings. Starting points from the product spec;
/// meant to be tuned against real player behaviour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncConfig {
    /// Below this, nothing is done.
    pub ignore_ms: f64,
    /// Up to this, a subtle rate adjustment.
    pub gentle_ms: f64,
    /// Up to this, a stronger rate adjustment; beyond it, a hard seek.
    pub strong_ms: f64,
    pub gentle_multiplier: f64,
    pub strong_multiplier: f64,
    /// Minimum time between two hard seeks.
    pub seek_cooldown_ms: f64,
    /// Drift that justifies an immediate seek after play/pause/seek/media
    /// change/reconnect, where waiting for a slow rate correction would be
    /// visible.
    pub immediate_seek_ms: f64,
    /// Local evaluation period.
    pub tick_ms: f64,
    /// `sync_report` period while playing.
    pub report_interval_ms: f64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            ignore_ms: 100.0,
            gentle_ms: 500.0,
            strong_ms: 1500.0,
            gentle_multiplier: 0.02,
            strong_multiplier: 0.05,
            seek_cooldown_ms: 3000.0,
            immediate_seek_ms: 250.0,
            tick_ms: 400.0,
            report_interval_ms: 5000.0,
        }
    }
}

/// Where a local playback change comes from. Only [`PlaybackOrigin::User`]
/// is ever sent to the room: everything else would feed back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackOrigin {
    User,
    RemoteSync,
    InternalCorrection,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LocalIntent {
    Play,
    Pause { position: f64 },
    Seek { position: f64 },
    Rate { rate: f64 },
}

/// What the player reports right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalPlayback {
    /// The shared media is loaded and ready.
    pub ready: bool,
    pub position: f64,
    pub paused: bool,
    pub buffering: bool,
    /// Effective speed currently applied by the player.
    pub rate: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayerAction {
    Seek(f64),
    /// Effective speed (room rate × correction multiplier).
    SetRate(f64),
    Play,
    Pause,
}

/// Derived state, for the debug panel.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Evaluation {
    pub actions: Vec<PlayerAction>,
    pub canonical_position: f64,
    /// Positive: local is ahead.
    pub drift_ms: f64,
    pub multiplier: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    Accepted,
    /// Older than what was already applied: ignored.
    Stale,
}

/// Maps the server's wall clock onto the local monotonic clock using ping/pong
/// samples (lowest RTT wins) and smooths the RTT.
#[derive(Debug, Clone, Default)]
pub struct ClockSync {
    samples: VecDeque<(f64, f64)>,
    smoothed_rtt: Option<f64>,
}

const MAX_SAMPLES: usize = 8;

impl ClockSync {
    /// `sent_at`/`received_at`: local monotonic ms around the ping.
    pub fn record(&mut self, sent_at: f64, received_at: f64, server_time: f64) {
        let rtt = received_at - sent_at;
        if !rtt.is_finite() || rtt < 0.0 {
            return;
        }
        let offset = server_time - (sent_at + rtt / 2.0);
        if self.samples.len() == MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back((rtt, offset));
        // One bad measurement must not swing the estimate: EMA, small gain.
        self.smoothed_rtt = Some(self.smoothed_rtt.map_or(rtt, |s| s * 0.8 + rtt * 0.2));
    }

    pub fn has_estimate(&self) -> bool {
        !self.samples.is_empty()
    }

    pub fn rtt_ms(&self) -> Option<f64> {
        self.smoothed_rtt
    }

    /// Offset of the lowest-RTT sample (least asymmetric path error).
    pub fn offset_ms(&self) -> f64 {
        self.samples.iter().min_by(|a, b| a.0.total_cmp(&b.0)).map_or(0.0, |s| s.1)
    }

    pub fn server_now(&self, local_now: f64) -> f64 {
        local_now + self.offset_ms()
    }
}

#[derive(Debug, Clone, Copy)]
struct ServerRate {
    rate: f64,
    until: f64,
}

#[derive(Debug)]
pub struct SyncEngine {
    cfg: SyncConfig,
    pub clock: ClockSync,
    snapshot: Option<PlaybackSnapshot>,
    last_sequence: u64,
    duration: Option<f64>,
    /// Set by an accepted non-heartbeat state: seek right away if needed.
    immediate: bool,
    /// Buffering just ended: resynchronize now.
    resync_after_buffering: bool,
    was_buffering: bool,
    correcting: bool,
    last_seek_at: Option<f64>,
    server_rate: Option<ServerRate>,
    last_multiplier: f64,
}

impl SyncEngine {
    pub fn new(cfg: SyncConfig) -> Self {
        Self {
            cfg,
            clock: ClockSync::default(),
            snapshot: None,
            last_sequence: 0,
            duration: None,
            immediate: false,
            resync_after_buffering: false,
            was_buffering: false,
            correcting: false,
            last_seek_at: None,
            server_rate: None,
            last_multiplier: 1.0,
        }
    }

    pub fn config(&self) -> &SyncConfig {
        &self.cfg
    }

    pub fn last_sequence(&self) -> u64 {
        self.last_sequence
    }

    pub fn snapshot(&self) -> Option<&PlaybackSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn set_duration(&mut self, secs: Option<f64>) {
        self.duration = secs.filter(|d| d.is_finite() && *d > 0.0);
    }

    /// Forgets the room (leave, close): the next room starts from scratch.
    pub fn reset(&mut self) {
        *self = Self { clock: std::mem::take(&mut self.clock), ..Self::new(self.cfg) };
    }

    /// Join, reconnect, media ready: land on the canonical position now.
    pub fn resync_now(&mut self) {
        self.immediate = true;
    }

    /// A new media was selected or the room (re)joined: sequence ordering is
    /// kept, the transient correction state is not.
    pub fn media_changed(&mut self) {
        self.correcting = false;
        self.server_rate = None;
        self.last_multiplier = 1.0;
        self.last_seek_at = None;
    }

    /// Accepts a canonical snapshot unless it is stale. `heartbeat` states
    /// (periodic `sync_state`) do not trigger an immediate seek.
    pub fn apply_snapshot(&mut self, snap: PlaybackSnapshot, heartbeat: bool) -> Applied {
        if snap.sequence < self.last_sequence {
            return Applied::Stale;
        }
        let changed = self.snapshot.is_none_or(|s| s.sequence != snap.sequence);
        self.last_sequence = snap.sequence;
        self.snapshot = Some(snap);
        if changed && !heartbeat {
            self.immediate = true;
            // A new canonical command supersedes a pending server-requested rate.
            self.server_rate = None;
        }
        Applied::Accepted
    }

    /// Applies a server `sync_correction` to one participant. Stale ones are ignored.
    pub fn apply_correction(&mut self, c: &Correction, now: f64) -> Applied {
        let (Correction::AdjustRate { sequence, .. } | Correction::Seek { sequence, .. }) = c;
        if *sequence < self.last_sequence {
            return Applied::Stale;
        }
        match c {
            Correction::AdjustRate { rate, duration_ms, .. } => {
                self.server_rate = Some(ServerRate { rate: *rate, until: now + duration_ms });
            }
            Correction::Seek { .. } => {
                // Handled by the caller as a one-shot seek action; keep the cooldown honest.
                self.last_seek_at = Some(now);
            }
        }
        Applied::Accepted
    }

    /// Expected canonical position at local time `now`.
    pub fn canonical_position(&self, now: f64) -> Option<f64> {
        let s = self.snapshot.as_ref()?;
        let pos = match s.state {
            PlayState::Paused => s.position,
            PlayState::Playing => {
                let elapsed_ms = (self.clock.server_now(now) - s.server_time).max(0.0);
                s.position + elapsed_ms / 1000.0 * s.rate
            }
        };
        Some(self.duration.map_or(pos, |d| pos.min(d)).max(0.0))
    }

    pub fn room_rate(&self) -> f64 {
        self.snapshot.map_or(1.0, |s| s.rate)
    }

    /// Decides what the player must do to follow the room. Called every
    /// `tick_ms` and right after any accepted canonical change.
    pub fn evaluate(&mut self, local: &LocalPlayback, now: f64) -> Evaluation {
        let mut ev = Evaluation { multiplier: 1.0, ..Evaluation::default() };
        let Some(snap) = self.snapshot else { return ev };
        let expected = self.canonical_position(now).unwrap_or(snap.position);
        ev.canonical_position = expected;
        ev.drift_ms = (local.position - expected) * 1000.0;

        // Not loaded yet, or buffering: the room goes on without us. We only
        // remember to resynchronize once the player is back.
        if !local.ready {
            return ev;
        }
        if local.buffering {
            self.was_buffering = true;
            return ev;
        }
        if self.was_buffering {
            self.was_buffering = false;
            self.resync_after_buffering = true;
        }

        let drift = ev.drift_ms;
        let abs = drift.abs();
        let seek_allowed = self.last_seek_at.is_none_or(|t| now - t >= self.cfg.seek_cooldown_ms);

        match snap.state {
            PlayState::Paused => {
                if !local.paused {
                    ev.actions.push(PlayerAction::Pause);
                }
                if abs >= self.cfg.ignore_ms && (self.immediate || seek_allowed) {
                    ev.actions.push(PlayerAction::Seek(expected));
                    self.last_seek_at = Some(now);
                }
                if (local.rate - snap.rate).abs() > 1e-3 {
                    ev.actions.push(PlayerAction::SetRate(snap.rate));
                }
                self.correcting = false;
                self.last_multiplier = 1.0;
            }
            PlayState::Playing => {
                if local.paused {
                    ev.actions.push(PlayerAction::Play);
                }
                let force = self.immediate || self.resync_after_buffering;
                let hard = abs >= self.cfg.strong_ms || (force && abs >= self.cfg.immediate_seek_ms);
                if hard && (force || seek_allowed) {
                    // Land slightly ahead of the canonical position: the seek itself takes a moment.
                    ev.actions.push(PlayerAction::Seek(expected));
                    self.last_seek_at = Some(now);
                    self.correcting = false;
                    self.last_multiplier = 1.0;
                } else {
                    ev.multiplier = self.rate_multiplier(drift, abs, hard);
                    self.last_multiplier = ev.multiplier;
                }
                let target = self.target_rate(snap.rate, ev.multiplier, now);
                if (local.rate - target).abs() > 1e-3 {
                    ev.actions.push(PlayerAction::SetRate(target));
                }
            }
        }
        self.immediate = false;
        self.resync_after_buffering = false;
        ev
    }

    /// Hysteresis: once correcting, keep going until clearly back in sync.
    fn rate_multiplier(&mut self, drift: f64, abs: f64, hard_but_cooling_down: bool) -> f64 {
        let stop_below = self.cfg.ignore_ms / 2.0;
        if abs < stop_below || (!self.correcting && abs < self.cfg.ignore_ms) {
            self.correcting = false;
            return 1.0;
        }
        self.correcting = true;
        let amount =
            if hard_but_cooling_down || abs >= self.cfg.gentle_ms { self.cfg.strong_multiplier } else { self.cfg.gentle_multiplier };
        // Ahead: slow down. Behind: speed up.
        if drift > 0.0 { 1.0 - amount } else { 1.0 + amount }
    }

    fn target_rate(&mut self, room_rate: f64, multiplier: f64, now: f64) -> f64 {
        if let Some(sr) = self.server_rate {
            if now < sr.until {
                return sr.rate.clamp(crate::protocol::RATE_MIN, crate::protocol::RATE_MAX);
            }
            self.server_rate = None;
        }
        (room_rate * multiplier).clamp(crate::protocol::RATE_MIN, crate::protocol::RATE_MAX)
    }

    /// The only way a local change becomes a room command. Remote and
    /// internal changes never are: that is what prevents feedback loops.
    pub fn outbound(&self, origin: PlaybackOrigin, intent: LocalIntent) -> Option<ClientMessage> {
        if origin != PlaybackOrigin::User {
            return None;
        }
        let sequence = Some(self.last_sequence);
        Some(match intent {
            LocalIntent::Play => ClientMessage::Play { position: None, sequence },
            LocalIntent::Pause { position } => ClientMessage::Pause { position: Some(position), sequence },
            LocalIntent::Seek { position } => ClientMessage::Seek { position, sequence },
            LocalIntent::Rate { rate } => ClientMessage::Rate { rate, sequence },
        })
    }

    /// Periodic `sync_report`. Silent while buffering or not ready.
    pub fn report(&self, local: &LocalPlayback) -> Option<ClientMessage> {
        if !local.ready || local.buffering {
            return None;
        }
        let snap = self.snapshot.as_ref()?;
        Some(ClientMessage::SyncReport {
            position: local.position,
            sequence: self.last_sequence,
            state: if local.paused { PlayState::Paused } else { snap.state },
            buffering: false,
            rtt_ms: self.clock.rtt_ms(),
        })
    }

    /// Debug/derived multiplier last applied.
    pub fn multiplier(&self) -> f64 {
        self.last_multiplier
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{Clock, ManualClock};

    const SERVER_EPOCH: f64 = 1_790_000_000_000.0;

    /// Engine whose clock offset maps local 0 ms to SERVER_EPOCH.
    fn engine() -> (SyncEngine, ManualClock) {
        let clock = ManualClock::default();
        let mut e = SyncEngine::new(SyncConfig::default());
        e.clock.record(0.0, 0.0, SERVER_EPOCH);
        (e, clock)
    }

    fn snap(state: PlayState, position: f64, rate: f64, server_time: f64, sequence: u64) -> PlaybackSnapshot {
        PlaybackSnapshot { state, position, rate, server_time, sequence }
    }

    fn local(position: f64, paused: bool, rate: f64) -> LocalPlayback {
        LocalPlayback { ready: true, position, paused, buffering: false, rate }
    }

    /// Engine that is already past its first (immediate) evaluation.
    fn settled(rate: f64) -> (SyncEngine, ManualClock) {
        let (mut e, clock) = engine();
        e.apply_snapshot(snap(PlayState::Playing, 100.0, rate, SERVER_EPOCH, 1), false);
        e.evaluate(&local(100.0, false, rate), clock.now_ms());
        (e, clock)
    }

    #[test]
    fn expected_position_advances_with_time_and_rate() {
        let (mut e, _) = engine();
        e.apply_snapshot(snap(PlayState::Playing, 100.0, 1.0, SERVER_EPOCH, 1), false);
        assert!((e.canonical_position(500.0).unwrap() - 100.5).abs() < 1e-9);
        e.apply_snapshot(snap(PlayState::Playing, 100.0, 2.0, SERVER_EPOCH, 2), false);
        assert!((e.canonical_position(500.0).unwrap() - 101.0).abs() < 1e-9);
    }

    #[test]
    fn paused_position_does_not_advance() {
        let (mut e, _) = engine();
        e.apply_snapshot(snap(PlayState::Paused, 42.0, 1.0, SERVER_EPOCH, 1), false);
        assert_eq!(e.canonical_position(60_000.0), Some(42.0));
    }

    #[test]
    fn stale_snapshots_are_ignored() {
        let (mut e, _) = engine();
        assert_eq!(e.apply_snapshot(snap(PlayState::Playing, 10.0, 1.0, SERVER_EPOCH, 43), false), Applied::Accepted);
        assert_eq!(e.apply_snapshot(snap(PlayState::Paused, 99.0, 1.0, SERVER_EPOCH, 42), false), Applied::Stale);
        assert_eq!(e.last_sequence(), 43);
        assert_eq!(e.snapshot().unwrap().state, PlayState::Playing);
        // The same sequence (heartbeat) is accepted.
        assert_eq!(e.apply_snapshot(snap(PlayState::Playing, 10.0, 1.0, SERVER_EPOCH, 43), true), Applied::Accepted);
    }

    #[test]
    fn stale_corrections_are_ignored() {
        let (mut e, _) = engine();
        e.apply_snapshot(snap(PlayState::Playing, 10.0, 1.0, SERVER_EPOCH, 9), false);
        assert_eq!(e.apply_correction(&Correction::Seek { position: 1.0, sequence: 8 }, 0.0), Applied::Stale);
        assert_eq!(e.apply_correction(&Correction::AdjustRate { rate: 1.03, duration_ms: 1000.0, sequence: 9 }, 0.0), Applied::Accepted);
    }

    /// Drift table: (drift in ms, expected outcome) for a client ahead (+) and behind (-).
    #[test]
    fn drift_correction_thresholds() {
        enum Want {
            Nothing,
            Rate(f64),
            Seek,
        }
        let cases = [
            (0.0, Want::Nothing),
            (50.0, Want::Nothing),
            (100.5, Want::Rate(0.02)),
            (300.0, Want::Rate(0.02)),
            (500.5, Want::Rate(0.05)),
            (1000.0, Want::Rate(0.05)),
            (2000.0, Want::Seek),
        ];
        for sign in [1.0, -1.0] {
            for (drift, want) in &cases {
                let (mut e, clock) = settled(1.0);
                clock.advance(10_000.0);
                // Canonical position now: 110.0 s.
                let l = local(110.0 + sign * drift / 1000.0, false, 1.0);
                let ev = e.evaluate(&l, clock.now_ms());
                match want {
                    Want::Nothing => assert!(ev.actions.is_empty(), "{sign}{drift}: {:?}", ev.actions),
                    Want::Rate(amount) => {
                        let target = 1.0 - sign * amount;
                        assert_eq!(ev.actions.len(), 1, "{sign}{drift}: {:?}", ev.actions);
                        let PlayerAction::SetRate(r) = ev.actions[0] else { panic!("{:?}", ev.actions) };
                        assert!((r - target).abs() < 1e-9, "{sign}{drift}: {r} vs {target}");
                    }
                    Want::Seek => {
                        assert!(matches!(ev.actions[0], PlayerAction::Seek(p) if (p - 110.0).abs() < 1e-6), "{:?}", ev.actions);
                    }
                }
                assert!((ev.drift_ms - sign * drift).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn correction_is_a_multiplier_around_the_user_rate() {
        let (mut e, clock) = settled(1.5);
        clock.advance(10_000.0);
        // Canonical: 100 + 10 * 1.5 = 115. Local is 300 ms ahead.
        let ev = e.evaluate(&local(115.3, false, 1.5), clock.now_ms());
        let PlayerAction::SetRate(r) = ev.actions[0] else { panic!() };
        assert!((r - 1.5 * 0.98).abs() < 1e-9);
        // Back in sync: restored to exactly the user's rate.
        clock.advance(400.0);
        let ev = e.evaluate(&local(115.6, false, 1.5 * 0.98), clock.now_ms());
        assert_eq!(ev.actions, vec![PlayerAction::SetRate(1.5)]);
    }

    #[test]
    fn hysteresis_avoids_flapping_around_the_threshold() {
        let (mut e, clock) = settled(1.0);
        clock.advance(10_000.0);
        e.evaluate(&local(110.2, false, 1.0), clock.now_ms()); // 200 ms ahead: start slowing
        // 80 ms is under ignore (100) but above the stop threshold (50): keep correcting.
        let ev = e.evaluate(&local(110.08, false, 0.98), clock.now_ms());
        assert!(ev.actions.is_empty(), "{:?}", ev.actions);
        assert!((ev.multiplier - 0.98).abs() < 1e-9);
        let ev = e.evaluate(&local(110.03, false, 0.98), clock.now_ms());
        assert_eq!(ev.actions, vec![PlayerAction::SetRate(1.0)]);
    }

    #[test]
    fn hard_seeks_respect_the_cooldown() {
        let (mut e, clock) = settled(1.0);
        clock.advance(10_000.0);
        let ev = e.evaluate(&local(120.0, false, 1.0), clock.now_ms());
        assert!(matches!(ev.actions[0], PlayerAction::Seek(_)));
        clock.advance(400.0);
        // Still far off right after: no second seek, strong rate correction instead.
        let ev = e.evaluate(&local(120.0, false, 1.0), clock.now_ms());
        assert!(ev.actions.iter().all(|a| !matches!(a, PlayerAction::Seek(_))), "{:?}", ev.actions);
        assert!(matches!(ev.actions[0], PlayerAction::SetRate(r) if r < 1.0));
        clock.advance(3000.0);
        let ev = e.evaluate(&local(130.0, false, 1.0), clock.now_ms());
        assert!(matches!(ev.actions[0], PlayerAction::Seek(_)));
    }

    #[test]
    fn a_new_canonical_state_seeks_immediately_even_for_moderate_drift() {
        let (mut e, clock) = settled(1.0);
        clock.advance(1000.0);
        e.apply_snapshot(snap(PlayState::Playing, 300.0, 1.0, SERVER_EPOCH + 1000.0, 2), false);
        let ev = e.evaluate(&local(101.0, false, 1.0), clock.now_ms());
        assert!(matches!(ev.actions[0], PlayerAction::Seek(p) if (p - 300.0).abs() < 1e-6));
        // 300 ms off after a seek command: immediate seek, not a slow rate correction.
        e.apply_snapshot(snap(PlayState::Playing, 300.0, 1.0, SERVER_EPOCH + 1000.0, 3), false);
        let ev = e.evaluate(&local(300.3, false, 1.0), clock.now_ms());
        assert!(matches!(ev.actions[0], PlayerAction::Seek(_)));
    }

    #[test]
    fn pause_from_the_room_pauses_and_lands_on_the_canonical_position() {
        let (mut e, clock) = settled(1.0);
        e.apply_snapshot(snap(PlayState::Paused, 123.4, 1.0, SERVER_EPOCH + 500.0, 2), false);
        let ev = e.evaluate(&local(123.9, false, 1.0), clock.now_ms());
        assert_eq!(ev.actions, vec![PlayerAction::Pause, PlayerAction::Seek(123.4)]);
    }

    #[test]
    fn join_while_paused_loads_paused_at_the_position() {
        let (mut e, clock) = engine();
        e.apply_snapshot(snap(PlayState::Paused, 50.0, 1.0, SERVER_EPOCH, 5), false);
        let ev = e.evaluate(&local(0.0, true, 1.0), clock.now_ms());
        assert_eq!(ev.actions, vec![PlayerAction::Seek(50.0)]);
    }

    #[test]
    fn join_while_playing_starts_at_the_derived_position() {
        let (mut e, clock) = engine();
        e.apply_snapshot(snap(PlayState::Playing, 100.0, 1.0, SERVER_EPOCH - 5_000.0, 5), false);
        clock.advance(2_000.0);
        // Position had advanced 5 s before we received it, plus 2 s since.
        let ev = e.evaluate(&local(0.0, true, 1.0), clock.now_ms());
        assert_eq!(ev.actions[0], PlayerAction::Play);
        assert!(matches!(ev.actions[1], PlayerAction::Seek(p) if (p - 107.0).abs() < 1e-6));
    }

    #[test]
    fn nothing_happens_until_the_media_is_ready() {
        let (mut e, clock) = engine();
        e.apply_snapshot(snap(PlayState::Playing, 100.0, 1.0, SERVER_EPOCH, 5), false);
        let mut l = local(0.0, true, 1.0);
        l.ready = false;
        assert!(e.evaluate(&l, clock.now_ms()).actions.is_empty());
    }

    #[test]
    fn buffering_neither_corrects_nor_pauses_the_room_and_resyncs_after() {
        let (mut e, clock) = settled(1.0);
        let mut l = local(105.0, false, 1.0);
        l.buffering = true;
        clock.advance(10_000.0);
        assert!(e.evaluate(&l, clock.now_ms()).actions.is_empty());
        // The room kept playing: no report, no pause command.
        assert!(e.report(&l).is_none());
        // Back: 5 s behind the room, resync immediately.
        l.buffering = false;
        let ev = e.evaluate(&l, clock.now_ms());
        assert!(matches!(ev.actions[0], PlayerAction::Seek(p) if (p - 110.0).abs() < 1e-6), "{:?}", ev.actions);
    }

    #[test]
    fn server_rate_correction_takes_over_until_it_expires() {
        let (mut e, clock) = settled(1.0);
        clock.advance(1000.0);
        e.apply_correction(&Correction::AdjustRate { rate: 1.03, duration_ms: 2000.0, sequence: 1 }, clock.now_ms());
        let ev = e.evaluate(&local(101.0, false, 1.0), clock.now_ms());
        assert_eq!(ev.actions, vec![PlayerAction::SetRate(1.03)]);
        clock.advance(2500.0);
        let ev = e.evaluate(&local(103.5, false, 1.03), clock.now_ms());
        assert_eq!(ev.actions, vec![PlayerAction::SetRate(1.0)]);
    }

    // --- feedback prevention -------------------------------------------------

    #[test]
    fn remote_pause_does_not_generate_a_pause_command() {
        let (mut e, clock) = settled(1.0);
        e.apply_snapshot(snap(PlayState::Paused, 10.0, 1.0, SERVER_EPOCH, 2), false);
        let ev = e.evaluate(&local(10.0, false, 1.0), clock.now_ms());
        assert!(ev.actions.contains(&PlayerAction::Pause));
        // The player then reports its own pause: it was applied by RemoteSync.
        assert_eq!(e.outbound(PlaybackOrigin::RemoteSync, LocalIntent::Pause { position: 10.0 }), None);
    }

    #[test]
    fn internal_correction_does_not_generate_a_user_event() {
        let (e, _) = settled(1.0);
        assert_eq!(e.outbound(PlaybackOrigin::InternalCorrection, LocalIntent::Rate { rate: 0.98 }), None);
        assert_eq!(e.outbound(PlaybackOrigin::InternalCorrection, LocalIntent::Seek { position: 5.0 }), None);
    }

    #[test]
    fn user_actions_become_commands_with_the_last_applied_sequence() {
        let (e, _) = settled(1.0);
        assert_eq!(
            e.outbound(PlaybackOrigin::User, LocalIntent::Seek { position: 540.25 }),
            Some(ClientMessage::Seek { position: 540.25, sequence: Some(1) })
        );
        assert_eq!(e.outbound(PlaybackOrigin::User, LocalIntent::Play), Some(ClientMessage::Play { position: None, sequence: Some(1) }));
    }

    // --- clock ----------------------------------------------------------------

    #[test]
    fn clock_sync_uses_the_lowest_rtt_sample_and_smooths_rtt() {
        let mut c = ClockSync::default();
        // Slow, asymmetric sample.
        c.record(0.0, 400.0, 1_000_200.0 + 150.0);
        // Fast sample: rtt 20, true offset 1_000_000.
        c.record(1000.0, 1020.0, 1_000_000.0 + 1010.0);
        assert!((c.offset_ms() - 1_000_000.0).abs() < 1e-6);
        let rtt = c.rtt_ms().unwrap();
        assert!(rtt > 20.0 && rtt < 400.0, "one bad sample must not dominate: {rtt}");
        assert!((c.server_now(5.0) - 1_000_005.0).abs() < 1e-6);
    }

    #[test]
    fn clock_sync_ignores_nonsense_samples() {
        let mut c = ClockSync::default();
        c.record(10.0, 5.0, 1.0);
        assert!(!c.has_estimate());
    }

    #[test]
    fn reports_are_silent_when_not_ready_and_carry_sequence_and_rtt() {
        let (mut e, _) = engine();
        e.apply_snapshot(snap(PlayState::Playing, 1.0, 1.0, SERVER_EPOCH, 7), false);
        let mut l = local(1.0, false, 1.0);
        let Some(ClientMessage::SyncReport { sequence, buffering, .. }) = e.report(&l) else { panic!() };
        assert_eq!((sequence, buffering), (7, false));
        l.ready = false;
        assert!(e.report(&l).is_none());
    }

    #[test]
    fn reset_forgets_the_room_but_keeps_the_clock_estimate() {
        let (mut e, _) = engine();
        e.apply_snapshot(snap(PlayState::Playing, 1.0, 1.0, SERVER_EPOCH, 7), false);
        e.reset();
        assert!(e.snapshot().is_none());
        assert_eq!(e.last_sequence(), 0);
        assert!(e.clock.has_estimate());
    }
}
