//! Where scenes and plot points fall in story time (DESIGN §6), from the `when` in their headers:
//!
//! | `when` | |
//! |---|---|
//! | `"1998-03-14 19:00"`, `"3 Thaw 412 AD"` | a point in the calendar |
//! | `"Day 4"`, `"Day 4 08:00"` | counted from the calendar's `day_one` |
//! | `{ from = "The wedding", offset = "+3d" }` | measured from another scene or plot point |
//! | `{ after = "The harbor" }`, `{ before = … }`, both | just after, before or between |
//! | *(none)* | unplaced: it waits in the tray |
//!
//! [`resolve`] follows each relative time back to a fixed point, places order-only items
//! between their neighbours, and flags what it can't place: names that find nothing, chains
//! that come back on themselves, and orders that can't all be true.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::calendar::{Calendar, DayOne, MINUTES_PER_DAY, Moment, Offset};
use crate::header::Header;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum When {
    Fixed(Moment),
    /// `Day 4`: day 1 is the calendar's `day_one`.
    Day { day: i64, time: Option<(i64, i64)> },
    Relative { from: String, offset: Offset },
    Order { after: Option<String>, before: Option<String> },
}

impl When {
    /// The `when` in a scene's or note's header, if it has one.
    pub fn from_header(header: &Header, calendar: &Calendar) -> Option<Result<Self, String>> {
        if let Some(text) = header.text("when") {
            return Some(Self::parse(&text, calendar));
        }
        let table = header.table("when")?;
        Some(Self::from_table(&table))
    }

    /// A `when` written as text: a date, or `Day 4`.
    pub fn parse(text: &str, calendar: &Calendar) -> Result<Self, String> {
        if let Some(when) = parse_day(text) {
            return when;
        }
        calendar.parse(text).map(Self::Fixed).map_err(|e| e.0)
    }

    fn from_table(fields: &[(String, String)]) -> Result<Self, String> {
        let get = |key: &str| fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.trim().to_owned());
        if let Some((key, _)) = fields.iter().find(|(k, _)| !["from", "offset", "after", "before"].contains(&k.as_str())) {
            return Err(format!("when doesn't take {key:?}: use from and offset, or after and before"));
        }
        match (get("from"), get("offset"), get("after"), get("before")) {
            (Some(from), offset, None, None) => {
                let offset = offset.map(|o| Offset::parse(&o)).transpose().map_err(|e| e.0)?.unwrap_or_default();
                Ok(Self::Relative { from, offset })
            }
            (None, None, after, before) if after.is_some() || before.is_some() => Ok(Self::Order { after, before }),
            (None, Some(_), _, _) => Err("an offset needs a from: what it's measured from".into()),
            (None, None, None, None) => Err("when is empty".into()),
            _ => Err("when takes from and offset, or after and before, not both".into()),
        }
    }
}

/// `Day 4`, `day 4 08:00`, `Day 4, 08:00`.
fn parse_day(text: &str) -> Option<Result<When, String>> {
    let text = text.trim();
    let rest = text.get(..4).filter(|d| d.eq_ignore_ascii_case("day "))?;
    let rest = text[rest.len()..].replace(',', " ");
    let mut words = rest.split_whitespace();
    let bad = || Err(format!("{text:?} isn't a day like Day 4 or Day 4 08:00"));
    let Some(Ok(day)) = words.next().map(str::parse::<i64>) else { return Some(bad()) };
    let time = match words.next() {
        None => None,
        Some(t) => {
            let parsed = t.split_once(':').and_then(|(h, m)| Some((h.parse::<i64>().ok()?, m.parse::<i64>().ok()?)));
            match parsed {
                Some((h, m)) if (0..24).contains(&h) && (0..60).contains(&m) => Some((h, m)),
                _ => return Some(bad()),
            }
        }
    };
    if words.next().is_some() || day < 1 {
        return Some(bad());
    }
    Some(Ok(When::Day { day, time }))
}

/// What a name in a `when` finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    Found(usize),
    Ambiguous,
    Missing,
}

/// Why something isn't on the timeline, or is on it under protest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The `when` couldn't be read; says why.
    Unreadable(String),
    /// A name that finds no scene or plot point.
    Missing(String),
    /// A name that fits more than one.
    Ambiguous(String),
    /// Its time is measured, in the end, from itself.
    Cycle,
    /// It's measured from (or ordered against) something that isn't placed: the item's index.
    WaitsOn(usize),
    /// Its order can't all be true, e.g. after something that comes later than what it's
    /// before. It's placed by the first half (after) and the rest is ignored.
    Contradiction,
    /// `Day 4` in a timeline that also has dates, with no `day_one` to say where day 1 is.
    NoDayOne,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Placed {
    /// Its time, if it can be worked out.
    pub time: Option<Moment>,
    /// Where it comes in story order, or `None` if it waits in the tray.
    pub position: Option<usize>,
    /// Placed by order alone (or measured from something that is): its neighbours are known,
    /// its time isn't.
    pub loose: bool,
    pub problem: Option<Problem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Timeline {
    /// Items in story order, by their index in what was given to [`resolve`].
    pub order: Vec<usize>,
    pub items: Vec<Placed>,
    /// The timeline counts days ("Day 4") rather than dates: nothing has a date and no
    /// `day_one` was set, so times are minutes from the start of Day 1.
    pub days_only: bool,
    /// The start of Day 1, when it's known.
    pub day_one: Option<Moment>,
}

impl Timeline {
    /// Which day it is, counting the day of `day_one` as Day 1.
    pub fn day_number(&self, moment: Moment) -> Option<i64> {
        let start = self.day_one?;
        Some(moment.day() - start.day() + 1)
    }

    /// A time in words: `Day 4, 08:00` on a timeline of days, a date otherwise.
    pub fn label(&self, calendar: &Calendar, moment: Moment) -> String {
        if !self.days_only {
            return calendar.format(moment);
        }
        let day = self.day_number(moment).unwrap_or_default();
        let minute = moment.minute.rem_euclid(MINUTES_PER_DAY);
        match moment.timed {
            true => format!("Day {day}, {:02}:{:02}", minute / 60, minute % 60),
            false => format!("Day {day}"),
        }
    }
}

/// How far the first pass got with an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Time {
    Exact(Moment),
    /// Measured from an order-only item (`root`), roughly `minutes` after it.
    Loose { root: usize, minutes: i64 },
    Unplaced,
}

struct Resolver<'a, F> {
    calendar: &'a Calendar,
    whens: &'a [Option<Result<When, String>>],
    lookup: F,
    times: Vec<Option<Time>>,
    problems: Vec<Option<Problem>>,
    visiting: Vec<bool>,
    day_one: Option<Result<Time, Problem>>,
    days_only: bool,
    has_dates: bool,
    /// Set where a chain came back on itself, while unwinding back to that item: everything
    /// between is on the cycle.
    cycle_head: Option<usize>,
}

/// Places `whens[i]` for each item `i`. `lookup` finds the item a name in a `when` means.
pub fn resolve(calendar: &Calendar, whens: &[Option<Result<When, String>>], lookup: impl Fn(&str) -> Lookup) -> Timeline {
    let n = whens.len();
    let has_dates = whens.iter().any(|w| matches!(w, Some(Ok(When::Fixed(_)))));
    let mut r = Resolver {
        calendar,
        whens,
        lookup,
        times: vec![None; n],
        problems: vec![None; n],
        visiting: vec![false; n],
        day_one: None,
        days_only: false,
        has_dates,
        cycle_head: None,
    };
    for i in 0..n {
        r.time(i);
    }
    let keys = Keys::find(&mut r);
    let order = keys.order(&r);

    let mut items: Vec<Placed> = (0..n)
        .map(|i| {
            let time = r.times[i].unwrap_or(Time::Unplaced);
            Placed {
                time: match time {
                    Time::Exact(m) => Some(m),
                    _ => None,
                },
                position: None,
                loose: matches!(time, Time::Loose { .. }),
                problem: r.problems[i].clone(),
            }
        })
        .collect();
    for (position, &i) in order.iter().enumerate() {
        items[i].position = Some(position);
    }
    let day_one = match r.day_one {
        Some(Ok(Time::Exact(m))) => Some(Moment { minute: m.day() * MINUTES_PER_DAY, timed: false }),
        _ if r.days_only => Some(Moment { minute: 0, timed: false }),
        _ => None,
    };
    Timeline { order, items, days_only: r.days_only, day_one }
}

impl<F: Fn(&str) -> Lookup> Resolver<'_, F> {
    fn find(&self, name: &str) -> Result<usize, Problem> {
        match (self.lookup)(name) {
            Lookup::Found(i) => Ok(i),
            Lookup::Ambiguous => Err(Problem::Ambiguous(name.to_owned())),
            Lookup::Missing => Err(Problem::Missing(name.to_owned())),
        }
    }

    /// Item `i`'s time, following its chain. A chain that comes back to an item still being
    /// worked out is a cycle: every item on it is flagged, and whatever hangs off it waits.
    fn time(&mut self, i: usize) -> Time {
        if let Some(time) = self.times[i] {
            return time;
        }
        if self.visiting[i] {
            self.problems[i] = Some(Problem::Cycle);
            self.cycle_head = Some(i);
            return Time::Unplaced;
        }
        self.visiting[i] = true;
        let (time, problem) = match &self.whens[i] {
            None => (Time::Unplaced, None),
            Some(Err(message)) => (Time::Unplaced, Some(Problem::Unreadable(message.clone()))),
            Some(Ok(When::Fixed(moment))) => (Time::Exact(*moment), None),
            Some(Ok(When::Order { after, before })) => {
                let names = after.iter().chain(before.iter());
                match names.map(|name| self.find(name)).find_map(Result::err) {
                    Some(problem) => (Time::Unplaced, Some(problem)),
                    None => (Time::Loose { root: i, minutes: 0 }, None),
                }
            }
            Some(Ok(When::Relative { from, offset })) => {
                let (from, offset) = (from.clone(), *offset);
                match self.find(&from) {
                    Err(problem) => (Time::Unplaced, Some(problem)),
                    Ok(j) => self.measured_from(j, |cal, m| offset.apply(cal, m), offset.approximate_minutes(self.calendar)),
                }
            }
            Some(Ok(When::Day { day, time })) => {
                let since = (day - 1) * MINUTES_PER_DAY + time.map_or(0, |(h, m)| h * 60 + m);
                let timed = time.is_some();
                let at = move |m: Moment| Moment { minute: m.day() * MINUTES_PER_DAY + since, timed };
                match self.day_one() {
                    Ok(Time::Exact(start)) => (Time::Exact(at(start)), None),
                    Ok(Time::Loose { root, minutes }) => (Time::Loose { root, minutes: minutes + since }, None),
                    Ok(Time::Unplaced) => unreachable!("day_one is placed or a problem"),
                    Err(problem) => (Time::Unplaced, Some(problem)),
                }
            }
        };
        self.visiting[i] = false;
        if self.cycle_head == Some(i) {
            self.cycle_head = None;
        }
        // A cycle found below flagged this item already, and that verdict stands.
        if self.problems[i] == Some(Problem::Cycle) {
            self.times[i] = Some(Time::Unplaced);
            return Time::Unplaced;
        }
        self.times[i] = Some(time);
        if problem.is_some() {
            self.problems[i] = problem;
        }
        time
    }

    /// Something measured from item `j`.
    fn measured_from(
        &mut self,
        j: usize,
        exact: impl Fn(&Calendar, Moment) -> Moment,
        approximate: i64,
    ) -> (Time, Option<Problem>) {
        match self.time(j) {
            Time::Exact(m) => (Time::Exact(exact(self.calendar, m)), None),
            Time::Loose { root, minutes } => (Time::Loose { root, minutes: minutes + approximate }, None),
            Time::Unplaced if self.cycle_head.is_some() => (Time::Unplaced, Some(Problem::Cycle)),
            Time::Unplaced => (Time::Unplaced, Some(Problem::WaitsOn(j))),
        }
    }

    /// Where Day 1 starts.
    fn day_one(&mut self) -> Result<Time, Problem> {
        if let Some(known) = &self.day_one {
            return known.clone();
        }
        let found = match self.calendar.day_one.clone() {
            Some(DayOne::Date(moment)) => Ok(Time::Exact(moment)),
            Some(DayOne::Item(name)) => {
                let j = self.find(&name)?;
                match self.measured_from(j, |_, m| m, 0) {
                    (time, None) => Ok(time),
                    // A cycle depends on which item is asking, so isn't remembered.
                    (_, Some(problem)) => return Err(problem),
                }
            }
            None if self.has_dates => Err(Problem::NoDayOne),
            None => {
                self.days_only = true;
                Ok(Time::Exact(Moment { minute: 0, timed: false }))
            }
        };
        self.day_one = Some(found.clone());
        found
    }
}

/// A sort key for story order: roughly when, then a rank among things at that time. Each "after"
/// adds a big step and each "before" takes away a small one, so something before an item that
/// is after X still comes after X.
type Key = (i64, i64);

const AFTER: i64 = 1 << 20;

/// Sort keys for story order.
struct Keys {
    keys: Vec<Option<Key>>,
    visiting: Vec<bool>,
    /// As in `Resolver`, for orders that come back on themselves.
    cycle_head: Option<usize>,
    /// For each order-only item, what it has to come after and before (when that holds).
    edges: Vec<(usize, usize)>,
}

impl Keys {
    fn find<F: Fn(&str) -> Lookup>(r: &mut Resolver<'_, F>) -> Self {
        let n = r.whens.len();
        let mut keys = Self { keys: vec![None; n], visiting: vec![false; n], cycle_head: None, edges: Vec::new() };
        for i in 0..n {
            keys.key(r, i);
        }
        keys
    }

    fn key<F: Fn(&str) -> Lookup>(&mut self, r: &mut Resolver<'_, F>, i: usize) -> Option<Key> {
        if let Some(key) = self.keys[i] {
            return Some(key);
        }
        let key = match r.times[i]? {
            Time::Unplaced => return None,
            Time::Exact(m) => (m.minute, 0),
            Time::Loose { root, minutes } if root != i => {
                let Some((at, rank)) = self.key(r, root) else {
                    r.times[i] = Some(Time::Unplaced);
                    if self.cycle_head.is_some() {
                        r.problems[i] = Some(Problem::Cycle);
                    } else {
                        r.problems[i].get_or_insert(Problem::WaitsOn(root));
                    }
                    return None;
                };
                (at + minutes, rank)
            }
            Time::Loose { .. } => {
                if self.visiting[i] {
                    r.problems[i] = Some(Problem::Cycle);
                    self.cycle_head = Some(i);
                    return None;
                }
                self.visiting[i] = true;
                let key = self.order_key(r, i);
                self.visiting[i] = false;
                if self.cycle_head == Some(i) {
                    self.cycle_head = None;
                }
                if key.is_none() {
                    r.times[i] = Some(Time::Unplaced);
                }
                key?
            }
        };
        self.keys[i] = Some(key);
        Some(key)
    }

    /// The key of an order-only item, from what it's after and before.
    fn order_key<F: Fn(&str) -> Lookup>(&mut self, r: &mut Resolver<'_, F>, i: usize) -> Option<Key> {
        let Some(Ok(When::Order { after, before })) = &r.whens[i] else { unreachable!("only order-only items are roots") };
        let (after, before) = (after.clone(), before.clone());
        let mut anchor = |keys: &mut Self, name: Option<String>| -> Result<Option<(usize, Key)>, ()> {
            let Some(name) = name else { return Ok(None) };
            let j = r.find(&name).expect("checked when timing");
            match keys.key(r, j) {
                Some(key) => Ok(Some((j, key))),
                None => {
                    if keys.cycle_head.is_some() {
                        r.problems[i] = Some(Problem::Cycle);
                    } else if r.problems[i].is_none() {
                        r.problems[i] = Some(Problem::WaitsOn(j));
                    }
                    Err(())
                }
            }
        };
        let a = anchor(self, after).ok()?;
        let b = anchor(self, before).ok()?;
        if r.problems[i] == Some(Problem::Cycle) {
            return None;
        }
        let key = match (a, b) {
            (Some((a, ka)), None) => {
                self.edges.push((a, i));
                (ka.0, ka.1 + AFTER)
            }
            (None, Some((b, kb))) => {
                self.edges.push((i, b));
                (kb.0, kb.1 - 1)
            }
            (Some((a, ka)), Some((b, kb))) => {
                if ka >= kb {
                    r.problems[i] = Some(Problem::Contradiction);
                    self.edges.push((a, i));
                    (ka.0, ka.1 + AFTER)
                } else {
                    self.edges.push((a, i));
                    self.edges.push((i, b));
                    if ka.0 == kb.0 { (ka.0, ka.1 + (kb.1 - ka.1) / 2) } else { (ka.0 + (kb.0 - ka.0) / 2, 0) }
                }
            }
            (None, None) => unreachable!("an order-only when has after or before"),
        };
        Some(key)
    }

    /// Story order: by key, then the order things were given in, never putting an order-only
    /// item on the wrong side of what it's after or before.
    fn order<F>(&self, r: &Resolver<'_, F>) -> Vec<usize> {
        let n = self.keys.len();
        let placed = |i: usize| self.keys[i].is_some() && !matches!(r.times[i], Some(Time::Unplaced) | None);
        let mut waiting = vec![0usize; n];
        let mut next: Vec<Vec<usize>> = vec![Vec::new(); n];
        for &(a, b) in &self.edges {
            if placed(a) && placed(b) {
                waiting[b] += 1;
                next[a].push(b);
            }
        }
        let mut ready: BinaryHeap<Reverse<(Key, usize)>> =
            (0..n).filter(|&i| placed(i) && waiting[i] == 0).map(|i| Reverse((self.keys[i].unwrap(), i))).collect();
        let mut order = Vec::new();
        while let Some(Reverse((_, i))) = ready.pop() {
            order.push(i);
            for &j in &next[i] {
                waiting[j] -= 1;
                if waiting[j] == 0 {
                    ready.push(Reverse((self.keys[j].unwrap(), j)));
                }
            }
        }
        order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Items by title, each with its `when` as written (TOML), resolved in the real calendar
    /// unless `calendar` says otherwise.
    fn timeline(calendar: &Calendar, items: &[(&str, &str)]) -> (Timeline, Vec<String>) {
        let titles: Vec<String> = items.iter().map(|(t, _)| t.to_string()).collect();
        let whens: Vec<_> = items
            .iter()
            .map(|(_, when)| {
                let header = Header::parse(&format!("when = {when}")).ok();
                header.and_then(|h| When::from_header(&h, calendar))
            })
            .collect();
        let lookup = |name: &str| {
            let found: Vec<usize> = titles.iter().enumerate().filter(|(_, t)| t.eq_ignore_ascii_case(name)).map(|(i, _)| i).collect();
            match found[..] {
                [i] => Lookup::Found(i),
                [] => Lookup::Missing,
                _ => Lookup::Ambiguous,
            }
        };
        (resolve(calendar, &whens, lookup), titles)
    }

    fn story_order(t: &Timeline, titles: &[String]) -> Vec<String> {
        t.order.iter().map(|&i| titles[i].clone()).collect()
    }

    #[test]
    fn reads_every_kind_of_when() {
        let real = Calendar::real();
        let when = |toml: &str| When::from_header(&Header::parse(toml).unwrap(), &real);
        assert_eq!(when("when = \"1998-03-14\""), Some(Ok(When::Fixed(real.parse("1998-03-14").unwrap()))));
        assert_eq!(when("when = 1998-03-14T19:00:00"), Some(Ok(When::Fixed(real.parse("1998-03-14 19:00").unwrap()))));
        assert_eq!(when("when = \"Day 4, 08:00\""), Some(Ok(When::Day { day: 4, time: Some((8, 0)) })));
        assert_eq!(
            when("when = { from = \"The wedding\", offset = \"+3d\" }"),
            Some(Ok(When::Relative { from: "The wedding".into(), offset: Offset::days(3) }))
        );
        assert_eq!(
            when("[when]\nafter = \"The harbor\"\nbefore = \"The Drowning\""),
            Some(Ok(When::Order { after: Some("The harbor".into()), before: Some("The Drowning".into()) }))
        );
        assert_eq!(when("title = \"x\""), None);
        for bad in [
            "when = \"next Tuesday\"",
            "when = \"Day zero\"",
            "when = { offset = \"+3d\" }",
            "when = { from = \"A\", after = \"B\" }",
            "when = { at = \"A\" }",
            "when = { from = \"A\", offset = \"later\" }",
        ] {
            assert!(matches!(when(bad), Some(Err(_))), "{bad}");
        }
    }

    #[test]
    fn dates_and_offsets_put_things_in_story_order() {
        let (t, titles) = timeline(
            &Calendar::real(),
            &[
                ("The night market", "{ from = \"The harbor\", offset = \"+6h\" }"),
                ("The harbor", "\"1998-03-14 19:00\""),
                ("Childhood", "\"1979-06-01\""),
                ("The wedding", "{ from = \"The night market\", offset = \"+1y\" }"),
            ],
        );
        assert_eq!(story_order(&t, &titles), ["Childhood", "The harbor", "The night market", "The wedding"]);
        let real = Calendar::real();
        assert_eq!(t.label(&real, t.items[0].time.unwrap()), "15 March 1998, 01:00");
        assert_eq!(t.label(&real, t.items[3].time.unwrap()), "15 March 1999, 01:00");
        assert!(t.items.iter().all(|p| p.problem.is_none() && !p.loose));
        assert!(!t.days_only);
    }

    #[test]
    fn order_only_items_go_between_their_neighbours() {
        let (t, titles) = timeline(
            &Calendar::real(),
            &[
                ("Coda", "{ after = \"The storm\" }"),
                ("The storm", "\"1998-05-01\""),
                ("Interlude", "{ after = \"The harbor\", before = \"The storm\" }"),
                ("The harbor", "\"1998-03-14\""),
                ("Prologue", "{ before = \"The harbor\" }"),
                ("Aftermath", "{ from = \"Interlude\", offset = \"+2d\" }"),
                ("Later", "\"1998-06-01\""),
            ],
        );
        assert_eq!(story_order(&t, &titles), ["Prologue", "The harbor", "Interlude", "Aftermath", "The storm", "Coda", "Later"]);
        assert!(t.items[0].loose && t.items[5].loose && !t.items[1].loose);
        assert_eq!(t.items[5].time, None, "measured from a loose item, so loose too");
    }

    #[test]
    fn unplaced_items_wait_in_the_tray() {
        let (t, titles) = timeline(
            &Calendar::real(),
            &[
                ("Someday", "nothing"),
                ("The harbor", "\"1998-03-14\""),
                ("After someday", "{ from = \"Someday\", offset = \"+1d\" }"),
                ("Typo", "{ from = \"The harbour\" }"),
            ],
        );
        assert_eq!(story_order(&t, &titles), ["The harbor"]);
        assert_eq!(t.items[0], Placed::default());
        assert_eq!(t.items[2].problem, Some(Problem::WaitsOn(0)));
        assert_eq!(t.items[3].problem, Some(Problem::Missing("The harbour".into())));
    }

    #[test]
    fn cycles_are_flagged_and_what_hangs_off_them_waits() {
        let (t, titles) = timeline(
            &Calendar::real(),
            &[
                ("A", "{ from = \"B\", offset = \"+1d\" }"),
                ("B", "{ from = \"C\", offset = \"+1d\" }"),
                ("C", "{ from = \"A\", offset = \"-3d\" }"),
                ("D", "{ from = \"A\", offset = \"+1d\" }"),
                ("Self", "{ from = \"Self\" }"),
                ("E", "{ after = \"F\" }"),
                ("F", "{ after = \"E\" }"),
                ("Fine", "\"2001-01-01\""),
            ],
        );
        assert_eq!(story_order(&t, &titles), ["Fine"]);
        for i in [0, 1, 2, 4, 5, 6] {
            assert_eq!(t.items[i].problem, Some(Problem::Cycle), "{}", titles[i]);
        }
        assert_eq!(t.items[3].problem, Some(Problem::WaitsOn(0)));
    }

    #[test]
    fn a_long_cycle_is_one_cycle_and_its_neighbours_only_wait() {
        let (t, titles) = timeline(
            &Calendar::real(),
            &[
                ("Before", "{ after = \"A\" }"),
                ("A", "{ from = \"B\" }"),
                ("B", "{ from = \"C\" }"),
                ("C", "{ from = \"D\" }"),
                ("D", "{ from = \"A\" }"),
                ("Ordered", "{ after = \"Loop1\" }"),
                ("Loop1", "{ after = \"Loop2\" }"),
                ("Loop2", "{ before = \"Loop3\" }"),
                ("Loop3", "{ from = \"Loop1\", offset = \"+1d\" }"),
            ],
        );
        assert!(story_order(&t, &titles).is_empty());
        for i in [1, 2, 3, 4, 6, 7, 8] {
            assert_eq!(t.items[i].problem, Some(Problem::Cycle), "{}", titles[i]);
        }
        assert_eq!(t.items[0].problem, Some(Problem::WaitsOn(1)));
        assert_eq!(t.items[5].problem, Some(Problem::WaitsOn(6)));
    }

    #[test]
    fn chains_of_orders_keep_every_promise() {
        let (t, titles) = timeline(
            &Calendar::real(),
            &[
                ("Y", "\"1998-02-01\""),
                ("O2", "{ after = \"O1\", before = \"Y\" }"),
                ("O1", "{ after = \"X\" }"),
                ("X", "\"1998-01-01\""),
                ("O3", "{ before = \"O1\" }"),
                ("Z", "\"1998-01-01\""),
            ],
        );
        assert_eq!(story_order(&t, &titles), ["X", "Z", "O3", "O1", "O2", "Y"]);
        assert!(t.items.iter().all(|p| p.problem.is_none()));
    }

    #[test]
    fn impossible_orders_are_flagged_and_placed_by_after() {
        let (t, titles) = timeline(
            &Calendar::real(),
            &[
                ("Early", "\"1998-01-01\""),
                ("Late", "\"1999-01-01\""),
                ("Confused", "{ after = \"Late\", before = \"Early\" }"),
                ("Ambiguous", "{ after = \"twin\" }"),
                ("Twin", "\"1998-06-01\""),
                ("twin", "\"1998-07-01\""),
            ],
        );
        assert_eq!(story_order(&t, &titles), ["Early", "Twin", "twin", "Late", "Confused"]);
        assert_eq!(t.items[2].problem, Some(Problem::Contradiction));
        assert_eq!(t.items[3].problem, Some(Problem::Ambiguous("twin".into())));
    }

    #[test]
    fn days_count_from_day_one() {
        // No dates and no day_one: a timeline of days.
        let (t, titles) = timeline(
            &Calendar::real(),
            &[("Arrival", "\"Day 1\""), ("Market", "\"Day 3 19:00\""), ("Night", "{ from = \"Market\", offset = \"+6h\" }")],
        );
        assert!(t.days_only);
        assert_eq!(story_order(&t, &titles), ["Arrival", "Market", "Night"]);
        let real = Calendar::real();
        assert_eq!(t.label(&real, t.items[2].time.unwrap()), "Day 4, 01:00");

        // Day one named by date.
        let calendar = Calendar::from_settings("[calendar]\nday_one = \"1998-03-12\"").unwrap().unwrap();
        let (t, _) = timeline(&calendar, &[("Market", "\"Day 3 19:00\""), ("Harbor", "\"1998-03-13\"")]);
        assert_eq!(calendar.format(t.items[0].time.unwrap()), "14 March 1998, 19:00");
        assert_eq!(t.day_number(t.items[1].time.unwrap()), Some(2));

        // Day one named by a scene, which can itself be order-only.
        let calendar = Calendar::from_settings("[calendar]\nday_one = \"The wedding\"").unwrap().unwrap();
        let (t, titles) = timeline(
            &calendar,
            &[("The wedding", "\"1998-03-12 15:00\""), ("Honeymoon", "\"Day 2\""), ("Funeral", "\"1998-03-14\"")],
        );
        assert_eq!(story_order(&t, &titles), ["The wedding", "Honeymoon", "Funeral"]);
        assert_eq!(calendar.format(t.items[1].time.unwrap()), "13 March 1998");

        // Days among dates, with nothing to say where Day 1 is.
        let (t, _) = timeline(&Calendar::real(), &[("Market", "\"Day 3\""), ("Harbor", "\"1998-03-13\"")]);
        assert_eq!(t.items[0].problem, Some(Problem::NoDayOne));
        assert_eq!(t.items[0].position, None);
    }

    #[test]
    fn an_invented_calendar_orders_the_same_way() {
        let calendar = Calendar::from_settings(
            "[calendar]\nname = \"Reckoning\"\nmonths = [{ name = \"Thaw\", days = 30 }, { name = \"Bloom\", days = 31 }]\n\
             eras = [{ name = \"Before\", short = \"BD\", counts_down = true }, { name = \"After\", short = \"AD\" }]",
        )
        .unwrap()
        .unwrap();
        let (t, titles) = timeline(
            &calendar,
            &[("Now", "\"3 Thaw 412\""), ("The Drowning", "\"1 Bloom 1 BD\""), ("Then", "{ from = \"Now\", offset = \"-1y\" }")],
        );
        assert_eq!(story_order(&t, &titles), ["The Drowning", "Then", "Now"]);
        assert_eq!(calendar.format(t.items[2].time.unwrap()), "3 Thaw 411 AD");
    }
}
