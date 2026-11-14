use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::{DateTime, SecondsFormat, TimeDelta, TimeZone, Utc};
use clap::Args;
use isotrail_bundle::{HikeStats, ProfileSample, SceneBundle, Weather, format_version};
use isotrail_geo::{PROFILE_SAMPLES, elevation_profile};
use isotrail_render::parse_count;
use isotrail_track::{Bounds, TrackStats, Units};
use serde::Serialize;
use tracing::warn;

use crate::overlay::data_credit;
use crate::servers::{CacheDirArg, WeatherUrlArg};
use crate::{UnitsArg, weather};

#[derive(Debug, Args)]
pub struct StatsArgs {
    /// GPX file or scene bundle (.itb) to read
    file: PathBuf,

    /// Print JSON for scripts, always in metres and seconds
    #[arg(long)]
    json: bool,

    /// Add the elevation profile to the JSON as N points, from 2 to 256
    #[arg(long, value_name = "N", value_parser = parse_profile, requires = "json")]
    profile: Option<usize>,

    /// Units for the table
    #[arg(long, value_enum, value_name = "metric|imperial", default_value_t = UnitsArg::Metric,
          hide_possible_values = true)]
    units: UnitsArg,

    /// Also show the weather hour by hour, from the bundle or from Open-Meteo
    #[arg(long)]
    weather: bool,

    /// Only use weather that is already in the cache
    #[arg(long, requires = "weather")]
    offline: bool,

    #[command(flatten)]
    weather_url: WeatherUrlArg,

    #[command(flatten)]
    cache_dir: CacheDirArg,
}

pub(crate) fn parse_profile(value: &str) -> Result<usize, String> {
    parse_count(value, &(2..=PROFILE_SAMPLES as u32)).map(|n| n as usize)
}

pub fn run(args: &StatsArgs) -> anyhow::Result<()> {
    let summary = Summary::read(&args.file, args.weather.then_some(args.offline), args)?;
    let mut out = io::stdout().lock();
    if args.json {
        serde_json::to_writer_pretty(&mut out, &Report::new(&summary, args.profile))?;
        writeln!(out)?;
    } else {
        let units = args.units.into();
        write_table(&mut out, &summary.title, &summary.stats, units)?;
        if let (Some(weather), Some(start), Some(end)) =
            (&summary.weather, summary.stats.start, summary.stats.end)
        {
            let span = (start.timestamp(), end.timestamp());
            weather::write_table(&mut out, weather, span, summary.stats.timezone, units)?;
        }
    }
    Ok(())
}

pub(crate) struct Summary {
    title: String,
    name: Option<String>,
    desc: Option<String>,
    stats: TrackStats,
    weather: Option<Weather>,
    /// Evenly spaced along the route, as a bundle holds it
    profile: Option<Vec<ProfileSample>>,
    scene: Option<SceneFacts>,
}

/// What only a scene bundle can say about itself
struct SceneFacts {
    format: u16,
    generator: String,
    caption_credit: Option<String>,
}

impl Summary {
    /// What a scene bundle read from `bytes` says about itself
    pub(crate) fn of_bundle(bundle: SceneBundle, bytes: &[u8], weather: bool) -> Summary {
        let scene = SceneFacts {
            format: format_version(bytes).unwrap_or_default(),
            generator: bundle.meta.provenance.generator.clone(),
            caption_credit: data_credit(&bundle),
        };
        let meta = bundle.meta;
        Summary {
            stats: track_stats(&meta.stats, meta.timezone.as_deref()),
            title: meta.title,
            name: meta.name,
            desc: meta.desc,
            weather: bundle.weather.filter(|_| weather),
            profile: Some(bundle.profile),
            scene: Some(scene),
        }
    }

    /// `weather` is `Some(offline)` to include the weather
    fn read(path: &Path, weather: Option<bool>, args: &StatsArgs) -> anyhow::Result<Summary> {
        crate::check_input(path)?;
        let bytes = crate::read_file(path)?;
        if crate::is_bundle_file(path, &bytes) {
            let bundle = SceneBundle::from_bytes(&bytes)
                .with_context(|| format!("can't read {}", path.display()))?;
            if weather.is_some() && bundle.weather.is_none() {
                warn!("the bundle has no weather");
            }
            return Ok(Summary::of_bundle(bundle, &bytes, weather.is_some()));
        }
        let track = crate::parse_track(&bytes, path)?;
        let weather = match weather {
            Some(offline) => {
                let fetcher = crate::bundle::fetcher(offline, args.cache_dir.get())?;
                weather::look_up(&track, fetcher, &args.weather_url.server()).map(|(w, _)| w)
            }
            None => None,
        };
        let stats = track.stats()?;
        Ok(Summary {
            profile: args
                .profile
                .and_then(|_| elevation_profile(&track, stats.distance_m)),
            stats,
            title: crate::title(&track, path),
            name: track.name,
            desc: track.desc,
            weather,
            scene: None,
        })
    }
}

fn track_stats(stats: &HikeStats, timezone: Option<&str>) -> TrackStats {
    let time = |secs: Option<i64>| DateTime::from_timestamp(secs?, 0);
    TrackStats {
        points: stats.points as usize,
        segments: stats.segments as usize,
        distance_m: stats.distance_m,
        gain_m: stats.gain_m,
        loss_m: stats.loss_m,
        min_ele_m: stats.min_ele_m,
        max_ele_m: stats.max_ele_m,
        bounds: Bounds {
            min_lat: stats.bounds.min_lat,
            min_lon: stats.bounds.min_lon,
            max_lat: stats.bounds.max_lat,
            max_lon: stats.bounds.max_lon,
        },
        start: time(stats.start),
        end: time(stats.end),
        elapsed: stats.elapsed_s.map(TimeDelta::seconds),
        moving: stats.moving_s.map(TimeDelta::seconds),
        timezone: timezone.and_then(|name| name.parse().ok()),
    }
}

const MISSING: &str = "—";

fn write_table(
    out: &mut impl Write,
    title: &str,
    stats: &TrackStats,
    units: Units,
) -> io::Result<()> {
    let elevation = |m: f64| format!("{:.0}{}", units.elevation(m), units.elevation_unit());
    let rows = [
        (
            "Distance",
            format!(
                "{:.2}{}",
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
pub(crate) struct Report<'a> {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    weather: Option<weather::Report>,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<Vec<ProfilePoint>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generator: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    caption_credit: Option<&'a str>,
}

#[derive(Debug, PartialEq, Serialize)]
struct ProfilePoint {
    distance_m: f64,
    ele_m: f64,
}

/// `n` points evenly spaced over the profile's distance, linear between
/// its samples, in decimetres
fn resample(profile: &[ProfileSample], n: usize) -> Vec<ProfilePoint> {
    let decimetres = |m: f64| (m * 10.0).round() / 10.0;
    let series: Vec<(f64, f64)> = profile
        .iter()
        .map(|s| (f64::from(s.distance_m), f64::from(s.ele_m)))
        .collect();
    let Some(&(total, _)) = series.last() else {
        return Vec::new();
    };
    let last = (n - 1).max(1);
    (0..n)
        .map(|i| {
            let at = (i * (series.len() - 1)) as f64 / last as f64;
            let below = (at.floor() as usize).min(series.len().saturating_sub(2));
            let ele = match (series.get(below), series.get(below + 1)) {
                (Some(&(_, e0)), Some(&(_, e1))) => e0 + (e1 - e0) * (at - below as f64),
                (Some(&(_, e)), None) | (None, Some(&(_, e))) => e,
                (None, None) => 0.0,
            };
            ProfilePoint {
                distance_m: decimetres(total * i as f64 / last as f64),
                ele_m: decimetres(ele),
            }
        })
        .collect()
}

#[derive(Serialize)]
struct BoundsReport {
    min_lat: f64,
    min_lon: f64,
    max_lat: f64,
    max_lon: f64,
}

impl<'a> Report<'a> {
    /// With `profile` points, if asked for and known
    pub(crate) fn new(summary: &'a Summary, profile: Option<usize>) -> Self {
        let stats = &summary.stats;
        let decimetres = |m: f64| (m * 10.0).round() / 10.0;
        let rfc3339 = |t: DateTime<_>| t.to_rfc3339_opts(SecondsFormat::Secs, true);
        Report {
            name: summary.name.as_deref(),
            desc: summary.desc.as_deref(),
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
            weather: summary.weather.as_ref().map(|weather| {
                let span = stats.start.zip(stats.end);
                weather::Report::new(weather, span.map(|(s, e)| (s.timestamp(), e.timestamp())))
            }),
            profile: profile.and_then(|n| Some(resample(summary.profile.as_ref()?, n))),
            format: summary.scene.as_ref().map(|scene| scene.format),
            generator: summary.scene.as_ref().map(|scene| &*scene.generator),
            caption_credit: summary
                .scene
                .as_ref()
                .and_then(|scene| scene.caption_credit.as_deref()),
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

    /// 256 samples over 2550m, climbing 2m every 10m
    fn profile() -> Vec<ProfileSample> {
        (0..PROFILE_SAMPLES)
            .map(|i| ProfileSample {
                distance_m: i as f32 * 10.0,
                ele_m: 300.0 + i as f32 * 2.0,
            })
            .collect()
    }

    #[test]
    fn profiles_are_evenly_resampled() {
        let point = |distance_m, ele_m| ProfilePoint { distance_m, ele_m };
        let samples = profile();
        assert_eq!(
            resample(&samples, 2),
            [point(0.0, 300.0), point(2550.0, 810.0)]
        );
        let all = resample(&samples, 256);
        assert_eq!(all.len(), 256);
        for (point, sample) in all.iter().zip(&samples) {
            assert_eq!(point.distance_m, f64::from(sample.distance_m));
            assert_eq!(point.ele_m, f64::from(sample.ele_m));
        }
        let some = resample(&samples, 64);
        assert_eq!(some.len(), 64);
        assert_eq!(some[0], point(0.0, 300.0));
        assert_eq!(some[63], point(2550.0, 810.0));
        // 2550m / 63 = 40.476m, between samples 4 and 5
        assert_eq!(some[1], point(40.5, 308.1));
        assert!(some.windows(2).all(|w| w[0].distance_m < w[1].distance_m));
    }

    #[test]
    fn profile_points_range_from_2_to_256() {
        assert_eq!(parse_profile("2"), Ok(2));
        assert_eq!(parse_profile("256"), Ok(256));
        for wrong in ["1", "257", "0", "many"] {
            assert_eq!(
                parse_profile(wrong),
                Err("expected a whole number from 2 to 256".into())
            );
        }
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
