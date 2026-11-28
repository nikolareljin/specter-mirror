//! Clock-driven display gate. Re-reading one observation cannot qualify a visit.
pub const FRESH_MS: u64 = 500;
pub const ABSENCE_MS: u64 = 3000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Face {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub confidence: u32,
}

impl Face {
    pub fn valid(self) -> bool {
        (800..=1000).contains(&self.confidence)
            && self.width > 0
            && self.height > 0
            && self.x.checked_add(self.width).is_some_and(|v| v <= 160)
            && self.y.checked_add(self.height).is_some_and(|v| v <= 120)
    }
}

#[derive(Default)]
pub struct Presence {
    last_tick: Option<u64>,
    observed: Option<u64>,
    streak: Option<u64>,
    last_face: Option<u64>,
    live: bool,
}

impl Presence {
    pub fn live(&self) -> bool {
        self.live
    }

    pub fn tick(
        &mut self,
        now: u64,
        result: Option<(u64, Option<Face>)>,
    ) -> Result<bool, &'static str> {
        if self.last_tick.is_some_and(|last| now < last) {
            return Err("presence time regressed");
        }
        if let Some((at, _)) = result {
            if at > now || self.observed.is_some_and(|last| at <= last) {
                return Err("invalid observation time");
            }
        }
        if self.observed.is_some_and(|at| now - at > FRESH_MS) {
            self.streak = None;
        }
        if let Some((at, face)) = result {
            self.observed = Some(at);
            if now - at <= FRESH_MS && face.is_some_and(Face::valid) {
                let start = *self.streak.get_or_insert(at);
                self.last_face = Some(at);
                if at - start > 2000 {
                    self.live = true;
                }
            } else {
                self.streak = None;
            }
        }
        if self.live && self.last_face.is_none_or(|at| now - at >= ABSENCE_MS) {
            self.live = false;
            self.streak = None;
        }
        self.last_tick = Some(now);
        Ok(self.live)
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn face() -> Option<Face> {
        Some(Face {
            x: 20,
            y: 10,
            width: 30,
            height: 40,
            confidence: 800,
        })
    }
    #[test]
    fn threshold_is_strict_and_departure_has_grace() {
        let mut p = Presence::default();
        for at in [0, 400, 800, 1200, 1600, 1999, 2000] {
            assert!(!p.tick(at, Some((at, face()))).unwrap());
        }
        assert!(p.tick(2001, Some((2001, face()))).unwrap());
        assert!(p.tick(5000, None).unwrap());
        assert!(!p.tick(5001, None).unwrap());
        assert!(!p.tick(5002, Some((5002, face()))).unwrap());
    }
    #[test]
    fn stale_polling_negative_and_invalid_bounds_cannot_activate() {
        let mut p = Presence::default();
        p.tick(0, Some((0, face()))).unwrap();
        for at in 1..3000 {
            assert!(!p.tick(at, None).unwrap());
        }
        assert!(!p.tick(3000, Some((3000, face()))).unwrap());
        assert!(!p.tick(3200, Some((3200, None))).unwrap());
        assert!(!p.tick(3400, Some((3400, face()))).unwrap());
        assert!(!p.tick(6000, Some((6000, face()))).unwrap());
        let mut bad = face().unwrap();
        bad.x = 160;
        assert!(!p.tick(6200, Some((6200, Some(bad)))).unwrap());
    }
    #[test]
    fn invalid_time_is_transactional_and_reset_forgets_visitors() {
        let mut p = Presence::default();
        p.tick(10, Some((10, face()))).unwrap();
        assert!(p.tick(9, None).is_err());
        assert!(p.tick(11, Some((12, face()))).is_err());
        assert!(p.tick(11, Some((10, face()))).is_err());
        assert_eq!(p.last_tick, Some(10));
        p.reset();
        assert!(!p.live());
        assert!(p.observed.is_none());
    }
}
