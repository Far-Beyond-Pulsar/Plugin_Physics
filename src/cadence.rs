//! How steadily the simulation thread runs: runs per second and the longest gap between two
//! runs, reported once a second so a stall that input (or anything else) causes shows in the log
//! (target `physics_plugin::cadence`).

use std::time::{Duration, Instant};

/// One window of the simulation thread's runs.
#[derive(Debug)]
pub struct Cadence {
    window_start: Instant,
    last_run: Option<Instant>,
    runs: u32,
    longest_gap: Duration,
    busiest: Duration,
}

/// What a closed window measured.
#[derive(Debug, PartialEq)]
pub struct Report {
    pub runs: u32,
    pub longest_gap: Duration,
    /// The longest a single run took.
    pub longest_run: Duration,
}

impl Cadence {
    pub fn new(now: Instant) -> Self {
        Self {
            window_start: now,
            last_run: None,
            runs: 0,
            longest_gap: Duration::ZERO,
            busiest: Duration::ZERO,
        }
    }

    /// A run began at `start` and took `took`. Returns the window's report once a second has
    /// passed.
    pub fn record(&mut self, start: Instant, took: Duration) -> Option<Report> {
        if let Some(last) = self.last_run {
            self.longest_gap = self.longest_gap.max(start.saturating_duration_since(last));
        }
        self.last_run = Some(start);
        self.runs += 1;
        self.busiest = self.busiest.max(took);
        if start.saturating_duration_since(self.window_start) < Duration::from_secs(1) {
            return None;
        }
        let report = Report {
            runs: self.runs,
            longest_gap: self.longest_gap,
            longest_run: self.busiest,
        };
        *self = Self::new(start);
        self.last_run = Some(start);
        Some(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_reports_runs_and_the_longest_gap() {
        let t0 = Instant::now();
        let mut c = Cadence::new(t0);
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        for ms in [0, 16, 33, 50, 250, 266, 283, 300] {
            assert!(c.record(at(ms), Duration::from_millis(1)).is_none());
        }
        let report = c.record(at(1010), Duration::from_millis(4)).unwrap();
        assert_eq!(report.runs, 9);
        assert_eq!(report.longest_gap, Duration::from_millis(710), "300 to 1010 is the stall");
        assert_eq!(report.longest_run, Duration::from_millis(4));
    }
}
