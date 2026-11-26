//! Bounded, seeded event director. It chooses which existing layers (delayed
//! reflection, ghost trails, skull) to request from the shared haunting
//! level. It renders nothing, detects nothing and keeps no buffers.
//!
//! # Policy
//!
//! * Candidates are the masks 1..=7 in ascending order (LAG=1, ECHO=2,
//!   SKULL=4). A candidate must be enabled, must have a head when it uses
//!   SKULL, must fit the budget with `cost = any(LAG|ECHO) + any(SKULL)` and
//!   must not use a layer whose 8000 ms cooldown is running.
//! * New events start only in Transforming/Haunted with strength >= 1500,
//!   reduced motion off and at least 3000 ms after the previous event ended.
//!   Recovering may continue an active event but never starts one.
//! * Weights (integer division): lag = 2 + (1000 - activity) / 250,
//!   echo = 2 + activity / 250, skull = 2 + (1000 - activity) / 250. A mask
//!   weighs the sum of its layers, halved (floor, minimum 1) when it is among
//!   the last four selected masks. The immediately previous mask is excluded
//!   unless it is the only eligible candidate.
//! * One draw, `next % total`, walks the candidates in ascending order. The
//!   modulo bias is below 2^-55 for these totals and accepted.
//! * Per event, three further draws follow in this order: duration
//!   4000..=8000 ms, maximum delay 150..=450 ms, maximum trail weight
//!   48..=128 (each `lo + next % span`). Waiting never draws.
//! * Envelope in permille: `min(elapsed, 1000, duration - elapsed)` capped at
//!   1000, so it rises for 1000 ms, holds, and falls for the last 1000 ms.
//!   Strength = `(envelope * level.strength + 500) / 1000` (round half up).
//!   Delay and trail = `max * strength / 6000` (floor), zero without their bit.
//!   Zero strength yields the all-zero neutral snapshot.
//! * RNG: SplitMix64 seeded with the constructor seed as the initial state.

use crate::{HauntLevel, HauntState, HAUNT_FULL_STRENGTH};
use std::fmt;

pub const LAG: u8 = 1;
pub const ECHO: u8 = 2;
pub const SKULL: u8 = 4;

const ALL_LAYERS: u8 = LAG | ECHO | SKULL;
const MIN_START_STRENGTH: u16 = 1500;
const MAX_ACTIVITY: u16 = 1000;
const MAX_BUDGET: u8 = 2;
const FADE_MS: u64 = 1000;
const GLOBAL_COOLDOWN_MS: u64 = 3000;
const LAYER_COOLDOWN_MS: u64 = 8000;
const HISTORY: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectorConfig {
    pub enabled_layers: u8,
}

impl Default for DirectorConfig {
    fn default() -> Self {
        Self {
            enabled_layers: ALL_LAYERS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectorInput {
    pub level: HauntLevel,
    pub head_available: bool,
    pub reduced_motion: bool,
    /// 0..=2 temporal/skull cost units.
    pub quality_budget: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirectorSnapshot {
    pub layers: u8,
    pub strength: u16,
    pub delay_ms: u16,
    pub trail_strength: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectorError {
    UnknownLayers,
    QualityBudget,
    Strength,
    Activity,
    TimeRegressed,
}

impl fmt::Display for DirectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnknownLayers => "unknown director layer bits",
            Self::QualityBudget => "quality budget must be 0..=2",
            Self::Strength => "haunt strength must be 0..=6000",
            Self::Activity => "haunt activity must be 0..=1000",
            Self::TimeRegressed => "director time moved backwards",
        })
    }
}

impl std::error::Error for DirectorError {}

#[derive(Clone, Copy, Debug)]
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn range(&mut self, lo: u64, hi_inclusive: u64) -> u64 {
        lo + self.next() % (hi_inclusive - lo + 1)
    }
}

#[derive(Clone, Copy, Debug)]
struct Event {
    mask: u8,
    start: u64,
    duration: u16,
    max_delay: u16,
    max_trail: u8,
}

/// Fixed-size director state (well under 512 bytes, no heap).
#[derive(Clone, Debug)]
pub struct HauntDirector {
    seed: u32,
    config: DirectorConfig,
    rng: SplitMix64,
    clock: Option<u64>,
    active: Option<Event>,
    recent: [u8; HISTORY],
    recent_len: u8,
    recent_next: u8,
    global_end: Option<u64>,
    layer_end: [Option<u64>; 3],
}

fn cost(mask: u8) -> u8 {
    u8::from(mask & (LAG | ECHO) != 0) + u8::from(mask & SKULL != 0)
}

/// Layer weights as (lag, echo, skull) for a validated activity.
fn layer_weights(activity: u16) -> (u32, u32, u32) {
    let a = u32::from(activity);
    (2 + (1000 - a) / 250, 2 + a / 250, 2 + (1000 - a) / 250)
}

fn base_weight(mask: u8, activity: u16) -> u32 {
    let (lag, echo, skull) = layer_weights(activity);
    (if mask & LAG != 0 { lag } else { 0 })
        + (if mask & ECHO != 0 { echo } else { 0 })
        + (if mask & SKULL != 0 { skull } else { 0 })
}

impl HauntDirector {
    pub fn new(seed: u32, config: DirectorConfig) -> Result<Self, DirectorError> {
        if config.enabled_layers & !ALL_LAYERS != 0 {
            return Err(DirectorError::UnknownLayers);
        }
        Ok(Self {
            seed,
            config,
            rng: SplitMix64(u64::from(seed)),
            clock: None,
            active: None,
            recent: [0; HISTORY],
            recent_len: 0,
            recent_next: 0,
            global_end: None,
            layer_end: [None; 3],
        })
    }

    /// Clears events, history, cooldowns and clock; replays the seed.
    pub fn reset(&mut self) {
        *self = Self {
            seed: self.seed,
            config: self.config,
            rng: SplitMix64(u64::from(self.seed)),
            clock: None,
            active: None,
            recent: [0; HISTORY],
            recent_len: 0,
            recent_next: 0,
            global_end: None,
            layer_end: [None; 3],
        };
    }

    pub fn update(
        &mut self,
        now_ms: u64,
        input: DirectorInput,
    ) -> Result<DirectorSnapshot, DirectorError> {
        if input.quality_budget > MAX_BUDGET {
            return Err(DirectorError::QualityBudget);
        }
        if input.level.strength > HAUNT_FULL_STRENGTH {
            return Err(DirectorError::Strength);
        }
        if input.level.activity > MAX_ACTIVITY {
            return Err(DirectorError::Activity);
        }
        if let Some(last) = self.clock {
            if now_ms < last {
                return Err(DirectorError::TimeRegressed);
            }
        }
        self.clock = Some(now_ms);

        let phase_ok = matches!(
            input.level.state,
            HauntState::Transforming | HauntState::Haunted | HauntState::Recovering
        );
        if let Some(event) = self.active {
            let keep = phase_ok
                && !input.reduced_motion
                && input.level.strength > 0
                && self.mask_allowed(event.mask, &input);
            if !keep || now_ms - event.start >= u64::from(event.duration) {
                self.finish(now_ms);
                return Ok(DirectorSnapshot::default());
            }
            return Ok(Self::snapshot(&event, now_ms, input.level.strength));
        }

        let may_start = matches!(
            input.level.state,
            HauntState::Transforming | HauntState::Haunted
        ) && input.level.strength >= MIN_START_STRENGTH
            && !input.reduced_motion;
        if may_start {
            self.try_start(now_ms, &input);
        }
        Ok(match self.active {
            Some(event) => Self::snapshot(&event, now_ms, input.level.strength),
            None => DirectorSnapshot::default(),
        })
    }

    fn mask_allowed(&self, mask: u8, input: &DirectorInput) -> bool {
        mask & !self.config.enabled_layers == 0
            && (mask & SKULL == 0 || input.head_available)
            && cost(mask) <= input.quality_budget
    }

    fn finish(&mut self, now: u64) {
        if let Some(event) = self.active.take() {
            self.global_end = Some(now);
            for (bit, end) in self.layer_end.iter_mut().enumerate() {
                if event.mask & (1 << bit) != 0 {
                    *end = Some(now);
                }
            }
        }
    }

    fn previous(&self) -> Option<u8> {
        (self.recent_len > 0)
            .then(|| self.recent[(usize::from(self.recent_next) + HISTORY - 1) % HISTORY])
    }

    fn recently_used(&self, mask: u8) -> bool {
        self.recent[..usize::from(self.recent_len)].contains(&mask)
    }

    fn weight(&self, mask: u8, activity: u16) -> u32 {
        let w = base_weight(mask, activity);
        if self.recently_used(mask) {
            (w / 2).max(1)
        } else {
            w
        }
    }

    fn candidate_ok(&self, mask: u8, now: u64, input: &DirectorInput) -> bool {
        if !self.mask_allowed(mask, input) {
            return false;
        }
        (0..3).all(|bit| {
            mask & (1 << bit) == 0
                || match self.layer_end[bit] {
                    Some(end) => now - end >= LAYER_COOLDOWN_MS,
                    None => true,
                }
        })
    }

    fn try_start(&mut self, now: u64, input: &DirectorInput) {
        if let Some(end) = self.global_end {
            if now - end < GLOBAL_COOLDOWN_MS {
                return;
            }
        }
        let previous = self.previous();
        let has_alternative =
            (1..=ALL_LAYERS).any(|m| Some(m) != previous && self.candidate_ok(m, now, input));
        let usable = |this: &Self, m: u8| {
            this.candidate_ok(m, now, input) && !(has_alternative && Some(m) == previous)
        };
        let activity = input.level.activity;
        let total: u32 = (1..=ALL_LAYERS)
            .filter(|&m| usable(self, m))
            .map(|m| self.weight(m, activity))
            .sum();
        if total == 0 {
            return;
        }
        let mut draw = (self.rng.next() % u64::from(total)) as u32;
        let mut chosen = 0;
        for m in 1..=ALL_LAYERS {
            if !usable(self, m) {
                continue;
            }
            let w = self.weight(m, activity);
            if draw < w {
                chosen = m;
                break;
            }
            draw -= w;
        }
        let duration = self.rng.range(4000, 8000) as u16;
        let max_delay = self.rng.range(150, 450) as u16;
        let max_trail = self.rng.range(48, 128) as u8;
        self.recent[usize::from(self.recent_next)] = chosen;
        self.recent_next = (self.recent_next + 1) % HISTORY as u8;
        self.recent_len = (self.recent_len + 1).min(HISTORY as u8);
        self.active = Some(Event {
            mask: chosen,
            start: now,
            duration,
            max_delay,
            max_trail,
        });
    }

    fn snapshot(event: &Event, now: u64, level_strength: u16) -> DirectorSnapshot {
        let elapsed = now - event.start;
        let remaining = u64::from(event.duration) - elapsed;
        let envelope = elapsed.min(remaining).min(FADE_MS) as u32;
        let strength = (envelope * u32::from(level_strength) + 500) / 1000;
        if strength == 0 {
            return DirectorSnapshot::default();
        }
        let full = u32::from(HAUNT_FULL_STRENGTH);
        DirectorSnapshot {
            layers: event.mask,
            strength: strength as u16,
            delay_ms: if event.mask & LAG != 0 {
                (u32::from(event.max_delay) * strength / full) as u16
            } else {
                0
            },
            trail_strength: if event.mask & ECHO != 0 {
                (u32::from(event.max_trail) * strength / full) as u8
            } else {
                0
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_weights_follow_documented_formula() {
        assert_eq!(layer_weights(0), (6, 2, 6));
        assert_eq!(layer_weights(249), (5, 2, 5));
        assert_eq!(layer_weights(250), (5, 3, 5));
        assert_eq!(layer_weights(1000), (2, 6, 2));
        assert_eq!(base_weight(7, 500), 12);
        assert_eq!(base_weight(5, 0), 12);
        assert_eq!(base_weight(2, 1000), 6);
    }

    #[test]
    fn recent_masks_are_halved_with_a_floor_of_one() {
        let mut d = HauntDirector::new(1, DirectorConfig::default()).unwrap();
        assert_eq!(d.weight(7, 500), 12);
        d.recent = [7, 1, 0, 0];
        d.recent_len = 2;
        assert_eq!(d.weight(7, 500), 6);
        assert_eq!(d.weight(1, 1000), 1); // (2 / 2) = 1
        assert_eq!(d.weight(2, 1000), 6);
        d.recent = [2, 0, 0, 0];
        d.recent_len = 1;
        assert_eq!(d.weight(2, 1000), 3);
        assert_eq!(d.weight(3, 1000), 8);
    }

    #[test]
    fn only_four_recent_masks_are_retained() {
        let mut d = HauntDirector::new(1, DirectorConfig::default()).unwrap();
        for m in [1u8, 2, 3, 4, 5] {
            d.recent[usize::from(d.recent_next)] = m;
            d.recent_next = (d.recent_next + 1) % 4;
            d.recent_len = (d.recent_len + 1).min(4);
        }
        assert!(!d.recently_used(1));
        assert!(d.recently_used(5));
        assert_eq!(d.previous(), Some(5));
    }

    #[test]
    fn splitmix_reference_values() {
        // Published SplitMix64 outputs for state 0.
        let mut r = SplitMix64(0);
        assert_eq!(r.next(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(r.next(), 0x6E78_9E6A_A1B9_65F4);
    }

    #[test]
    fn storage_is_small() {
        assert!(std::mem::size_of::<HauntDirector>() < 512);
    }
}
