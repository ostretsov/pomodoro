//! The countdown itself.  Time is passed in as a [`Duration`] since an
//! arbitrary fixed point (the app uses the boot clock, which keeps counting
//! during suspend), so every transition is testable without sleeping.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Not started (or reset): shows the full duration.
    Idle,
    Running,
    /// Stopped while running; Start resumes with the time that was left.
    Paused,
    /// The time ran out.
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Running { ends_at: Duration },
    Paused { remaining: Duration },
    Finished,
}

#[derive(Debug, Clone)]
pub struct Timer {
    duration: Duration,
    state: State,
}

impl Timer {
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            state: State::Idle,
        }
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn phase(&self) -> Phase {
        match self.state {
            State::Idle => Phase::Idle,
            State::Running { .. } => Phase::Running,
            State::Paused { .. } => Phase::Paused,
            State::Finished => Phase::Finished,
        }
    }

    /// Starts a full period, or resumes a paused one.  No-op while running.
    pub fn start(&mut self, now: Duration) {
        self.state = match self.state {
            State::Idle | State::Finished => State::Running {
                ends_at: now + self.duration,
            },
            State::Paused { remaining } => State::Running {
                ends_at: now + remaining,
            },
            running @ State::Running { .. } => running,
        };
    }

    /// Pauses a running timer, keeping the time that is left.
    pub fn stop(&mut self, now: Duration) {
        if let State::Running { ends_at } = self.state {
            self.state = State::Paused {
                remaining: ends_at.saturating_sub(now),
            };
        }
    }

    /// Back to the full duration, not running.
    pub fn reset(&mut self) {
        self.state = State::Idle;
    }

    /// Advances the clock.  Returns true exactly once: when the time runs out.
    pub fn tick(&mut self, now: Duration) -> bool {
        match self.state {
            State::Running { ends_at } if now >= ends_at => {
                self.state = State::Finished;
                true
            }
            _ => false,
        }
    }

    pub fn remaining(&self, now: Duration) -> Duration {
        match self.state {
            State::Idle => self.duration,
            State::Running { ends_at } => ends_at.saturating_sub(now),
            State::Paused { remaining } => remaining,
            State::Finished => Duration::ZERO,
        }
    }

    /// The part of the period that is left: 1.0 at the start, 0.0 at the end.
    pub fn fraction_left(&self, now: Duration) -> f64 {
        if self.duration.is_zero() {
            return 0.0;
        }
        (self.remaining(now).as_secs_f64() / self.duration.as_secs_f64()).clamp(0.0, 1.0)
    }
}

/// `mm:ss`, rounded up to whole seconds (a countdown shows 0:00 only at the end).
pub fn format_remaining(d: Duration) -> String {
    let secs = d.as_secs() + u64::from(d.subsec_nanos() > 0);
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    fn s(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    #[test]
    fn new_timer_is_idle_with_full_time() {
        let t = Timer::new(20 * MIN);
        assert_eq!(t.phase(), Phase::Idle);
        assert_eq!(t.remaining(s(1000)), 20 * MIN);
        assert_eq!(t.fraction_left(s(1000)), 1.0);
    }

    #[test]
    fn counts_down_and_finishes_once() {
        let mut t = Timer::new(20 * MIN);
        t.start(s(100));
        assert_eq!(t.phase(), Phase::Running);
        assert_eq!(t.remaining(s(160)), 19 * MIN);
        assert!((t.fraction_left(s(700)) - 0.5).abs() < 1e-9);
        assert!(!t.tick(s(100) + 20 * MIN - s(1)));
        assert!(t.tick(s(100) + 20 * MIN));
        assert_eq!(t.phase(), Phase::Finished);
        assert_eq!(t.remaining(s(5000)), Duration::ZERO);
        assert!(!t.tick(s(5000)), "finishes only once");
    }

    #[test]
    fn late_tick_still_finishes() {
        let mut t = Timer::new(MIN);
        t.start(s(0));
        assert!(t.tick(s(3600)));
    }

    #[test]
    fn stop_pauses_and_start_resumes() {
        let mut t = Timer::new(20 * MIN);
        t.start(s(0));
        t.stop(s(300));
        assert_eq!(t.phase(), Phase::Paused);
        assert_eq!(t.remaining(s(10_000)), 15 * MIN, "time stands still");
        assert!(!t.tick(s(10_000)));
        t.start(s(10_000));
        assert_eq!(t.remaining(s(10_060)), 14 * MIN);
        assert!(t.tick(s(10_000) + 15 * MIN));
    }

    #[test]
    fn start_while_running_changes_nothing() {
        let mut t = Timer::new(20 * MIN);
        t.start(s(0));
        t.start(s(600));
        assert_eq!(t.remaining(s(600)), 10 * MIN);
    }

    #[test]
    fn stop_when_not_running_changes_nothing() {
        let mut t = Timer::new(MIN);
        t.stop(s(5));
        assert_eq!(t.phase(), Phase::Idle);
        t.start(s(0));
        t.tick(s(60));
        t.stop(s(70));
        assert_eq!(t.phase(), Phase::Finished);
    }

    #[test]
    fn reset_returns_to_full_time() {
        for stop in [false, true] {
            let mut t = Timer::new(20 * MIN);
            t.start(s(0));
            if stop {
                t.stop(s(60));
            }
            t.reset();
            assert_eq!(t.phase(), Phase::Idle);
            assert_eq!(t.remaining(s(900)), 20 * MIN);
            assert!(!t.tick(s(100_000)));
        }
    }

    #[test]
    fn start_after_finish_runs_a_new_full_period() {
        let mut t = Timer::new(MIN);
        t.start(s(0));
        t.tick(s(60));
        t.start(s(100));
        assert_eq!(t.phase(), Phase::Running);
        assert_eq!(t.remaining(s(100)), MIN);
    }

    #[test]
    fn formats_rounding_up() {
        assert_eq!(format_remaining(20 * MIN), "20:00");
        assert_eq!(format_remaining(Duration::from_millis(59_001)), "01:00");
        assert_eq!(format_remaining(Duration::from_millis(59_000)), "00:59");
        assert_eq!(format_remaining(Duration::from_millis(1)), "00:01");
        assert_eq!(format_remaining(Duration::ZERO), "00:00");
        assert_eq!(format_remaining(100 * MIN), "100:00");
    }

    #[test]
    fn zero_duration_does_not_divide_by_zero() {
        let t = Timer::new(Duration::ZERO);
        assert_eq!(t.fraction_left(s(0)), 0.0);
    }
}
