use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::Context;
use chrono::{DateTime, SecondsFormat, TimeDelta, TimeZone, Utc};
use clap::{Args, ValueEnum};
use isotrail_track::{GpxReader, Track, TrackReader, TrackStats, Units};
use serde::Serialize;

#[derive(Debug, Args)]
pub struct StatsArgs {
    /// GPX file to read
    file: PathBuf,

    /// Print machine-readable JSON (always metres and seconds)
    #[arg(long)]
    json: bool,

    /// Units for the human-readable table
    #[arg(long, value_enum, default_value_t = UnitsArg::Metric)]
    units: UnitsArg,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum UnitsArg {
    Metric,
    Imperial,
}

impl From<UnitsArg> for Units {
    fn from(units: UnitsArg) -> Self {
        match units {
            UnitsArg::Metric => Units::Metric,
            UnitsArg::Imperial => Units::Imperial,
        }
    }
}

pub fn run(args: &StatsArgs) -> anyhow::Result<()> {
    let path = &args.file;
    let mut file =
        File::open(path).with_context(|| format!("could not open {}", path.display()))?;
    let track = GpxReader
        .read(&mut file)
        .and_then(Track::clean)
        .with_context(|| format!("could not read {}", path.display()))?;
    let stats = track.stats()?;
    let title = track
        .name
        .clone()
        .or_else(|| Some(path.file_stem()?.to_string_lossy().into_owned()))
        .unwrap_or_default();

    let mut out = io::stdout().lock();
    if args.json {
        serde_json::to_writer_pretty(&mut out, &Report::new(&track, &stats))?;
        writeln!(out)?;
    } else {
        write_table(&mut out, &title, &stats, args.units.into())?;
    }
    Ok(())
}

const MISSING: &str = "—";

fn write_table(
    out: &mut impl Write,
    title: &str,
    stats: &TrackStats,
    units: Units,
) -> io::Result<()> {
    let elevation = |m: f64| format!("{:.0} {}", units.elevation(m), units.elevation_unit());
    let rows = [
        (
            "Distance",
            format!(
                "{:.2} {}",
                units.distance(stats.distance_m),
                units.distance_unit()
            ),
        ),
        ("Gain", stats.gain_m.map_or(MISSING.into(), elevation)),
        ("Loss", stats.loss_m.map_or(MISSING.into(), elevation)),
        (
            "Elevation",
            match (stats.min_ele_m, stats.max_ele_m) {
                (Some(min), Some(max)) => {
                    format!("{:.0} – {}", units.elevation(min), elevation(max))
                }
                _ => MISSING.into(),
            },
        ),
        ("Elapsed", stats.elapsed.map_or(MISSING.into(), duration)),
        ("Moving", stats.moving.map_or(MISSING.into(), duration)),
        ("Start", when(stats.start, stats.start_local())),
        ("End", when(stats.end, stats.end_local())),
        (
            "Time zone",
            stats.timezone.map_or(MISSING.into(), |tz| tz.name().into()),
        ),
        (
            "Bounds",
            format!(
                "{:.5}, {:.5} to {:.5}, {:.5}",
                stats.bounds.min_lat,
                stats.bounds.min_lon,
                stats.bounds.max_lat,
                stats.bounds.max_lon
            ),
        ),
        (
            "Points",
            format!(
                "{} in {} segment{}",
                stats.points,
                stats.segments,
                if stats.segments == 1 { "" } else { "s" }
            ),
        ),
    ];

    writeln!(out, "{title}")?;
    writeln!(out)?;
    for (label, value) in rows {
        writeln!(out, "  {label:<10} {value}")?;
    }
    Ok(())
}

fn duration(d: TimeDelta) -> String {
    let secs = d.num_seconds();
    if secs < 3600 {
        return format!("{}m {:02}s", secs / 60, secs % 60);
    }
    let mins = (secs + 30) / 60;
    let (days, hours, mins) = (mins / 1440, mins / 60 % 24, mins % 60);
    if days > 0 {
        format!("{days}d {hours:02}h {mins:02}m")
    } else {
        format!("{hours}h {mins:02}m")
    }
}

fn when<Tz: TimeZone>(utc: Option<DateTime<Utc>>, local: Option<DateTime<Tz>>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let Some(utc) = utc else {
        return MISSING.into();
    };
    let utc = utc.format("%Y-%m-%d %H:%M UTC").to_string();
    match local {
        Some(local) => format!("{} · {utc}", local.format("%Y-%m-%d %H:%M %Z")),
        None => utc,
    }
}

#[derive(Serialize)]
struct Report<'a> {
    name: Option<&'a str>,
    desc: Option<&'a str>,
    points: usize,
    segments: usize,
    distance_m: f64,
    gain_m: Option<f64>,
    loss_m: Option<f64>,
    min_ele_m: Option<f64>,
    max_ele_m: Option<f64>,
    elapsed_s: Option<i64>,
    moving_s: Option<i64>,
    start: Option<String>,
    end: Option<String>,
    start_local: Option<String>,
    end_local: Option<String>,
    timezone: Option<&'static str>,
    bounds: BoundsReport,
}

#[derive(Serialize)]
struct BoundsReport {
    min_lat: f64,
    min_lon: f64,
    max_lat: f64,
    max_lon: f64,
}

impl<'a> Report<'a> {
    fn new(track: &'a Track, stats: &TrackStats) -> Self {
        let decimetres = |m: f64| (m * 10.0).round() / 10.0;
        let rfc3339 = |t: DateTime<_>| t.to_rfc3339_opts(SecondsFormat::Secs, true);
        Report {
            name: track.name.as_deref(),
            desc: track.desc.as_deref(),
            points: stats.points,
            segments: stats.segments,
            distance_m: decimetres(stats.distance_m),
            gain_m: stats.gain_m.map(decimetres),
            loss_m: stats.loss_m.map(decimetres),
            min_ele_m: stats.min_ele_m,
            max_ele_m: stats.max_ele_m,
            elapsed_s: stats.elapsed.map(|d| d.num_seconds()),
            moving_s: stats.moving.map(|d| d.num_seconds()),
            start: stats.start.map(rfc3339),
            end: stats.end.map(rfc3339),
            start_local: stats
                .start_local()
                .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true)),
            end_local: stats
                .end_local()
                .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true)),
            timezone: stats.timezone.map(|tz| tz.name()),
            bounds: BoundsReport {
                min_lat: stats.bounds.min_lat,
                min_lon: stats.bounds.min_lon,
                max_lat: stats.bounds.max_lat,
                max_lon: stats.bounds.max_lon,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(duration(TimeDelta::seconds(27)), "0m 27s");
        assert_eq!(duration(TimeDelta::seconds(3927)), "1h 05m");
        assert_eq!(duration(TimeDelta::seconds(7_170)), "2h 00m");
        assert_eq!(duration(TimeDelta::seconds(3599)), "59m 59s");
        assert_eq!(
            duration(TimeDelta::seconds(3 * 86_400 + 5 * 3600 + 60)),
            "3d 05h 01m"
        );
    }

    #[test]
    fn times_show_local_and_utc() {
        let utc = Utc.with_ymd_and_hms(2025, 7, 1, 23, 30, 0).unwrap();
        let local = utc.with_timezone(&isotrail_track::Tz::Asia__Tokyo);
        assert_eq!(
            when(Some(utc), Some(local)),
            "2025-07-02 08:30 JST · 2025-07-01 23:30 UTC"
        );
        assert_eq!(when::<Utc>(Some(utc), None), "2025-07-01 23:30 UTC");
        assert_eq!(when::<Utc>(None, None), MISSING);
    }
}
