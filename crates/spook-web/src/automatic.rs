use spook_core::{motion_gray8, HauntLevel, HauntProgression, HauntState};

pub(crate) struct Automatic {
    progression: HauntProgression,
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
            previous,
            sampled: None,
            level: HauntLevel {
                state: HauntState::Idle,
                strength: 0,
                activity: 0,
            },
        })
    }

    /// Replaces the progression with seeded randomized timing and resets the
    /// session. The same seed replays the same stage durations.
    pub fn set_seed(&mut self, seed: u32) -> Result<(), String> {
        self.progression = HauntProgression::with_seed(seed).map_err(|e| e.to_string())?;
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
    ) -> Result<HauntLevel, String> {
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
        if due {
            self.previous.copy_from_slice(gray);
            self.sampled = Some(now);
        }
        self.level = level;
        Ok(level)
    }

    pub fn reset(&mut self) {
        self.progression.reset();
        self.sampled = None;
        self.level = HauntLevel {
            state: HauntState::Idle,
            strength: 0,
            activity: 0,
        };
    }
}
