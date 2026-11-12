use std::sync::LazyLock;

use isotrail_bundle::Units;

use crate::DateFormat;

/// Every word the overlays draw, and how they write numbers, times and dates
#[derive(Clone, Debug, PartialEq)]
pub struct Labels {
    pub distance: String,
    pub gain: String,
    pub loss: String,
    pub time: String,
    pub elapsed: String,
    pub high_point: String,
    pub low_point: String,
    pub speed: String,
    pub pace: String,
    pub km: String,
    pub mi: String,
    pub m: String,
    pub ft: String,
    pub kmh: String,
    pub mph: String,
    /// `{pace}` and `{unit}`
    pub pace_value: String,
    /// `{m}`, under an hour
    pub minutes: String,
    /// `{h}`, `{m}` and `{mm}`, padded to two digits
    pub hours: String,
    /// `{d}` and `{h}`, for elapsed times of a day or more
    pub days: String,
    pub months: [String; 12],
    pub months_short: [String; 12],
    /// Monday first
    pub weekdays: [String; 7],
    pub weekdays_short: [String; 7],
    /// The date formats, from [`DATE_PLACEHOLDERS`]
    pub date_long: String,
    pub date_short: String,
    pub date_iso: String,
    pub decimal: String,
}

pub const DATE_PLACEHOLDERS: [&str; 9] = [
    "weekday",
    "weekday-short",
    "day",
    "dd",
    "month",
    "month-short",
    "mm",
    "year",
    "yyyy",
];

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

impl Default for Labels {
    fn default() -> Labels {
        Labels {
            distance: "distance".into(),
            gain: "gain".into(),
            loss: "loss".into(),
            time: "time".into(),
            elapsed: "elapsed".into(),
            high_point: "high point".into(),
            low_point: "low point".into(),
            speed: "speed".into(),
            pace: "pace".into(),
            km: "km".into(),
            mi: "mi".into(),
            m: "m".into(),
            ft: "ft".into(),
            kmh: "km/h".into(),
            mph: "mph".into(),
            pace_value: "{pace} /{unit}".into(),
            minutes: "{m}m".into(),
            hours: "{h}h {mm}m".into(),
            days: "{d}d {h}h".into(),
            months: MONTHS.map(String::from),
            months_short: MONTHS.map(|word| word[..3].to_string()),
            weekdays: WEEKDAYS.map(String::from),
            weekdays_short: WEEKDAYS.map(|word| word[..3].to_string()),
            date_long: "{weekday} {day} {month} {year}".into(),
            date_short: "{day} {month-short} {year}".into(),
            date_iso: "{yyyy}-{mm}-{dd}".into(),
            decimal: ".".into(),
        }
    }
}

static BUILT_IN: LazyLock<Labels> = LazyLock::new(Labels::default);

impl Labels {
    pub fn built_in() -> &'static Labels {
        &BUILT_IN
    }

    pub fn distance_unit(&self, units: Units) -> &str {
        match units {
            Units::Metric => &self.km,
            Units::Imperial => &self.mi,
        }
    }

    pub fn elevation_unit(&self, units: Units) -> &str {
        match units {
            Units::Metric => &self.m,
            Units::Imperial => &self.ft,
        }
    }

    pub fn speed_unit(&self, units: Units) -> &str {
        match units {
            Units::Metric => &self.kmh,
            Units::Imperial => &self.mph,
        }
    }

    /// `value` with `decimals` places and this decimal separator
    pub fn number(&self, value: f64, decimals: usize) -> String {
        let text = format!("{value:.decimals$}");
        if self.decimal == "." {
            text
        } else {
            text.replace('.', &self.decimal)
        }
    }

    /// Rounded to minutes
    pub fn duration(&self, seconds: i64) -> String {
        let minutes = (seconds.max(0) + 30) / 60;
        if minutes < 60 {
            fill(&self.minutes, &[("m", minutes.to_string())])
        } else {
            let (h, m) = (minutes / 60, minutes % 60);
            fill(
                &self.hours,
                &[
                    ("h", h.to_string()),
                    ("m", m.to_string()),
                    ("mm", format!("{m:02}")),
                ],
            )
        }
    }

    /// Days and hours once it reaches a day, as multi-day hikes span nights
    pub fn elapsed(&self, seconds: i64) -> String {
        if seconds < 86_400 {
            return self.duration(seconds);
        }
        let hours = (seconds + 1800) / 3600;
        fill(
            &self.days,
            &[
                ("d", (hours / 24).to_string()),
                ("h", (hours % 24).to_string()),
            ],
        )
    }

    /// A `YYYY-MM-DD` date in `format`, or `None` if it isn't a real date
    pub fn date(&self, iso: &str, format: DateFormat) -> Option<String> {
        let mut parts = iso.splitn(3, '-').map(str::parse::<u32>);
        let (year, month, day) = (
            parts.next()?.ok()?,
            parts.next()?.ok()?,
            parts.next()?.ok()?,
        );
        if year == 0 {
            return None;
        }
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days_in_month = match month {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            1..=12 => 31,
            _ => return None,
        };
        if day == 0 || day > days_in_month {
            return None;
        }
        // Sakamoto's method, Sunday 0, turned to Monday 0
        const OFFSETS: [u32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let y = if month < 3 { year - 1 } else { year };
        let sunday_first = (y + y / 4 - y / 100 + y / 400 + OFFSETS[month as usize - 1] + day) % 7;
        let weekday = (sunday_first as usize + 6) % 7;
        let (month_index, pattern) = (
            month as usize - 1,
            match format {
                DateFormat::Long => &self.date_long,
                DateFormat::Short => &self.date_short,
                DateFormat::Iso => &self.date_iso,
            },
        );
        Some(fill(
            pattern,
            &[
                ("weekday", self.weekdays[weekday].clone()),
                ("weekday-short", self.weekdays_short[weekday].clone()),
                ("day", day.to_string()),
                ("dd", format!("{day:02}")),
                ("month", self.months[month_index].clone()),
                ("month-short", self.months_short[month_index].clone()),
                ("mm", format!("{month:02}")),
                ("year", year.to_string()),
                ("yyyy", format!("{year:04}")),
            ],
        ))
    }
}

/// `pattern` with each `{name}` replaced by its value; other text, and
/// braces around unknown names, stay as they are
pub(crate) fn fill(pattern: &str, values: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut rest = pattern;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let value = after.find('}').and_then(|close| {
            let name = &after[..close];
            values
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (value, close))
        });
        match value {
            Some((value, close)) => {
                out.push_str(value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The `{name}`s in `pattern`
pub(crate) fn placeholders(pattern: &str) -> impl Iterator<Item = &str> {
    pattern.split('{').skip(1).filter_map(|part| {
        let close = part.find('}')?;
        Some(&part[..close])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn polish() -> Labels {
        fn names<const N: usize>(text: &str) -> [String; N] {
            let names: Vec<String> = text.split(' ').map(str::to_string).collect();
            names.try_into().unwrap()
        }
        Labels {
            distance: "dystans".into(),
            hours: "{h} godz. {mm} min".into(),
            months: names(
                "stycznia lutego marca kwietnia maja czerwca lipca sierpnia września \
                 października listopada grudnia",
            ),
            months_short: names("sty lut mar kwi maj cze lip sie wrz paź lis gru"),
            weekdays: names("poniedziałek wtorek środa czwartek piątek sobota niedziela"),
            date_long: "{weekday}, {day} {month} {year}".into(),
            decimal: ",".into(),
            ..Labels::default()
        }
    }

    #[test]
    fn the_built_in_words_are_english() {
        let labels = Labels::built_in();
        assert_eq!(
            labels.date("2025-06-14", DateFormat::Long).as_deref(),
            Some("Saturday 14 June 2025")
        );
        assert_eq!(
            labels.date("2025-06-14", DateFormat::Short).as_deref(),
            Some("14 Jun 2025")
        );
        assert_eq!(
            labels.date("0999-01-02", DateFormat::Iso).as_deref(),
            Some("0999-01-02")
        );
        assert_eq!(labels.duration(4 * 3600 + 5 * 60), "4h 05m");
        assert_eq!(labels.elapsed(3 * 86_400 + 5 * 3600), "3d 5h");
        assert_eq!(labels.number(7.36, 1), "7.4");
    }

    #[test]
    fn a_translation_changes_words_dates_and_numbers() {
        let labels = polish();
        assert_eq!(
            labels.date("2025-06-14", DateFormat::Long).as_deref(),
            Some("sobota, 14 czerwca 2025")
        );
        assert_eq!(
            labels.date("2025-10-05", DateFormat::Short).as_deref(),
            Some("5 paź 2025")
        );
        assert_eq!(
            labels.date("2025-06-16", DateFormat::Long).as_deref(),
            Some("poniedziałek, 16 czerwca 2025")
        );
        assert_eq!(labels.duration(4 * 3600 + 34 * 60), "4 godz. 34 min");
        assert_eq!(labels.number(7.36, 1), "7,4");
    }

    #[test]
    fn patterns_fill_known_names_and_keep_the_rest() {
        let values = [("a", "1".to_string()), ("bb", "2".to_string())];
        assert_eq!(fill("{a}-{bb} {c} {a", &values), "1-2 {c} {a");
        assert_eq!(fill("{{a}}", &values), "{1}");
        assert_eq!(
            placeholders("{weekday}, {day} {x").collect::<Vec<_>>(),
            ["weekday", "day"]
        );
    }
}
