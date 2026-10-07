//! How the History panel lays out a file's snapshots: by day, then by writing session.

use std::ops::Range;

/// Snapshots less than this many seconds apart belong to one session.
pub const SESSION_GAP: i64 = 30 * 60;

/// One day's snapshots, as ranges of indices into the newest-first list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Day {
    /// The `day_of` key its snapshots share.
    pub key: i64,
    /// Its sessions, newest first: runs of snapshots less than `SESSION_GAP` apart.
    pub sessions: Vec<Range<usize>>,
}

/// Groups snapshot times (seconds, newest first) into days and sessions. `day_of` names a
/// time's local day; a session never spans two days.
pub fn group(times: &[i64], day_of: impl Fn(i64) -> i64) -> Vec<Day> {
    let mut days: Vec<Day> = Vec::new();
    for (i, &time) in times.iter().enumerate() {
        let key = day_of(time);
        if days.last().is_none_or(|day| day.key != key) {
            days.push(Day { key, sessions: Vec::new() });
        }
        let sessions = &mut days.last_mut().expect("just pushed").sessions;
        match sessions.last_mut() {
            Some(session) if times[session.end - 1] - time < SESSION_GAP => session.end = i + 1,
            _ => sessions.push(i..i + 1),
        }
    }
    days
}

/// How many words each snapshot added (or took out) since the one before it, given word counts
/// newest first. None for the oldest, which has nothing to compare with.
pub fn changes(words: &[usize]) -> Vec<Option<i64>> {
    (0..words.len())
        .map(|i| words.get(i + 1).map(|&before| words[i] as i64 - before as i64))
        .collect()
}

/// The words a run of snapshots added or took out, all told; None if none of them can say.
pub fn total(changes: &[Option<i64>]) -> Option<i64> {
    changes.iter().flatten().copied().reduce(|a, b| a + b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 60 * 60;
    const DAY: i64 = 24 * HOUR;

    fn by_utc_day(time: i64) -> i64 {
        time.div_euclid(DAY)
    }

    #[test]
    fn snapshots_close_together_are_one_session() {
        // Newest first: 14:32, 14:20, 13:58, then a gap, 11:48, 11:41.
        let at = |h: i64, m: i64| 3 * DAY + h * HOUR + m * 60;
        let times = [at(14, 32), at(14, 20), at(13, 58), at(11, 48), at(11, 41)];
        assert_eq!(
            group(&times, by_utc_day),
            vec![Day {
                key: 3,
                sessions: vec![0..3, 3..5]
            }]
        );
    }

    #[test]
    fn a_gap_of_exactly_half_an_hour_starts_a_new_session() {
        let times = [SESSION_GAP + 10, 10];
        assert_eq!(group(&times, by_utc_day)[0].sessions, vec![0..1, 1..2]);
        let times = [SESSION_GAP + 9, 10];
        assert_eq!(group(&times, by_utc_day)[0].sessions, vec![0..2]);
    }

    #[test]
    fn a_session_stops_at_midnight() {
        let times = [DAY + 5 * 60, DAY - 5 * 60];
        let days = group(&times, by_utc_day);
        assert_eq!(days.iter().map(|d| (d.key, d.sessions.clone())).collect::<Vec<_>>(), vec![(1, vec![0..1]), (0, vec![1..2])]);
    }

    #[test]
    fn no_snapshots_no_days() {
        assert_eq!(group(&[], by_utc_day), vec![]);
    }

    #[test]
    fn changes_compare_each_snapshot_with_the_one_before() {
        assert_eq!(changes(&[239, 235, 252, 223]), vec![Some(4), Some(-17), Some(29), None]);
        assert_eq!(total(&changes(&[239, 235, 252, 223])[..3]), Some(16));
        assert_eq!(total(&[None]), None);
        assert_eq!(total(&[Some(0), None]), Some(0));
    }
}
