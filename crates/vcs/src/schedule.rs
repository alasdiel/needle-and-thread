use std::time::{Duration, Instant};

/// Decides when to take an automatic snapshot, the way Google Docs groups its version history:
/// once writing pauses for `idle`, or every `max_gap` during writing that never pauses.
/// Switching scenes and closing the app snapshot right away; callers do that directly.
#[derive(Debug, Clone)]
pub struct Scheduler {
    idle: Duration,
    max_gap: Duration,
    /// First and latest edit since the last snapshot.
    pending: Option<(Instant, Instant)>,
}

impl Scheduler {
    pub fn new(idle: Duration, max_gap: Duration) -> Self {
        Self {
            idle,
            max_gap,
            pending: None,
        }
    }

    pub fn edited(&mut self, now: Instant) {
        let first = self.pending.map_or(now, |(first, _)| first);
        self.pending = Some((first, now));
    }

    pub fn has_changes(&self) -> bool {
        self.pending.is_some()
    }

    pub fn is_due(&self, now: Instant) -> bool {
        self.pending.is_some_and(|(first, last)| {
            now.saturating_duration_since(last) >= self.idle || now.saturating_duration_since(first) >= self.max_gap
        })
    }

    /// Call after a snapshot (automatic or not) so the next one waits for new edits.
    pub fn taken(&mut self) {
        self.pending = None;
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new(Duration::from_secs(2 * 60), Duration::from_secs(10 * 60))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: Duration = Duration::from_secs(1);

    #[test]
    fn nothing_is_due_without_edits() {
        assert!(!Scheduler::default().is_due(Instant::now() + 3600 * SEC));
    }

    #[test]
    fn due_after_a_pause() {
        let t0 = Instant::now();
        let mut s = Scheduler::default();
        s.edited(t0);
        assert!(!s.is_due(t0 + 119 * SEC));
        assert!(s.is_due(t0 + 120 * SEC));
    }

    #[test]
    fn continuous_writing_still_snapshots_every_ten_minutes() {
        let t0 = Instant::now();
        let mut s = Scheduler::default();
        // An edit every 30 s never leaves a 2-minute pause.
        for i in 0..22 {
            s.edited(t0 + i * 30 * SEC);
            assert_eq!(s.is_due(t0 + i * 30 * SEC + SEC), i * 30 + 1 >= 600, "at {}s", i * 30 + 1);
        }
    }

    #[test]
    fn taking_a_snapshot_resets_the_clock() {
        let t0 = Instant::now();
        let mut s = Scheduler::default();
        s.edited(t0);
        s.taken();
        assert!(!s.has_changes());
        s.edited(t0 + 500 * SEC);
        assert!(!s.is_due(t0 + 600 * SEC));
    }
}
