//! Calendars (DESIGN §6): the real one, or one invented for a world. Every date becomes a
//! number of minutes from the calendar's start, so sorting and offsets are plain arithmetic
//! in any calendar.
//!
//! Both kinds are the same machinery: months with their lengths, a leap rule, eras and
//! weekdays. The real calendar is that machinery set up as the Gregorian one (proleptic,
//! counting from 1 January 1 AD, with 1 BC as year 0), so `14 March 1998`, `1998-03-14` and
//! `44 BC` all read.

use std::fmt;

use serde::Deserialize;

pub const MINUTES_PER_DAY: i64 = 24 * 60;

/// A point in a calendar, in minutes from its start. `timed` is false for a date without a
/// time of day, which is shown without one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Moment {
    pub minute: i64,
    pub timed: bool,
}

impl Moment {
    pub fn day(self) -> i64 {
        self.minute.div_euclid(MINUTES_PER_DAY)
    }
}

/// A date taken apart, as a calendar reads and writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Date {
    /// The year counted from the calendar's start: 1 is its first year, 0 the year before.
    pub year: i64,
    /// Index into the calendar's months.
    pub month: usize,
    /// From 1.
    pub day: i64,
    /// Hours and minutes, if the date has a time of day.
    pub time: Option<(i64, i64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Month {
    pub name: String,
    pub days: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Era {
    pub name: String,
    pub short: Option<String>,
    /// How many years it lasts; the last era has no end.
    pub years: Option<i64>,
    /// Counts back from the next era, like BC. Only the first era can.
    pub counts_down: bool,
}

/// Years divisible by `every` add `days` to `month`, except those divisible by `except_every`,
/// unless they're also divisible by `unless_every`. The real calendar's is 4, 100, 400.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leap {
    pub every: i64,
    pub month: usize,
    pub days: i64,
    pub except_every: Option<i64>,
    pub unless_every: Option<i64>,
}

/// Where "Day 1" is, for times written as `Day 4`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DayOne {
    Date(Moment),
    /// A scene or plot point, by name: Day 1 is the day it happens.
    Item(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calendar {
    /// `None` for the real calendar.
    pub name: Option<String>,
    pub months: Vec<Month>,
    pub eras: Vec<Era>,
    pub leap: Option<Leap>,
    pub weekdays: Vec<String>,
    /// The weekday of the calendar's very first day, as an index into `weekdays`.
    pub first_weekday: usize,
    pub day_one: Option<DayOne>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarError(pub String);

impl fmt::Display for CalendarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CalendarError {}

fn error<T>(message: impl Into<String>) -> Result<T, CalendarError> {
    Err(CalendarError(message.into()))
}

/// `[calendar]` in `project.toml` or `world.toml`, as written.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CalendarToml {
    name: Option<String>,
    #[serde(default)]
    months: Vec<MonthToml>,
    #[serde(default)]
    eras: Vec<EraToml>,
    leap: Option<LeapToml>,
    #[serde(default)]
    weekdays: Vec<String>,
    first_weekday: Option<String>,
    day_one: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MonthToml {
    name: String,
    days: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EraToml {
    name: String,
    short: Option<String>,
    years: Option<i64>,
    #[serde(default)]
    counts_down: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LeapToml {
    every: i64,
    month: String,
    #[serde(default = "one")]
    days: i64,
    except_every: Option<i64>,
    unless_every: Option<i64>,
}

fn one() -> i64 {
    1
}

#[derive(Deserialize)]
struct FileToml {
    calendar: Option<toml::Value>,
}

const REAL_MONTHS: [(&str, i64); 12] = [
    ("January", 31),
    ("February", 28),
    ("March", 31),
    ("April", 30),
    ("May", 31),
    ("June", 30),
    ("July", 31),
    ("August", 31),
    ("September", 30),
    ("October", 31),
    ("November", 30),
    ("December", 31),
];

const REAL_WEEKDAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

impl Calendar {
    /// The real (Gregorian) calendar.
    pub fn real() -> Self {
        Self {
            name: None,
            months: REAL_MONTHS.iter().map(|&(name, days)| Month { name: name.into(), days }).collect(),
            eras: vec![
                Era { name: "Before Christ".into(), short: Some("BC".into()), years: None, counts_down: true },
                Era { name: "Anno Domini".into(), short: Some("AD".into()), years: None, counts_down: false },
            ],
            leap: Some(Leap { every: 4, month: 1, days: 1, except_every: Some(100), unless_every: Some(400) }),
            weekdays: REAL_WEEKDAYS.iter().map(|&d| d.into()).collect(),
            // 1 January 1 AD, counted back from today's calendar, was a Monday.
            first_weekday: 0,
            day_one: None,
        }
    }

    pub fn is_real(&self) -> bool {
        self.name.is_none()
    }

    /// The `[calendar]` table of a `project.toml` or `world.toml`, if it has one. One with no
    /// `months` is the real calendar (it might only set `day_one`).
    pub fn from_settings(toml: &str) -> Result<Option<Self>, CalendarError> {
        let file: FileToml = toml::from_str(toml).map_err(|e| CalendarError(e.message().to_owned()))?;
        let Some(table) = file.calendar else { return Ok(None) };
        let raw: CalendarToml =
            table.try_into().map_err(|e: toml::de::Error| CalendarError(format!("[calendar]: {}", e.message())))?;
        Self::from_toml(raw).map(Some)
    }

    fn from_toml(raw: CalendarToml) -> Result<Self, CalendarError> {
        let mut calendar = if raw.months.is_empty() {
            if raw.name.is_some() || !raw.eras.is_empty() || raw.leap.is_some() || !raw.weekdays.is_empty() {
                return error("an invented calendar needs its months");
            }
            Self::real()
        } else {
            let months: Vec<Month> = raw.months.into_iter().map(|m| Month { name: m.name, days: m.days }).collect();
            if let Some(bad) = months.iter().find(|m| m.days < 1) {
                return error(format!("{} needs at least one day", bad.name));
            }
            let eras: Vec<Era> = raw
                .eras
                .into_iter()
                .map(|e| Era { name: e.name, short: e.short, years: e.years, counts_down: e.counts_down })
                .collect();
            for (i, era) in eras.iter().enumerate() {
                if era.counts_down && i != 0 {
                    return error(format!("only the first era can count down ({})", era.name));
                }
                if era.counts_down && era.years.is_some() {
                    return error(format!("{} counts down, so it can't have a number of years", era.name));
                }
                let last = i + 1 == eras.len();
                if !era.counts_down && !last && era.years.is_none_or(|y| y < 1) {
                    return error(format!("{} needs its number of years, since another era follows it", era.name));
                }
            }
            if eras.len() == 1 && eras[0].counts_down {
                return error("an era that counts down needs another era after it");
            }
            let leap = match raw.leap {
                None => None,
                Some(l) => {
                    let Some(month) = find_month(&months, &l.month) else {
                        return error(format!("the leap month {} isn't one of the months", l.month));
                    };
                    if l.every < 1 || l.except_every.is_some_and(|e| e < 1) || l.unless_every.is_some_and(|e| e < 1) {
                        return error("leap years need a positive \"every\"");
                    }
                    Some(Leap { every: l.every, month, days: l.days, except_every: l.except_every, unless_every: l.unless_every })
                }
            };
            Self { name: Some(raw.name.unwrap_or_default()), months, eras, leap, weekdays: raw.weekdays, first_weekday: 0, day_one: None }
        };
        if let Some(first) = raw.first_weekday {
            let Some(i) = calendar.weekdays.iter().position(|d| same_word(d, &first)) else {
                return error(format!("{first} isn't one of the weekdays"));
            };
            calendar.first_weekday = i;
        }
        if let Some(day_one) = raw.day_one {
            calendar.day_one = Some(match calendar.parse(&day_one) {
                Ok(moment) => DayOne::Date(moment),
                Err(_) => DayOne::Item(day_one),
            });
        }
        Ok(calendar)
    }

    pub fn is_leap(&self, year: i64) -> bool {
        let Some(leap) = &self.leap else { return false };
        let divides = |n: i64| year.rem_euclid(n) == 0;
        divides(leap.every)
            && !(leap.except_every.is_some_and(divides) && !leap.unless_every.is_some_and(divides))
    }

    /// Leap years in `[1, year)`, or minus those in `[year, 1)` for years before the first.
    fn leaps_before(&self, year: i64) -> i64 {
        let Some(leap) = &self.leap else { return 0 };
        let multiples = |n: i64| (year - 1).div_euclid(n);
        let mut count = multiples(leap.every);
        if let Some(except) = leap.except_every {
            count -= multiples(except);
            if let Some(unless) = leap.unless_every {
                count += multiples(unless);
            }
        }
        count * leap.days
    }

    fn common_year(&self) -> i64 {
        self.months.iter().map(|m| m.days).sum()
    }

    pub fn year_length(&self, year: i64) -> i64 {
        self.common_year() + if self.is_leap(year) { self.leap.as_ref().map_or(0, |l| l.days) } else { 0 }
    }

    pub fn month_length(&self, year: i64, month: usize) -> i64 {
        let extra = match &self.leap {
            Some(leap) if leap.month == month && self.is_leap(year) => leap.days,
            _ => 0,
        };
        self.months[month].days + extra
    }

    /// Days from the calendar's start to the first day of `year`.
    fn days_before_year(&self, year: i64) -> i64 {
        (year - 1) * self.common_year() + self.leaps_before(year)
    }

    fn year_of_day(&self, day: i64) -> i64 {
        let average = self.common_year() as f64 + self.leap.as_ref().map_or(0.0, |l| l.days as f64 / l.every as f64);
        let mut year = (day as f64 / average).floor() as i64 + 1;
        while self.days_before_year(year) > day {
            year -= 1;
        }
        while self.days_before_year(year + 1) <= day {
            year += 1;
        }
        year
    }

    pub fn to_moment(&self, date: &Date) -> Moment {
        let months: i64 = (0..date.month).map(|m| self.month_length(date.year, m)).sum();
        let day = self.days_before_year(date.year) + months + date.day - 1;
        let (h, m) = date.time.unwrap_or((0, 0));
        Moment { minute: day * MINUTES_PER_DAY + h * 60 + m, timed: date.time.is_some() }
    }

    pub fn to_date(&self, moment: Moment) -> Date {
        let day = moment.day();
        let year = self.year_of_day(day);
        let mut rest = day - self.days_before_year(year);
        let mut month = 0;
        while month + 1 < self.months.len() && rest >= self.month_length(year, month) {
            rest -= self.month_length(year, month);
            month += 1;
        }
        let minute = moment.minute.rem_euclid(MINUTES_PER_DAY);
        Date { year, month, day: rest + 1, time: moment.timed.then_some((minute / 60, minute % 60)) }
    }

    pub fn weekday(&self, moment: Moment) -> Option<&str> {
        let n = self.weekdays.len() as i64;
        (n > 0).then(|| self.weekdays[(moment.day() + self.first_weekday as i64).rem_euclid(n) as usize].as_str())
    }

    /// The era a year falls in, and the year as counted within it.
    pub fn era_of(&self, year: i64) -> Option<(usize, i64)> {
        if self.eras.is_empty() {
            return None;
        }
        if year < 1 && self.eras[0].counts_down {
            return Some((0, 1 - year));
        }
        let mut start = 1;
        for (i, era) in self.eras.iter().enumerate().filter(|(_, e)| !e.counts_down) {
            match era.years {
                Some(years) if year >= start + years && i + 1 < self.eras.len() => start += years,
                _ => return Some((i, year - start + 1)),
            }
        }
        unreachable!("the last era has no end")
    }

    /// The calendar's own year from a year counted within an era.
    fn year_in(&self, era: usize, year: i64) -> i64 {
        if self.eras[era].counts_down {
            return 1 - year;
        }
        let before: i64 = self.eras[..era].iter().filter(|e| !e.counts_down).filter_map(|e| e.years).sum();
        before + year
    }

    /// The era a year written without one is in: the last one, the era the story is most
    /// likely set in.
    fn default_era(&self) -> Option<usize> {
        self.eras.len().checked_sub(1)
    }

    /// Reads a date: `3 Thaw 412 AD`, `3 Thaw 412`, `Thaw 412` (its first day), `412 AD`, each
    /// with an optional time (`19:00`) after. The real calendar also reads `1998-03-14`,
    /// `1998-03-14 19:00`, `1998-03` and `March 14, 1998`. Month and era names can be written
    /// in any case; a month can be shortened to its first three letters.
    pub fn parse(&self, text: &str) -> Result<Moment, CalendarError> {
        let text = text.trim();
        if self.is_real()
            && let Some(moment) = self.parse_iso(text)
        {
            return Ok(moment);
        }
        let cleaned = text.replace(',', " ");
        let mut words: Vec<&str> = cleaned.split_whitespace().collect();
        let mut time = None;
        if let Some(last) = words.last()
            && let Some(t) = parse_time(last)
        {
            time = Some(t);
            words.pop();
        }
        // The era, at the end: by its short name or its full name.
        let mut era = None;
        'eras: for (i, e) in self.eras.iter().enumerate() {
            let names = std::iter::once(e.name.as_str()).chain(e.short.as_deref());
            for name in names {
                let n = name.split_whitespace().count();
                if n <= words.len() && same_word(&words[words.len() - n..].join(" "), name) {
                    words.truncate(words.len() - n);
                    era = Some(i);
                    break 'eras;
                }
            }
        }
        let bad = || CalendarError(format!("{text:?} isn't a date in this calendar"));
        let year_word = words.pop().ok_or_else(bad)?;
        let year: i64 = year_word.parse().map_err(|_| bad())?;
        let (day, month_words) = match words.first().and_then(|w| w.parse::<i64>().ok()) {
            Some(day) => (Some(day), &words[1..]),
            // "March 14 1998": the day after the month.
            None => match words.last().and_then(|w| w.parse::<i64>().ok()) {
                Some(day) => (Some(day), &words[..words.len() - 1]),
                None => (None, &words[..]),
            },
        };
        let month = if month_words.is_empty() {
            if day.is_some() {
                return Err(bad());
            }
            0
        } else {
            find_month(&self.months, &month_words.join(" ")).ok_or_else(bad)?
        };
        let year = match era.or_else(|| self.default_era()) {
            Some(era) => {
                if year < 1 {
                    return Err(bad());
                }
                self.year_in(era, year)
            }
            None => year,
        };
        let day = day.unwrap_or(1);
        if day < 1 || day > self.month_length(year, month) {
            return error(format!("{} has no day {day}", self.months[month].name));
        }
        Ok(self.to_moment(&Date { year, month, day, time }))
    }

    /// `1998-03-14`, `1998-03-14 19:00`, `1998-03-14T19:00:00`, `1998-03` or `-0043-03-15`.
    fn parse_iso(&self, text: &str) -> Option<Moment> {
        let (date, time) = match text.split_once(['T', ' ']) {
            Some((date, time)) => (date, Some(parse_time(time.trim())?)),
            None => (text, None),
        };
        let (sign, date) = match date.strip_prefix('-') {
            Some(rest) => (-1, rest),
            None => (1, date),
        };
        let parts: Vec<&str> = date.split('-').collect();
        if parts.len() < 2 || parts.len() > 3 || parts[0].len() < 4 {
            return None;
        }
        let number = |s: &str| s.chars().all(|c| c.is_ascii_digit()).then(|| s.parse::<i64>().ok()).flatten();
        let year = sign * number(parts[0])?;
        let month = number(parts[1])?;
        let day = parts.get(2).map_or(Some(1), |d| number(d))?;
        if !(1..=12).contains(&month) || day < 1 || day > self.month_length(year, month as usize - 1) {
            return None;
        }
        Some(self.to_moment(&Date { year, month: month as usize - 1, day, time }))
    }

    /// A moment in words: `14 March 1998`, `3 Thaw 412 AD, 19:00`, `44 BC`… The real
    /// calendar names an era only before AD.
    pub fn format(&self, moment: Moment) -> String {
        let date = self.to_date(moment);
        let mut text = format!("{} {} {}", date.day, self.months[date.month].name, self.year_label(date.year));
        if let Some((h, m)) = date.time {
            text.push_str(&format!(", {h:02}:{m:02}"));
        }
        text
    }

    /// A year with its era: `412 AD`, `44 BC`, `1998`.
    pub fn year_label(&self, year: i64) -> String {
        match self.era_of(year) {
            None => year.to_string(),
            Some((era, n)) => {
                let e = &self.eras[era];
                if self.is_real() && !e.counts_down {
                    n.to_string()
                } else {
                    format!("{n} {}", e.short.as_deref().unwrap_or(&e.name))
                }
            }
        }
    }

    /// Moves a moment by whole years and months, keeping its day of the month where it can
    /// (31 January plus a month is the last day of February).
    pub fn add_months(&self, moment: Moment, months: i64) -> Moment {
        if months == 0 {
            return moment;
        }
        let date = self.to_date(moment);
        let n = self.months.len() as i64;
        let total = date.year * n + date.month as i64 + months;
        let (year, month) = (total.div_euclid(n), total.rem_euclid(n) as usize);
        let day = date.day.min(self.month_length(year, month));
        let mut moved = self.to_moment(&Date { year, month, day, time: date.time });
        // to_moment drops the time when there was none; keep the minutes either way.
        moved.minute += moment.minute.rem_euclid(MINUTES_PER_DAY) - moved.minute.rem_euclid(MINUTES_PER_DAY);
        moved.timed = moment.timed;
        moved
    }

    /// The average length of a month, for ordering things whose dates aren't known.
    pub fn average_month_minutes(&self) -> i64 {
        let leap = self.leap.as_ref().map_or(0.0, |l| l.days as f64 / l.every as f64);
        (((self.common_year() as f64 + leap) / self.months.len() as f64) * MINUTES_PER_DAY as f64) as i64
    }
}

impl Default for Calendar {
    fn default() -> Self {
        Self::real()
    }
}

fn same_word(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// A month by its name, or by its first three letters when that's unambiguous.
fn find_month(months: &[Month], name: &str) -> Option<usize> {
    let name = name.trim().trim_end_matches('.');
    if let Some(i) = months.iter().position(|m| same_word(&m.name, name)) {
        return Some(i);
    }
    if name.chars().count() < 3 {
        return None;
    }
    let lower = name.to_lowercase();
    let mut found = months.iter().enumerate().filter(|(_, m)| m.name.to_lowercase().starts_with(&lower));
    match (found.next(), found.next()) {
        (Some((i, _)), None) => Some(i),
        _ => None,
    }
}

/// `19:00`, `7:05` or `19:00:00` (seconds are dropped).
fn parse_time(text: &str) -> Option<(i64, i64)> {
    let mut parts = text.split(':');
    let h: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    if let Some(s) = parts.next() {
        s.parse::<f64>().ok()?;
    }
    if parts.next().is_some() || !(0..24).contains(&h) || !(0..60).contains(&m) {
        return None;
    }
    Some((h, m))
}

/// A distance in story time: `+3d`, `-2h`, `+1d 6h`, `+10y`, `+2 months`. Years and months
/// follow the calendar (a month later is the same day next month); the rest is minutes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Offset {
    pub years: i64,
    pub months: i64,
    pub minutes: i64,
}

impl Offset {
    pub fn days(days: i64) -> Self {
        Self { minutes: days * MINUTES_PER_DAY, ..Self::default() }
    }

    /// Whether it's a matter of hours or minutes, so a time of day means something.
    pub fn has_time(&self) -> bool {
        self.minutes.rem_euclid(MINUTES_PER_DAY) != 0
    }

    pub fn parse(text: &str) -> Result<Self, CalendarError> {
        let bad = || CalendarError(format!("{text:?} isn't an offset like +3d, -2h or +1y 2mo"));
        let text = text.trim();
        let (sign, rest) = match text.chars().next() {
            Some('+') => (1, &text[1..]),
            Some('-' | '−') => (-1, text.trim_start_matches(['-', '−'])),
            _ => (1, text),
        };
        let mut offset = Self::default();
        let mut chars = rest.trim().chars().peekable();
        let mut any = false;
        loop {
            while chars.next_if(|c| c.is_whitespace() || *c == ',').is_some() {}
            if chars.peek().is_none() {
                break;
            }
            let mut digits = String::new();
            while let Some(c) = chars.next_if(char::is_ascii_digit) {
                digits.push(c);
            }
            while chars.next_if(|c| c.is_whitespace()).is_some() {}
            let mut unit = String::new();
            while let Some(c) = chars.next_if(|c| c.is_alphabetic()) {
                unit.push(c.to_ascii_lowercase());
            }
            let n: i64 = digits.parse().map_err(|_| bad())?;
            match unit.as_str() {
                "y" | "yr" | "yrs" | "year" | "years" => offset.years += n,
                "mo" | "mos" | "month" | "months" => offset.months += n,
                "w" | "wk" | "wks" | "week" | "weeks" => offset.minutes += n * 7 * MINUTES_PER_DAY,
                "d" | "day" | "days" => offset.minutes += n * MINUTES_PER_DAY,
                "h" | "hr" | "hrs" | "hour" | "hours" => offset.minutes += n * 60,
                "m" | "min" | "mins" | "minute" | "minutes" => offset.minutes += n,
                _ => return Err(bad()),
            }
            any = true;
        }
        if !any {
            return Err(bad());
        }
        Ok(Self { years: sign * offset.years, months: sign * offset.months, minutes: sign * offset.minutes })
    }

    /// The moment this far from `moment`. Years and months are the calendar's own.
    pub fn apply(&self, calendar: &Calendar, moment: Moment) -> Moment {
        let months = self.years * calendar.months.len() as i64 + self.months;
        let moved = calendar.add_months(moment, months);
        Moment { minute: moved.minute + self.minutes, timed: moment.timed }
    }

    /// Roughly how long it is, for ordering things that have no date.
    pub fn approximate_minutes(&self, calendar: &Calendar) -> i64 {
        (self.years * calendar.months.len() as i64 + self.months) * calendar.average_month_minutes() + self.minutes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glass_coast() -> Calendar {
        Calendar::from_settings(
            r#"
name = "The Glass Coast"

[calendar]
name = "Reckoning of the Glass Coast"
months = [
  { name = "Thaw", days = 30 },
  { name = "Bloom", days = 31 },
  { name = "High Sun", days = 30 },
  { name = "Ember", days = 29 },
]
eras = [
  { name = "Before the Drowning", short = "BD", counts_down = true },
  { name = "After the Drowning", short = "AD" },
]
leap = { every = 3, month = "Ember" }
weekdays = ["Firstday", "Midday", "Lastday"]
"#,
        )
        .unwrap()
        .unwrap()
    }

    #[test]
    fn the_real_calendar_counts_days_like_everyone_else() {
        let real = Calendar::real();
        // 719,162 days from 1 January 1 AD to 1 January 1970.
        assert_eq!(real.parse("1970-01-01").unwrap().day(), 719_162);
        assert_eq!(real.parse("2000-03-01").unwrap().day() - real.parse("2000-02-28").unwrap().day(), 2);
        assert_eq!(real.parse("1900-03-01").unwrap().day() - real.parse("1900-02-28").unwrap().day(), 1);
        assert_eq!(real.weekday(real.parse("2026-10-08").unwrap()), Some("Thursday"));
        assert_eq!(real.weekday(real.parse("1 January 1").unwrap()), Some("Monday"));
    }

    #[test]
    fn the_real_calendar_reads_dates_written_several_ways() {
        let real = Calendar::real();
        let expected = real.parse("1998-03-14 19:00").unwrap();
        assert!(expected.timed);
        for text in ["14 March 1998 19:00", "March 14, 1998 19:00", "14 Mar 1998, 19:00", "1998-03-14T19:00:00"] {
            assert_eq!(real.parse(text).unwrap(), expected, "{text}");
        }
        assert!(!real.parse("1998-03-14").unwrap().timed);
        assert_eq!(real.parse("March 1998").unwrap(), real.parse("1998-03-01").unwrap());
        assert_eq!(real.parse("1998-03").unwrap(), real.parse("1998-03-01").unwrap());
        assert_eq!(real.parse("15 March 44 BC").unwrap(), real.parse("-0043-03-15").unwrap());
        for bad in ["30 February 2001", "1998-13-01", "yesterday", "14 Smarch 1998", "25:00"] {
            assert!(real.parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn dates_read_back_as_they_were_written() {
        let real = Calendar::real();
        assert_eq!(real.format(real.parse("1998-03-14 19:05").unwrap()), "14 March 1998, 19:05");
        assert_eq!(real.format(real.parse("2000-02-29").unwrap()), "29 February 2000");
        assert_eq!(real.format(real.parse("15 March 44 BC").unwrap()), "15 March 44 BC");
        assert_eq!(real.format(real.parse("31 December 1 BC").unwrap()), "31 December 1 BC");
        for day in [-800_000, -1, 0, 1, 59, 365, 719_162, 740_000] {
            let moment = Moment { minute: day * MINUTES_PER_DAY, timed: false };
            assert_eq!(real.to_moment(&real.to_date(moment)), moment, "day {day}");
        }
    }

    #[test]
    fn an_invented_calendar_has_its_own_months_eras_and_leap_years() {
        let c = glass_coast();
        assert_eq!(c.name.as_deref(), Some("Reckoning of the Glass Coast"));
        assert_eq!(c.year_length(1), 120);
        assert_eq!(c.year_length(3), 121);
        assert_eq!(c.month_length(3, 3), 30);
        let day = c.parse("3 Thaw 412 AD").unwrap();
        assert_eq!(c.format(day), "3 Thaw 412 AD");
        assert_eq!(c.parse("3 thaw 412").unwrap(), day, "the last era is the default");
        assert_eq!(c.format(c.parse("29 high sun 2 BD 06:30").unwrap()), "29 High Sun 2 BD, 06:30");
        assert_eq!(c.parse("1 Thaw 1 AD").unwrap().minute, 0);
        assert_eq!(c.parse("30 Ember 1 BD").unwrap().day(), -1, "1 BD is the year just before");
        assert!(c.parse("30 Ember 4 AD").is_err(), "4 isn't a leap year");
        assert!(c.parse("30 Ember 3 AD").is_ok());
        assert_eq!(c.weekday(day), c.weekday(Moment { minute: day.minute + 3 * MINUTES_PER_DAY, timed: false }));
        for d in -400..400 {
            let moment = Moment { minute: d * MINUTES_PER_DAY, timed: false };
            assert_eq!(c.to_moment(&c.to_date(moment)), moment, "day {d}");
        }
    }

    #[test]
    fn eras_follow_one_another() {
        let c = Calendar::from_settings(
            r#"
[calendar]
name = "Ages"
months = [{ name = "Long", days = 100 }]
eras = [{ name = "First Age", years = 500 }, { name = "Second Age", short = "S.A." }]
"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(c.era_of(500), Some((0, 500)));
        assert_eq!(c.era_of(501), Some((1, 1)));
        assert_eq!(c.parse("1 Long 1 S.A.").unwrap(), c.parse("1 Long 501 First Age").unwrap());
        assert_eq!(c.format(c.parse("Long 3 First Age").unwrap()), "1 Long 3 First Age");
    }

    #[test]
    fn mistakes_in_a_calendar_are_explained() {
        let bad = |toml: &str| Calendar::from_settings(toml).unwrap_err().0;
        assert!(bad("[calendar]\nname = \"X\"").contains("months"));
        assert!(bad("[calendar]\nmonths = [{ name = \"A\", days = 0 }]").contains("A needs at least one day"));
        assert!(bad("[calendar]\nmonths = [{ name = \"A\", days = 3 }]\nleap = { every = 4, month = \"B\" }").contains("B"));
        assert!(
            bad("[calendar]\nmonths = [{ name = \"A\", days = 3 }]\neras = [{ name = \"One\" }, { name = \"Two\" }]")
                .contains("One needs its number of years")
        );
        assert!(bad("[calendar]\nmonths = [{ name = \"A\", days = 3 }]\nmoons = 2").contains("moons"));
        assert_eq!(Calendar::from_settings("title = \"Tidewater\"").unwrap(), None);
    }

    #[test]
    fn day_one_is_a_date_or_the_name_of_something_in_the_story() {
        let c = Calendar::from_settings("[calendar]\nday_one = \"1998-03-12\"").unwrap().unwrap();
        assert!(c.is_real());
        assert_eq!(c.day_one, Some(DayOne::Date(Calendar::real().parse("1998-03-12").unwrap())));
        let c = Calendar::from_settings("[calendar]\nday_one = \"The wedding\"").unwrap().unwrap();
        assert_eq!(c.day_one, Some(DayOne::Item("The wedding".into())));
    }

    #[test]
    fn offsets_read_in_several_spellings() {
        let day = MINUTES_PER_DAY;
        let o = |years, months, minutes| Offset { years, months, minutes };
        assert_eq!(Offset::parse("+3d").unwrap(), o(0, 0, 3 * day));
        assert_eq!(Offset::parse("-2h").unwrap(), o(0, 0, -120));
        assert_eq!(Offset::parse("+1d 6h").unwrap(), o(0, 0, day + 360));
        assert_eq!(Offset::parse("10 years").unwrap(), o(10, 0, 0));
        assert_eq!(Offset::parse("-1y, 2mo").unwrap(), o(-1, -2, 0));
        assert_eq!(Offset::parse("+2 weeks 30m").unwrap(), o(0, 0, 14 * day + 30));
        for bad in ["", "+", "soon", "+3 fortnights", "d"] {
            assert!(Offset::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn offsets_follow_the_calendar() {
        let real = Calendar::real();
        let at = |s: &str| real.parse(s).unwrap();
        let add = |s: &str, o: &str| real.format(Offset::parse(o).unwrap().apply(&real, at(s)));
        assert_eq!(add("31 January 2001", "+1mo"), "28 February 2001");
        assert_eq!(add("29 February 2000", "+1y"), "28 February 2001");
        assert_eq!(add("1998-03-14 19:00", "+6h"), "15 March 1998, 01:00");
        assert_eq!(add("1998-03-14 19:00", "-1y 2d"), "12 March 1997, 19:00");
        assert_eq!(add("1998-03-14", "+3d"), "17 March 1998");
        let c = glass_coast();
        let moved = Offset::parse("+1y").unwrap().apply(&c, c.parse("3 Bloom 412").unwrap());
        assert_eq!(c.format(moved), "3 Bloom 413 AD", "a year is the calendar's own year");
    }
}
