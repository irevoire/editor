use jiff::{SignedDuration, Timestamp};

/// Represent the state of an animation that execute once and stops.
#[derive(Debug, Clone, Copy)]
pub struct OneShotAnimation {
    started_at: Timestamp,
    duration: SignedDuration,
}

impl OneShotAnimation {
    pub fn start(now: Timestamp, duration: SignedDuration) -> Self {
        Self {
            started_at: now,
            duration,
        }
    }

    /// Progress in `[0.0, 1.0]`, clamped, recomputed fresh from `now` every call.
    pub fn progress(&self, now: Timestamp) -> f32 {
        if self.duration <= SignedDuration::ZERO {
            return 1.0;
        }
        let elapsed = now.duration_since(self.started_at);
        (elapsed.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0)
    }

    pub fn is_done(&self, now: Timestamp) -> bool {
        self.progress(now) >= 1.0
    }

    /// `Some` while the animation hasn't finished, giving the next timestamp
    /// a caller should redraw at (`now + frame_interval`, capped so it never
    /// suggests a wakeup past the animation's end). `None` once done.
    pub fn next_wakeup(&self, now: Timestamp, frame_interval: SignedDuration) -> Option<Timestamp> {
        if self.is_done(now) {
            return None;
        }
        let end = self.started_at + self.duration;
        let next = now + frame_interval;
        Some(next.min(end))
    }
}

/// Represent the state of an animation that repeat itself for ever.
#[derive(Debug, Clone, Copy)]
pub struct LoopingAnimation {
    started_at: Timestamp,
    step_duration: SignedDuration,
}

impl LoopingAnimation {
    pub fn start(now: Timestamp, step_duration: SignedDuration) -> Self {
        Self {
            started_at: now,
            step_duration,
        }
    }

    /// how many full steps have elapsed since `started_at`.
    pub fn step(&self, now: Timestamp) -> usize {
        if self.step_duration <= SignedDuration::ZERO {
            return 0;
        }
        let elapsed = now.duration_since(self.started_at).as_secs_f64();
        let step_duration = self.step_duration.as_secs_f64();
        (elapsed / step_duration).max(0.0).floor() as usize
    }

    pub fn next_wakeup(&self, now: Timestamp) -> Option<Timestamp> {
        let next_step = i32::try_from(self.step(now) + 1).unwrap_or(i32::MAX);
        Some(self.started_at + self.step_duration * next_step)
    }
}

/// Cubic ease-out: fast start, slowly settling into place, no overshoot.
pub fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

#[cfg(test)]
mod test {
    use jiff::ToSpan;

    use super::*;

    #[test]
    fn one_shot_animation_progress_is_clamped_and_pure() {
        let t0 = Timestamp::UNIX_EPOCH;
        let anim = OneShotAnimation::start(t0, SignedDuration::from_millis(200));

        assert_eq!(anim.progress(t0), 0.0);
        assert!((anim.progress(t0 + 100.milliseconds()) - 0.5).abs() < 1e-6);
        assert_eq!(anim.progress(t0 + 200.milliseconds()), 1.0);
        assert_eq!(anim.progress(t0 + 5.seconds()), 1.0);
        assert!(!anim.is_done(t0));
        assert!(anim.is_done(t0 + 200.milliseconds()));
        assert!(anim.is_done(t0 + 5.seconds()));
    }

    #[test]
    fn one_shot_animation_next_wakeup_caps_at_end_then_none() {
        let t0 = Timestamp::UNIX_EPOCH;
        let anim = OneShotAnimation::start(t0, SignedDuration::from_millis(200));
        let one_second = SignedDuration::from_secs(1);

        assert_eq!(
            anim.next_wakeup(t0, one_second),
            Some(t0 + 200.milliseconds())
        );
        assert_eq!(
            anim.next_wakeup(t0 + 150.milliseconds(), one_second),
            Some(t0 + 200.milliseconds())
        );
        assert_eq!(anim.next_wakeup(t0 + 200.milliseconds(), one_second), None);
        assert_eq!(anim.next_wakeup(t0 + 5.seconds(), one_second), None);
    }

    #[test]
    fn ease_out_cubic_bounds_and_monotonic() {
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert_eq!(ease_out_cubic(1.0), 1.0);

        let samples = [0.0, 0.25, 0.5, 0.75, 1.0];
        let mut prev = -1.0;
        for t in samples {
            let v = ease_out_cubic(t);
            assert!(v <= 1.0);
            assert!(v >= prev);
            prev = v;
        }
    }

    #[test]
    fn looping_animation_is_lag_proof() {
        let t0 = Timestamp::UNIX_EPOCH;
        let step = LoopingAnimation::start(t0, SignedDuration::from_millis(500));

        assert_eq!(step.step(t0), 0);
        assert_eq!(step.step(t0 + 499.milliseconds()), 0);
        assert_eq!(step.step(t0 + 500.milliseconds()), 1);

        // A single large gap must jump straight to the correct step, not
        // advance by one the way a per-call increment would.
        let jumped = t0 + 10.seconds() + 550.milliseconds();
        assert_eq!(step.step(jumped), 21);
        assert_eq!(step.next_wakeup(jumped), Some(t0 + 11.seconds()));
    }
}
