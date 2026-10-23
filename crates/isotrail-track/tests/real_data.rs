use std::fs::{self, File};
use std::path::{Path, PathBuf};

use isotrail_track::{GpxReader, MIN_SEGMENT_POINTS, TrackReader};

fn input_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../input")
}

fn gpx_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("gpx"))
        })
        .collect();
    files.sort();
    files
}

fn expectations(dir: &Path) -> toml::Table {
    let path = dir.join("expected.toml");
    match fs::read_to_string(&path) {
        Ok(text) => text
            .parse()
            .unwrap_or_else(|err| panic!("{}: {err}", path.display())),
        Err(_) => toml::Table::new(),
    }
}

#[test]
#[ignore = "reads local recordings from input/"]
fn real_recordings_parse_and_match_expectations() {
    let dir = input_dir();
    let files = gpx_files(&dir);
    if files.is_empty() {
        eprintln!("skipped: no .gpx files in {}", dir.display());
        return;
    }
    let expected = expectations(&dir);
    let mut failures = Vec::new();

    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let mut file = File::open(path).unwrap();
        let raw = GpxReader
            .read(&mut file)
            .unwrap_or_else(|err| panic!("{name}: {err}"));
        let (raw_points, raw_segments) = (raw.point_count(), raw.segments.len());
        let track = raw.clean().unwrap_or_else(|err| panic!("{name}: {err}"));
        let (points, segments) = (track.point_count(), track.segments.len());
        let with_ele = track.points().filter(|p| p.ele.is_some()).count();
        let with_time = track.points().filter(|p| p.time.is_some()).count();
        eprintln!(
            "{name}\n  title {:?}\n  {raw_points} points in {raw_segments} segments, \
             {points} points in {segments} segments after cleaning\n  \
             elevation on {with_ele}, time on {with_time} of {points} points",
            track.name.as_deref().unwrap_or_default()
        );

        assert!(
            track
                .segments
                .iter()
                .all(|s| s.points.len() >= MIN_SEGMENT_POINTS),
            "{name}: a short segment survived cleaning"
        );

        let Some(expected) = expected.get(&name).and_then(|v| v.as_table()) else {
            eprintln!("  no expectations for this file");
            continue;
        };
        let actual = [
            ("raw_points", raw_points),
            ("raw_segments", raw_segments),
            ("points", points),
            ("segments", segments),
        ];
        for (key, actual) in actual {
            let Some(want) = expected.get(key) else {
                continue;
            };
            let want = want
                .as_integer()
                .unwrap_or_else(|| panic!("{name}: `{key}` must be an integer"));
            if i64::try_from(actual) != Ok(want) {
                failures.push(format!("{name}: {key} is {actual}, expected {want}"));
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn number(table: &toml::Table, key: &str) -> Option<f64> {
    match table.get(key)? {
        toml::Value::Integer(i) => Some(*i as f64),
        toml::Value::Float(f) => Some(*f),
        _ => panic!("`{key}` must be a number"),
    }
}

fn within(actual: f64, want: f64, tolerance: f64) -> bool {
    (actual - want).abs() <= want.abs() * tolerance
}

#[test]
#[ignore = "reads local recordings from input/"]
fn real_recordings_stats_match_expectations() {
    let dir = input_dir();
    let files = gpx_files(&dir);
    if files.is_empty() {
        eprintln!("skipped: no .gpx files in {}", dir.display());
        return;
    }
    let expected = expectations(&dir);
    let mut failures = Vec::new();

    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let mut file = File::open(path).unwrap();
        let stats = GpxReader
            .read(&mut file)
            .and_then(|track| track.clean())
            .and_then(|track| track.stats())
            .unwrap_or_else(|err| panic!("{name}: {err}"));
        let distance_km = stats.distance_m / 1000.0;
        let gain = stats.gain_m.unwrap_or(f64::NAN);
        let moving_min = stats
            .moving
            .map_or(f64::NAN, |d| d.num_seconds() as f64 / 60.0);
        eprintln!(
            "{name}\n  {distance_km:.2} km, gain {gain:.0} m, loss {:.0} m, \
             elevation {:?}..{:?} m\n  elapsed {:?}, moving {moving_min:.0} min, zone {:?}",
            stats.loss_m.unwrap_or(f64::NAN),
            stats.min_ele_m,
            stats.max_ele_m,
            stats.elapsed.map(|d| d.num_minutes()),
            stats.timezone.map(|tz| tz.name()),
        );

        let Some(expected) = expected.get(&name).and_then(|v| v.as_table()) else {
            eprintln!("  no expectations for this file");
            continue;
        };
        let mut check = |what: &str, actual: f64, want: Option<f64>, tolerance: f64| {
            if let Some(want) = want
                && !within(actual, want, tolerance)
            {
                failures.push(format!(
                    "{name}: {what} is {actual:.2}, expected {want} ±{:.0}%",
                    tolerance * 100.0
                ));
            }
        };
        check(
            "distance_km",
            distance_km,
            number(expected, "distance_km"),
            0.01,
        );
        check(
            "distance_km (AllTrails)",
            distance_km,
            number(expected, "alltrails_distance_km"),
            0.02,
        );
        check(
            "moving_min (AllTrails)",
            moving_min,
            number(expected, "alltrails_moving_min"),
            0.10,
        );
        if let Some(want) = number(expected, "alltrails_gain_m") {
            check("gain_m (AllTrails)", gain, Some(want), 0.10);
        } else if let (Some(low), Some(high)) = (
            number(expected, "gain_ma9_m"),
            number(expected, "gain_raw_m"),
        ) && !(low..=high).contains(&gain)
        {
            failures.push(format!(
                "{name}: gain_m is {gain:.0}, expected between {low} and {high}"
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
