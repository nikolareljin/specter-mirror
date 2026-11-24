use crate::{HauntConfig, HauntError, HauntMachine, HauntState};

pub const HAUNT_FULL_STRENGTH: u16 = 6000;
const INTRODUCTION_MS: u64 = 5000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HauntLevel {
    pub state: HauntState,
    pub strength: u16,
    pub activity: u16,
}

/// Shared intensity envelope: five seconds clear, six seconds up, three down.
/// Motion is a changed-pixel estimate. No head or person detection is implied.
pub struct HauntProgression {
    machine: HauntMachine,
    started: Option<u64>,
    last_now: Option<u64>,
    strength: u16,
    rising: bool,
}

impl HauntProgression {
    pub fn new() -> Result<Self, HauntError> {
        Ok(Self {
            machine: HauntMachine::new(HauntConfig {
                enter_activity: 5,
                leave_activity: 1,
                transform_ms: 6000,
                ..HauntConfig::default()
            })?,
            started: None,
            last_now: None,
            strength: 0,
            rising: false,
        })
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
        let age = now_ms - *self.started.get_or_insert(now_ms);
        let dt = self.last_now.map_or(0, |last| now_ms - last);
        let rising = age >= INTRODUCTION_MS
            && matches!(
                snapshot.state,
                HauntState::Transforming | HauntState::Haunted
            )
            && !reduced_motion;
        if reduced_motion {
            self.strength = 0;
        } else if rising {
            let elapsed = if self.rising {
                dt
            } else {
                dt.min(age.saturating_sub(INTRODUCTION_MS))
                    .min(snapshot.state_elapsed_ms)
            };
            self.strength = (u64::from(self.strength) + elapsed.min(6000)).min(6000) as u16;
        } else {
            let elapsed = if self.rising {
                dt.min(snapshot.state_elapsed_ms)
            } else {
                dt
            };
            self.strength = self.strength.saturating_sub((elapsed.min(6000) * 2) as u16);
        }
        self.last_now = Some(now_ms);
        self.rising = rising;
        Ok(HauntLevel {
            state: snapshot.state,
            strength: self.strength,
            activity,
        })
    }

    pub fn reset(&mut self) {
        self.machine.reset();
        self.started = None;
        self.last_now = None;
        self.strength = 0;
        self.rising = false;
    }
}
