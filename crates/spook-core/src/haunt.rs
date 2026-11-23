//! Clock-driven interaction states driven by motion activity.
//!
//! `PresenceDetected` is a motion estimate, not person detection. Stillness is
//! given a bounded grace period; leaving it is a timed guess, not proof that a
//! visitor departed. The machine has no wall clock, allocation or I/O.

use std::fmt;

const MAX_DURATION_MS: u64 = 120_000;
const MAX_ACTIVITY: u16 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HauntState {
    Idle,
    PresenceDetected,
    Transforming,
    Haunted,
    Recovering,
}

/// Thresholds are activity values (0..=1000); durations are milliseconds.
/// `enter_activity` must be in `1..1000` (1 to 999), `leave_activity` below it,
/// and each duration in `1..120000` (1 to 119999).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HauntConfig {
    pub enter_activity: u16,
    pub leave_activity: u16,
    pub debounce_ms: u64,
    pub transform_ms: u64,
    pub stillness_ms: u64,
    pub recovery_ms: u64,
}

impl Default for HauntConfig {
    fn default() -> Self {
        Self {
            enter_activity: 50,
            leave_activity: 20,
            debounce_ms: 500,
            transform_ms: 4000,
            stillness_ms: 10_000,
            recovery_ms: 3000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HauntSnapshot {
    pub state: HauntState,
    /// Zero right after a transition, otherwise `now - state entry time`.
    pub state_elapsed_ms: u64,
    /// The hysteresis latch after this update.
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HauntError {
    InvalidConfig,
    ActivityOutOfRange,
    TimeRegression,
}

impl fmt::Display for HauntError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidConfig => "invalid interaction configuration",
            Self::ActivityOutOfRange => "activity must not exceed 1000",
            Self::TimeRegression => "timestamp is earlier than the previous update",
        })
    }
}

impl std::error::Error for HauntError {}

/// Fixed-size state: configuration, state, entry time, clock and latch.
#[derive(Clone, Debug)]
pub struct HauntMachine {
    config: HauntConfig,
    state: HauntState,
    entered_ms: u64,
    last_now: Option<u64>,
    active: bool,
    last_active: Option<u64>,
}

impl HauntMachine {
    pub fn new(config: HauntConfig) -> Result<Self, HauntError> {
        let duration_ok = |ms: u64| (1..MAX_DURATION_MS).contains(&ms);
        let valid = (1..MAX_ACTIVITY).contains(&config.enter_activity)
            && config.leave_activity < config.enter_activity
            && duration_ok(config.debounce_ms)
            && duration_ok(config.transform_ms)
            && duration_ok(config.stillness_ms)
            && duration_ok(config.recovery_ms);
        if !valid {
            return Err(HauntError::InvalidConfig);
        }
        Ok(Self {
            config,
            state: HauntState::Idle,
            entered_ms: 0,
            last_now: None,
            active: false,
            last_active: None,
        })
    }

    /// Applies one observation. Activity above 1000 and regressing time are
    /// rejected before any change; equal time is accepted. The first valid
    /// update starts the clock (state elapsed zero). At most one transition
    /// happens per call, so large time jumps take constant time.
    pub fn update(&mut self, now_ms: u64, activity: u16) -> Result<HauntSnapshot, HauntError> {
        if activity > MAX_ACTIVITY {
            return Err(HauntError::ActivityOutOfRange);
        }
        if self.last_now.is_some_and(|last| now_ms < last) {
            return Err(HauntError::TimeRegression);
        }
        if self.last_now.is_none() {
            self.entered_ms = now_ms;
        }
        self.last_now = Some(now_ms);

        if activity >= self.config.enter_activity {
            self.active = true;
        } else if activity <= self.config.leave_activity {
            self.active = false;
        }
        if self.active {
            self.last_active = Some(now_ms);
        }

        let cfg = self.config;
        let elapsed = now_ms - self.entered_ms;
        // Without a recorded active time (unreachable in states that use it),
        // report zero stillness so the grace period never ends spuriously.
        let still_for = self.last_active.map_or(0, |at| now_ms - at);
        let still_over = !self.active && still_for >= cfg.stillness_ms;
        let next = match self.state {
            HauntState::Idle if self.active => Some(HauntState::PresenceDetected),
            HauntState::PresenceDetected if !self.active => Some(HauntState::Idle),
            HauntState::PresenceDetected if elapsed >= cfg.debounce_ms => {
                Some(HauntState::Transforming)
            }
            HauntState::Transforming if still_over => Some(HauntState::Recovering),
            HauntState::Transforming if elapsed >= cfg.transform_ms => Some(HauntState::Haunted),
            HauntState::Haunted if still_over => Some(HauntState::Recovering),
            HauntState::Recovering if self.active => Some(HauntState::PresenceDetected),
            HauntState::Recovering if elapsed >= cfg.recovery_ms => Some(HauntState::Idle),
            _ => None,
        };
        let state_elapsed_ms = match next {
            Some(state) => {
                self.state = state;
                self.entered_ms = now_ms;
                0
            }
            None => elapsed,
        };
        Ok(HauntSnapshot {
            state: self.state,
            state_elapsed_ms,
            active: self.active,
        })
    }

    /// Back to `Idle` with no clock and an inactive latch; config is kept.
    pub fn reset(&mut self) {
        self.state = HauntState::Idle;
        self.entered_ms = 0;
        self.last_now = None;
        self.active = false;
        self.last_active = None;
    }

    pub fn state(&self) -> HauntState {
        self.state
    }
}
