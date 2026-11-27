use spook_core::{
    motion_gray8, DirectorConfig, DirectorInput, DirectorSnapshot, HauntDirector, HauntLevel,
    HauntProgression, HauntState,
};

/// Director seed used until the host calls `configure_seed`. The progression
/// stays on its fixed (unseeded) timing in that case.
pub(crate) const DEFAULT_DIRECTOR_SEED: u32 = 0;
/// Temporal/skull cost units available to the director. Measured resource
/// adaptation is separate work.
const QUALITY_BUDGET: u8 = 2;

/// Shared progression (opening, phase, strength, activity) plus the director
/// that selects LAG, ECHO and SKULL. Both replay from the same session seed.
pub(crate) struct Automatic {
    progression: HauntProgression,
    director: HauntDirector,
    previous: Vec<u8>,
    sampled: Option<u64>,
    pub level: HauntLevel,
}

impl Automatic {
    pub fn new(pixels: usize) -> Result<Self, String> {
        let mut previous = Vec::new();
        previous
            .try_reserve_exact(pixels)
            .map_err(|_| "motion buffer allocation failed")?;
        previous.resize(pixels, 0);
        Ok(Self {
            progression: HauntProgression::new().map_err(|e| e.to_string())?,
            director: HauntDirector::new(DEFAULT_DIRECTOR_SEED, DirectorConfig::default())
                .map_err(|e| e.to_string())?,
            previous,
            sampled: None,
            level: HauntLevel {
                state: HauntState::Idle,
                strength: 0,
                activity: 0,
            },
        })
    }

    /// Replaces the progression and the director with versions seeded from
    /// `seed` and resets the session. The same seed replays the same stage
    /// durations and event selections. Both are built before either is
    /// replaced, so a failure changes nothing.
    pub fn set_seed(&mut self, seed: u32) -> Result<(), String> {
        let progression = HauntProgression::with_seed(seed).map_err(|e| e.to_string())?;
        let director =
            HauntDirector::new(seed, DirectorConfig::default()).map_err(|e| e.to_string())?;
        self.progression = progression;
        self.director = director;
        self.reset();
        Ok(())
    }

    pub fn update(
        &mut self,
        gray: &[u8],
        width: usize,
        height: usize,
        now: u64,
        reduced: bool,
        head_available: bool,
    ) -> Result<(HauntLevel, DirectorSnapshot), String> {
        let due = self
            .sampled
            .is_none_or(|last| now.saturating_sub(last) >= 100);
        let activity = if due && self.sampled.is_some() {
            motion_gray8(gray, &self.previous, width, height, width, 12)
                .map_err(|e| e.to_string())?
                .activity
        } else if due {
            0
        } else {
            self.level.activity
        };
        let level = self
            .progression
            .update(now, activity, reduced)
            .map_err(|e| e.to_string())?;
        // The director runs on a copy (fixed size, no heap) and is committed
        // only on success, so an error never leaves it half-advanced.
        let mut director = self.director.clone();
        let snapshot = director
            .update(
                now,
                DirectorInput {
                    level,
                    head_available,
                    reduced_motion: reduced,
                    quality_budget: QUALITY_BUDGET,
                },
            )
            .map_err(|e| e.to_string())?;
        self.director = director;
        if due {
            self.previous.copy_from_slice(gray);
            self.sampled = Some(now);
        }
        self.level = level;
        Ok((level, snapshot))
    }

    pub fn reset(&mut self) {
        self.progression.reset();
        self.director.reset();
        self.sampled = None;
        self.level = HauntLevel {
            state: HauntState::Idle,
            strength: 0,
            activity: 0,
        };
    }
}
