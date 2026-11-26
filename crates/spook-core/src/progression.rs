use crate::{HauntConfig, HauntError, HauntMachine, HauntState};

pub const HAUNT_FULL_STRENGTH: u16 = 6000;
/// Seeded stage durations are drawn from this range (inclusive).
pub const MIN_STAGE_MS: u64 = 4000;
pub const MAX_STAGE_MS: u64 = 15000;
/// Progress is kept in 1/1024 strength units so rates need not be integers.
const FIXED_POINT: u64 = 1024;
const FULL_PROGRESS: u64 = HAUNT_FULL_STRENGTH as u64 * FIXED_POINT;

/// Stage durations for one visit: clear opening, rise to full strength, the
/// stillness grace that holds the effect, and the return to a clear mirror.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HauntTiming {
    pub intro_ms: u64,
    pub rise_ms: u64,
    pub hold_ms: u64,
    pub recovery_ms: u64,
}

impl HauntTiming {
    /// The original fixed timing, used without a seed.
    pub const FIXED: Self = Self {
        intro_ms: 5000,
        rise_ms: 6000,
        hold_ms: 10_000,
        recovery_ms: 3000,
    };
}

/// SplitMix64 over a host-supplied seed. No external entropy or clock: the
/// same seed yields the same sequence of timings. The seed is not an identity.
#[derive(Clone, Copy, Debug)]
struct TimingPlanner {
    seed: u32,
    state: u64,
}

impl TimingPlanner {
    fn new(seed: u32) -> Self {
        Self {
            seed,
            state: u64::from(seed),
        }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn stage(&mut self) -> u64 {
        MIN_STAGE_MS + self.next() % (MAX_STAGE_MS - MIN_STAGE_MS + 1)
    }

    fn draw(&mut self) -> HauntTiming {
        HauntTiming {
            intro_ms: self.stage(),
            rise_ms: self.stage(),
            hold_ms: self.stage(),
            recovery_ms: self.stage(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HauntLevel {
    pub state: HauntState,
    pub strength: u16,
    pub activity: u16,
}

/// Shared intensity envelope. Without a seed it is the fixed cycle: five
/// seconds clear, six up, a ten second stillness grace, three down, all
/// linear. With a seed every visit draws its own 4 to 15 second opening, rise,
/// grace and return, and strength follows a smoothstep curve so changes start
/// and end gently. Motion is a changed-pixel estimate; no head or person
/// detection is implied.
pub struct HauntProgression {
    machine: HauntMachine,
    timing: HauntTiming,
    planner: Option<TimingPlanner>,
    clear_until: Option<u64>,
    last_now: Option<u64>,
    last_state: HauntState,
    progress: u64,
    rise_remainder: u64,
    fall_remainder: u64,
    rising: bool,
}

fn config(timing: HauntTiming) -> HauntConfig {
    HauntConfig {
        enter_activity: 5,
        leave_activity: 1,
        transform_ms: timing.rise_ms,
        stillness_ms: timing.hold_ms,
        recovery_ms: timing.recovery_ms,
        ..HauntConfig::default()
    }
}

/// Integer smoothstep over 0..=6000: 3s^2 - 2s^3.
fn ease(linear: u64) -> u16 {
    let full = u64::from(HAUNT_FULL_STRENGTH);
    (linear * linear * (3 * full - 2 * linear) / (full * full)) as u16
}

impl HauntProgression {
    pub fn new() -> Result<Self, HauntError> {
        Self::build(HauntTiming::FIXED, None)
    }

    /// Randomized timing from a host seed (any `u32`).
    pub fn with_seed(seed: u32) -> Result<Self, HauntError> {
        let mut planner = TimingPlanner::new(seed);
        let timing = planner.draw();
        Self::build(timing, Some(planner))
    }

    fn build(timing: HauntTiming, planner: Option<TimingPlanner>) -> Result<Self, HauntError> {
        Ok(Self {
            machine: HauntMachine::new(config(timing))?,
            timing,
            planner,
            clear_until: None,
            last_now: None,
            last_state: HauntState::Idle,
            progress: 0,
            rise_remainder: 0,
            fall_remainder: 0,
            rising: false,
        })
    }

    /// The durations in effect for the current visit.
    pub fn timing(&self) -> HauntTiming {
        self.timing
    }

    pub fn seed(&self) -> Option<u32> {
        self.planner.map(|p| p.seed)
    }

    /// Invalid activity/time changes nothing. Reduced motion immediately
    /// clears intensity; other transitions retain a continuous envelope.
    pub fn update(
        &mut self,
        now_ms: u64,
        activity: u16,
        reduced_motion: bool,
    ) -> Result<HauntLevel, HauntError> {
        let snapshot = self.machine.update(now_ms, activity)?;
        let clear_until = *self
            .clear_until
            .get_or_insert(now_ms.saturating_add(self.timing.intro_ms));
        let dt = self.last_now.map_or(0, |last| now_ms - last);
        let rising = now_ms >= clear_until
            && matches!(
                snapshot.state,
                HauntState::Transforming | HauntState::Haunted
            )
            && !reduced_motion;
        if reduced_motion {
            self.progress = 0;
            self.rise_remainder = 0;
            self.fall_remainder = 0;
        } else if rising {
            let elapsed = if self.rising {
                dt
            } else {
                dt.min(now_ms - clear_until).min(snapshot.state_elapsed_ms)
            };
            if !self.rising {
                self.rise_remainder = 0;
            }
            self.fall_remainder = 0;
            let numerator = elapsed.min(self.timing.rise_ms) * FULL_PROGRESS + self.rise_remainder;
            let step = numerator / self.timing.rise_ms;
            self.rise_remainder = numerator % self.timing.rise_ms;
            self.progress = (self.progress + step).min(FULL_PROGRESS);
            if self.progress == FULL_PROGRESS {
                self.rise_remainder = 0;
            }
        } else {
            let elapsed = if self.rising {
                dt.min(snapshot.state_elapsed_ms)
            } else {
                dt
            };
            if self.rising {
                self.fall_remainder = 0;
            }
            self.rise_remainder = 0;
            let numerator =
                elapsed.min(self.timing.recovery_ms) * FULL_PROGRESS + self.fall_remainder;
            let step = numerator / self.timing.recovery_ms;
            self.fall_remainder = numerator % self.timing.recovery_ms;
            self.progress = self.progress.saturating_sub(step);
            if self.progress == 0 {
                self.fall_remainder = 0;
            }
        }
        // A completed return starts the next visit with freshly drawn stages.
        if self.last_state == HauntState::Recovering && snapshot.state == HauntState::Idle {
            if let Some(planner) = self.planner.as_mut() {
                let next = planner.draw();
                self.machine
                    .set_durations(next.rise_ms, next.hold_ms, next.recovery_ms)?;
                self.timing = next;
                self.clear_until = Some(now_ms.saturating_add(next.intro_ms));
            }
        }
        self.last_state = snapshot.state;
        self.last_now = Some(now_ms);
        self.rising = rising;
        let linear = self.progress / FIXED_POINT;
        let strength = if self.planner.is_some() {
            ease(linear)
        } else {
            linear as u16
        };
        Ok(HauntLevel {
            state: snapshot.state,
            strength,
            activity,
        })
    }

    pub fn reset(&mut self) {
        self.machine.reset();
        if let Some(planner) = self.planner.as_mut() {
            // Replay the same seed from the start of its sequence.
            *planner = TimingPlanner::new(planner.seed);
            self.timing = planner.draw();
            let timing = self.timing;
            let _ = self
                .machine
                .set_durations(timing.rise_ms, timing.hold_ms, timing.recovery_ms);
        }
        self.clear_until = None;
        self.last_now = None;
        self.last_state = HauntState::Idle;
        self.progress = 0;
        self.rise_remainder = 0;
        self.fall_remainder = 0;
        self.rising = false;
    }
}
