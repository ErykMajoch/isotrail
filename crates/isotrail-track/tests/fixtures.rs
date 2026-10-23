//! Reads the synthetic GPX files in `fixtures/gpx/`

use chrono::{TimeZone, Utc};
use isotrail_track::{GpxReader, Track, TrackError, TrackReader};

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/gpx/",
            $name
        ))
    };
}

fn segment_lengths(track: &Track) -> Vec<usize> {
    track.segments.iter().map(|s| s.points.len()).collect()
}

#[test]
fn multi_segment_keeps_segment_boundaries() {
    let track = GpxReader
        .parse(fixture!("multi-segment.gpx"))
        .unwrap()
        .clean()
        .unwrap();
    assert_eq!(track.name.as_deref(), Some("Synthetic Ridge Loop"));
    assert_eq!(
        track.desc.as_deref(),
        Some("Three recording segments separated by two pauses.")
    );
    assert_eq!(segment_lengths(&track), [4, 3, 5]);
    assert!(track.points().all(|p| p.ele.is_some() && p.time.is_some()));

    let first = track.segments[0].points[0];
    assert_eq!(
        (first.lat, first.lon, first.ele),
        (46.5, 10.0, Some(2000.0))
    );
    assert_eq!(
        first.time,
        Some(Utc.with_ymd_and_hms(2025, 7, 1, 8, 0, 0).unwrap())
    );
    let last = track.segments[2].points[4];
    assert_eq!(
        (last.lat, last.lon, last.ele),
        (46.5022, 10.003, Some(2005.0))
    );
    assert_eq!(
        last.time,
        Some(Utc.with_ymd_and_hms(2025, 7, 1, 9, 5, 27).unwrap())
    );
}

#[test]
fn single_point_segments_are_dropped_by_cleaning() {
    let raw = GpxReader
        .parse(fixture!("single-point-segments.gpx"))
        .unwrap();
    assert_eq!(segment_lengths(&raw), [1, 3, 1, 3, 0]);
    assert_eq!(raw.point_count(), 8);

    let track = raw.clean().unwrap();
    assert_eq!(segment_lengths(&track), [3, 3]);
    assert_eq!(track.segments[0].points[0].lat, 46.5101);
    assert_eq!(track.segments[1].points[0].lat, 46.5107);
}

#[test]
fn missing_time_is_none() {
    let track = GpxReader
        .parse(fixture!("no-time.gpx"))
        .unwrap()
        .clean()
        .unwrap();
    assert_eq!(track.point_count(), 4);
    assert!(track.points().all(|p| p.time.is_none() && p.ele.is_some()));
}

#[test]
fn missing_elevation_is_none() {
    let track = GpxReader
        .parse(fixture!("no-ele.gpx"))
        .unwrap()
        .clean()
        .unwrap();
    assert_eq!(track.point_count(), 4);
    assert!(track.points().all(|p| p.ele.is_none() && p.time.is_some()));
}

#[test]
fn route_is_read_without_times() {
    let track = GpxReader
        .parse(fixture!("route-only.gpx"))
        .unwrap()
        .clean()
        .unwrap();
    assert_eq!(track.name.as_deref(), Some("Synthetic Planned Traverse"));
    assert_eq!(
        track.desc.as_deref(),
        Some("A planned route with no recording. Two points carry planner timestamps.")
    );
    assert_eq!(segment_lengths(&track), [5]);
    assert!(track.points().all(|p| p.time.is_none() && p.ele.is_some()));
}

#[test]
fn non_ascii_name_survives() {
    let track = GpxReader
        .parse(fixture!("non-ascii-name.gpx"))
        .unwrap()
        .clean()
        .unwrap();
    assert_eq!(
        track.name.as_deref(),
        Some("Dzień 1 | Grań nad łąką & żlebem (1)")
    );
    assert_eq!(
        track.desc.as_deref(),
        Some("Śnieg, mgła & wiatr \u{2014} widok <3 🏔")
    );
    assert_eq!(track.point_count(), 3);
}

#[test]
fn empty_file_is_an_error() {
    let err = GpxReader.parse(fixture!("empty.gpx")).unwrap_err();
    assert!(matches!(err, TrackError::EmptyInput), "{err:?}");
}
