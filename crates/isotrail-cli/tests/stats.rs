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
fn table_in_metric_units() {
    let file = fixture("multi-segment.gpx");
    let text = stdout(&isotrail(&["stats", file.to_str().unwrap()]));
    assert!(text.starts_with("Synthetic Ridge Loop\n"), "{text}");
    for line in [
        "Distance   0.34 km",
        "Gain       10 m",
        "Loss       4 m",
        "Elevation  2000 – 2012 m",
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
    assert!(text.contains("Distance   0.21 mi"), "{text}");
    assert!(text.contains("Elevation  6562 – 6601 ft"), "{text}");
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
    assert!(err.contains("could not open"), "{err}");

    let empty = fixture("empty.gpx");
    let output = isotrail(&["stats", empty.to_str().unwrap()]);
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        err.contains("could not read") && err.contains("the input is empty"),
        "{err}"
    );
}
