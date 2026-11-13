use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/gpx")
        .join(name)
}

fn isotrail(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn json_gives_the_format_a_bundle_was_written_in() {
    let bundles = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/bundles");
    let formats = [
        isotrail_bundle::OLDEST_FORMAT_VERSION,
        isotrail_bundle::FORMAT_VERSION,
    ];
    let read: Vec<serde_json::Value> = formats
        .iter()
        .map(|format| {
            let file = bundles.join(format!("format-{format}.itb"));
            let json = stdout(&isotrail(&["stats", file.to_str().unwrap(), "--json"]));
            serde_json::from_str(&json).unwrap()
        })
        .collect();
    for (json, format) in read.iter().zip(formats) {
        assert_eq!(json["format"], format);
        assert_eq!(json["generator"], "isotrail test");
    }
    let without_format = |json: &serde_json::Value| {
        let mut json = json.clone();
        json.as_object_mut().unwrap().remove("format");
        json
    };
    assert_eq!(without_format(&read[0]), without_format(&read[1]));
}

#[test]
fn table_in_metric_units() {
    let file = fixture("multi-segment.gpx");
    let text = stdout(&isotrail(&["stats", file.to_str().unwrap()]));
    assert!(text.starts_with("Synthetic Ridge Loop\n"), "{text}");
    for line in [
        "Distance   0.34km",
        "Gain       10m",
        "Loss       4m",
        "Elevation  2000 – 2012m",
        "Elapsed    1h 05m",
        "Moving     0m 27s",
        "Start      2025-07-01 10:00 CEST · 2025-07-01 08:00 UTC",
        "Points     12 in 3 segments",
    ] {
        assert!(text.contains(line), "missing {line:?} in\n{text}");
    }
}

#[test]
fn table_in_imperial_units() {
    let file = fixture("multi-segment.gpx");
    let text = stdout(&isotrail(&[
        "stats",
        file.to_str().unwrap(),
        "--units",
        "imperial",
    ]));
    assert!(text.contains("Distance   0.21mi"), "{text}");
    assert!(text.contains("Elevation  6562 – 6601ft"), "{text}");
}

#[test]
fn json_uses_si_units() {
    let file = fixture("multi-segment.gpx");
    for extra in [&[][..], &["--units", "imperial"][..]] {
        let mut args = vec!["stats", file.to_str().unwrap(), "--json"];
        args.extend(extra);
        let json: serde_json::Value = serde_json::from_str(&stdout(&isotrail(&args))).unwrap();
        assert_eq!(json["name"], "Synthetic Ridge Loop");
        assert_eq!(json["distance_m"], 343.8);
        assert_eq!(json["gain_m"], 10.0);
        assert_eq!(json["max_ele_m"], 2012.0);
        assert_eq!(json["elapsed_s"], 3927);
        assert_eq!(json["moving_s"], 27);
        assert_eq!(json["start"], "2025-07-01T08:00:00Z");
        assert_eq!(json["start_local"], "2025-07-01T10:00:00+02:00");
        assert_eq!(json["bounds"]["max_lon"], 10.003);
        assert_eq!(
            (json["points"].as_u64(), json["segments"].as_u64()),
            (Some(12), Some(3))
        );
    }
}

#[test]
fn missing_values_are_dashes() {
    let file = fixture("no-time.gpx");
    let output = isotrail(&["stats", file.to_str().unwrap()]);
    let text = stdout(&output);
    assert!(text.contains("Moving     —"), "{text}");
    assert!(text.contains("Start      —"), "{text}");
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(log.contains("no timestamps"), "{log}");

    let file = fixture("no-time.gpx");
    let json: serde_json::Value = serde_json::from_str(&stdout(&isotrail(&[
        "stats",
        file.to_str().unwrap(),
        "--json",
    ])))
    .unwrap();
    assert!(json["moving_s"].is_null() && json["start"].is_null());
}

#[test]
fn unreadable_files_fail_with_context() {
    let missing = fixture("does-not-exist.gpx");
    let output = isotrail(&["stats", missing.to_str().unwrap()]);
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        err.contains("can't open") && err.contains("the file doesn't exist"),
        "{err}"
    );

    let empty = fixture("empty.gpx");
    let output = isotrail(&["stats", empty.to_str().unwrap()]);
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        err.contains("can't read") && err.contains("the input is empty"),
        "{err}"
    );
}

/// Exit 0, and each warning once
fn stats_warning_once(file: &str, warnings: &[&str]) -> String {
    let output = isotrail(&["stats", fixture(file).to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    for warning in warnings {
        assert_eq!(
            stderr.matches(warning).count(),
            1,
            "{warning:?} in\n{stderr}"
        );
    }
    stdout(&output)
}

#[test]
fn bad_values_are_reported_and_skipped() {
    let text = stats_warning_once(
        "bad-values.gpx",
        &[
            "skipped 2 points without a valid position; 1 elevation and 1 time could not be read and are treated as missing (first on line 15)",
        ],
    );
    assert!(text.contains("Points     6 in 1 segment"), "{text}");
}

#[test]
fn glitched_times_and_stray_points_are_reported() {
    let text = stats_warning_once(
        "glitched-times.gpx",
        &["ignored 2 timestamps before 1990 or weeks away from the rest of the hike"],
    );
    assert!(text.contains("Elapsed    0m 24s"), "{text}");
    assert!(text.contains("Start      2025-07-01"), "{text}");
    let text = stats_warning_once(
        "stray-points.gpx",
        &["dropped 2 stray points far from their neighbours"],
    );
    assert!(text.contains("Distance   0.24km"), "{text}");
}

#[test]
fn several_tracks_are_joined_with_a_note() {
    stats_warning_once(
        "multi-track.gpx",
        &["the file has 2 tracks; they are joined into one hike"],
    );
}
