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
