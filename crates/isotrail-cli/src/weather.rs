use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, SecondsFormat, Utc};
use isotrail_bundle::{
    Units, WEATHER_ATTRIBUTION, Weather, WeatherDataset, WeatherSample, weather_condition,
};
use isotrail_geo::{
    Fetcher, MAX_TIMED_SPAN_DAYS, OpenMeteoSource, RequestError, WeatherError, WeatherRequest,
    WeatherServer, WeatherSource,
};
use isotrail_track::{Track, Tz};
use serde::Serialize;
use tracing::warn;

const MISSING: &str = "—";
const HOUR_S: i64 = 3600;

/// The weather during the hike and where it came from, or a warning and
/// nothing when it can't be found
pub fn look_up(
    track: &Track,
    fetcher: impl Fetcher,
    server: &WeatherServer,
) -> Option<(Weather, String)> {
    let request = match WeatherRequest::from_track(track, now()) {
        Ok(request) => request,
        Err(RequestError::Untimed) => {
            warn!("the track has no times, so the weather is left out");
            return None;
        }
        Err(RequestError::TooLong { days }) => {
            warn!(
                "the hike spans {days} days, more than {MAX_TIMED_SPAN_DAYS}, so the weather is left out"
            );
            return None;
        }
    };
    match OpenMeteoSource::new(fetcher)
        .server(server)
        .weather(&request)
    {
        Ok(weather) => {
            let places = request.locations.len();
            let provenance = format!(
                "Open-Meteo {} at {places} location{}",
                dataset_name(weather.dataset),
                if places == 1 { "" } else { "s" }
            );
            Some((weather, provenance))
        }
        Err(WeatherError::Offline { .. }) => {
            warn!(
                "the weather for this hike isn't in the cache, so it is left out; run once without --offline to add it"
            );
            None
        }
        Err(err) => {
            warn!(
                "can't get the weather, so it is left out: {}",
                crate::describe(&err.into())
            );
            None
        }
    }
}

/// Unix seconds, for choosing between the archive and the forecast
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

pub fn dataset_name(dataset: WeatherDataset) -> &'static str {
    match dataset {
        WeatherDataset::Archive => "archive",
        WeatherDataset::Forecast => "forecast",
    }
}

/// The hours whose values describe the hike: on the hour from the one at or
/// before the start to the one at or after the end
fn hike_hours(weather: &Weather, start: i64, end: i64) -> &[WeatherSample] {
    let first = start.div_euclid(HOUR_S) * HOUR_S;
    let last = (end + HOUR_S - 1).div_euclid(HOUR_S) * HOUR_S;
    let samples = &weather.samples;
    let from = samples.partition_point(|s| s.time < first);
    let to = samples.partition_point(|s| s.time <= last);
    &samples[from..to.max(from)]
}

pub struct Summary {
    /// The most common condition over the hike's hours, the worse on a tie
    pub condition: &'static str,
    pub min_c: f32,
    pub max_c: f32,
    pub rain_mm: f32,
    pub snowfall_cm: f32,
    pub snow_depth_m: f32,
    /// Strongest wind and where it came from
    pub wind: (f32, f32),
}

impl Summary {
    pub fn new(weather: &Weather, start: i64, end: i64) -> Option<Summary> {
        let hours = hike_hours(weather, start, end);
        let first = hours.first()?;
        // Sums and codes cover the hour that ends at a sample's time
        let walked: Vec<&WeatherSample> = hours
            .iter()
            .filter(|s| s.time > start && s.time - HOUR_S < end)
            .collect();
        let walked = if walked.is_empty() {
            vec![first]
        } else {
            walked
        };
        let mut codes: Vec<u8> = walked.iter().map(|s| s.weather_code).collect();
        codes.sort_unstable();
        let code = codes
            .chunk_by(|a, b| a == b)
            .max_by_key(|run| run.len())
            .map_or(first.weather_code, |run| run[0]);
        let strongest = hours
            .iter()
            .max_by(|a, b| a.wind_speed_m_s.total_cmp(&b.wind_speed_m_s))
            .unwrap_or(first);
        Some(Summary {
            condition: weather_condition(code),
            min_c: hours
                .iter()
                .map(|s| s.temperature_c)
                .fold(f32::MAX, f32::min),
            max_c: hours
                .iter()
                .map(|s| s.temperature_c)
                .fold(f32::MIN, f32::max),
            rain_mm: walked
                .iter()
                .map(|s| (s.precipitation_mm - s.snowfall_cm / 0.7).max(s.rain_mm))
                .sum(),
            snowfall_cm: walked.iter().map(|s| s.snowfall_cm).sum(),
            snow_depth_m: hours.iter().map(|s| s.snow_depth_m).fold(0.0, f32::max),
            wind: (strongest.wind_speed_m_s, strongest.wind_direction_deg),
        })
    }

    /// One line for the `bundle` report
    pub fn line(&self, dataset: WeatherDataset) -> String {
        let units = Units::Metric;
        let mut parts = vec![
            self.condition.to_owned(),
            temperature_range(self.min_c, self.max_c, units),
        ];
        if self.rain_mm >= 0.05 {
            parts.push(format!("{} rain", precipitation(self.rain_mm, units)));
        }
        if self.snowfall_cm >= 0.05 {
            parts.push(format!("{} snow", snowfall(self.snowfall_cm, units)));
        }
        format!(
            "{} (Open-Meteo {})",
            parts.join(", "),
            dataset_name(dataset)
        )
    }
}

pub fn write_table(
    out: &mut impl Write,
    weather: &Weather,
    (start, end): (i64, i64),
    timezone: Option<Tz>,
    units: Units,
) -> io::Result<()> {
    let Some(summary) = Summary::new(weather, start, end) else {
        return Ok(());
    };
    let amount = |value: f32, text: String| if value >= 0.05 { text } else { "none".into() };
    let mut rows = vec![
        (
            "Weather",
            format!(
                "{} (Open-Meteo {})",
                summary.condition,
                dataset_name(weather.dataset)
            ),
        ),
        (
            "Temp",
            temperature_range(summary.min_c, summary.max_c, units),
        ),
        (
            "Rain",
            amount(summary.rain_mm, precipitation(summary.rain_mm, units)),
        ),
        (
            "Snowfall",
            amount(summary.snowfall_cm, snowfall(summary.snowfall_cm, units)),
        ),
        (
            "Wind",
            format!("up to {}", wind(summary.wind.0, summary.wind.1, units)),
        ),
    ];
    if summary.snow_depth_m >= 0.005 {
        rows.push(("Snow depth", snow_depth(summary.snow_depth_m, units)));
    }
    rows.push(("Credit", WEATHER_ATTRIBUTION.into()));

    writeln!(out)?;
    for (label, value) in rows {
        writeln!(out, "  {label:<10} {value}")?;
    }
    writeln!(out)?;
    writeln!(
        out,
        "  {:<6} {:>8}  {:<22} {:>14}  {:>8}  Wind",
        if timezone.is_some() { "Hour" } else { "UTC" },
        "Temp",
        "Conditions",
        "Cloud l/m/h %",
        "Precip"
    )?;
    let mut day = None;
    for s in hike_hours(weather, start, end) {
        let Some(utc) = DateTime::from_timestamp(s.time, 0) else {
            continue;
        };
        let (date, hour) = match timezone {
            Some(tz) => {
                let local = utc.with_timezone(&tz);
                (
                    local.format("%a %-d %b %Y").to_string(),
                    local.format("%H:%M").to_string(),
                )
            }
            None => (
                utc.format("%a %-d %b %Y").to_string(),
                utc.format("%H:%M").to_string(),
            ),
        };
        if day.as_ref() != Some(&date) {
            writeln!(out, "  {date}")?;
            day = Some(date);
        }
        let water = if s.snowfall_cm >= 0.05 {
            snowfall(s.snowfall_cm, units)
        } else if s.precipitation_mm >= 0.05 {
            precipitation(s.precipitation_mm, units)
        } else {
            MISSING.into()
        };
        writeln!(
            out,
            "  {hour:<6} {:>8}  {:<22} {:>14}  {water:>8}  {}",
            temperature(s.temperature_c, units),
            weather_condition(s.weather_code),
            format!(
                "{:.0}/{:.0}/{:.0}",
                s.cloud_low_pct, s.cloud_mid_pct, s.cloud_high_pct
            ),
            wind(s.wind_speed_m_s, s.wind_direction_deg, units),
        )?;
    }
    Ok(())
}

fn temperature(celsius: f32, units: Units) -> String {
    match units {
        Units::Metric => format!("{celsius:.1}°C"),
        Units::Imperial => format!("{:.0}°F", celsius * 1.8 + 32.0),
    }
}

fn temperature_range(min: f32, max: f32, units: Units) -> String {
    let (min, max, unit) = match units {
        Units::Metric => (min, max, "°C"),
        Units::Imperial => (min * 1.8 + 32.0, max * 1.8 + 32.0, "°F"),
    };
    let (min, max) = (format!("{min:.0}"), format!("{max:.0}"));
    if min == max {
        format!("{max}{unit}")
    } else {
        format!("{min} – {max}{unit}")
    }
}

fn precipitation(mm: f32, units: Units) -> String {
    match units {
        Units::Metric => format!("{mm:.1}mm"),
        Units::Imperial => format!("{:.2} in", mm / 25.4),
    }
}

fn snowfall(cm: f32, units: Units) -> String {
    match units {
        Units::Metric => format!("{cm:.1}cm"),
        Units::Imperial => format!("{:.1} in", cm / 2.54),
    }
}

fn snow_depth(m: f32, units: Units) -> String {
    match units {
        Units::Metric => format!("{:.0}cm", m * 100.0),
        Units::Imperial => format!("{:.0} in", m / 0.0254),
    }
}

fn wind(m_s: f32, from_deg: f32, units: Units) -> String {
    let speed = match units {
        Units::Metric => format!("{:.0}km/h", m_s * 3.6),
        Units::Imperial => format!("{:.0}mph", m_s / 0.447_04),
    };
    format!("{speed} {}", compass(from_deg))
}

fn compass(deg: f32) -> &'static str {
    const POINTS: [&str; 16] = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW",
        "NW", "NNW",
    ];
    POINTS[((deg.rem_euclid(360.0) / 22.5).round() as usize) % 16]
}

/// The bundle's samples as `stats --json` prints them, in SI-suffixed keys
#[derive(Serialize)]
pub struct Report {
    dataset: &'static str,
    attribution: &'static str,
    samples: Vec<SampleReport>,
}

#[derive(Serialize)]
struct SampleReport {
    time: Option<String>,
    temperature_c: f32,
    weather_code: u8,
    condition: &'static str,
    cloud_cover_pct: f32,
    cloud_cover_low_pct: f32,
    cloud_cover_mid_pct: f32,
    cloud_cover_high_pct: f32,
    precipitation_mm: f32,
    rain_mm: f32,
    snowfall_cm: f32,
    snow_depth_m: f32,
    wind_speed_m_s: f32,
    wind_direction_deg: f32,
}

impl Report {
    pub fn new(weather: &Weather) -> Report {
        let rfc3339 = |time| {
            DateTime::<Utc>::from_timestamp(time, 0)
                .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true))
        };
        Report {
            dataset: dataset_name(weather.dataset),
            attribution: WEATHER_ATTRIBUTION,
            samples: weather
                .samples
                .iter()
                .map(|s| SampleReport {
                    time: rfc3339(s.time),
                    temperature_c: s.temperature_c,
                    weather_code: s.weather_code,
                    condition: weather_condition(s.weather_code),
                    cloud_cover_pct: s.cloud_cover_pct,
                    cloud_cover_low_pct: s.cloud_low_pct,
                    cloud_cover_mid_pct: s.cloud_mid_pct,
                    cloud_cover_high_pct: s.cloud_high_pct,
                    precipitation_mm: s.precipitation_mm,
                    rain_mm: s.rain_mm,
                    snowfall_cm: s.snowfall_cm,
                    snow_depth_m: s.snow_depth_m,
                    wind_speed_m_s: s.wind_speed_m_s,
                    wind_direction_deg: s.wind_direction_deg,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use isotrail_testkit::synthetic_weather;

    use super::*;

    const FIRST: i64 = 1_750_064_400;

    #[test]
    fn summary_covers_the_hours_walked() {
        let weather = synthetic_weather(FIRST, 8);
        // 10 past the first hour to 10 past the fourth: hours 1 to 4 walked
        let summary = Summary::new(&weather, FIRST + 600, FIRST + 3 * 3600 + 600).unwrap();
        assert_eq!((summary.min_c, summary.max_c), (2.0, 8.0));
        assert!((summary.rain_mm - 0.6).abs() < 1e-6);
        assert!((summary.snowfall_cm - 0.35).abs() < 1e-6);
        assert_eq!(summary.condition, "snow showers");
        assert_eq!(summary.wind, (8.0, 250.0));
        assert_eq!(
            summary.line(WeatherDataset::Archive),
            "snow showers, 2 – 8°C, 0.6mm rain, 0.3cm snow (Open-Meteo archive)"
        );
    }

    #[test]
    fn hours_outside_the_samples_have_no_summary() {
        let weather = synthetic_weather(FIRST, 3);
        assert!(Summary::new(&weather, FIRST + 86_400, FIRST + 90_000).is_none());
    }

    #[test]
    fn table_has_summary_then_hours_by_local_day() {
        let weather = synthetic_weather(FIRST, 8);
        let mut out = Vec::new();
        let tz = Some(Tz::Europe__Warsaw);
        let span = (FIRST + 600, FIRST + 3 * 3600 + 600);
        write_table(&mut out, &weather, span, tz, Units::Metric).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("  Weather    snow showers (Open-Meteo archive)\n"),
            "{text}"
        );
        assert!(text.contains("  Temp       2 – 8°C\n"), "{text}");
        assert!(text.contains("  Wind       up to 29km/h WSW\n"), "{text}");
        assert!(text.contains("  Snow depth 5cm\n"), "{text}");
        assert!(
            text.contains("  Credit     Weather data by Open-Meteo.com\n"),
            "{text}"
        );
        assert!(text.contains("  Mon 16 Jun 2025\n"), "{text}");
        assert!(text.contains("  11:00     2.0°C  partly cloudy"), "{text}");
        assert!(text.contains("light rain"), "{text}");
        assert_eq!(text.matches("°C  ").count(), 5, "{text}");

        let mut out = Vec::new();
        write_table(&mut out, &weather, span, None, Units::Imperial).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("  Temp       36 – 46°F\n"), "{text}");
        assert!(text.contains("  UTC "), "{text}");
        assert!(text.contains("  09:00 "), "{text}");
    }

    #[test]
    fn compass_points() {
        assert_eq!(compass(0.0), "N");
        assert_eq!(compass(359.0), "N");
        assert_eq!(compass(200.0), "SSW");
        assert_eq!(compass(270.0), "W");
    }
}
