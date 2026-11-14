use std::process::{Command, Output, Stdio};

fn isotrail(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .args(args)
        .env("COLUMNS", "100")
        .env("ISOTRAIL_CONFIG", "")
        .env_remove("ISOTRAIL_CACHE_DIR")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

/// The headings of a help screen, in order
fn headings(help: &str) -> Vec<&str> {
    help.lines()
        .filter(|line| !line.starts_with(' ') && line.ends_with(':'))
        .map(|line| line.trim_end_matches(':'))
        .collect()
}

/// The long options listed, in order
fn options(help: &str) -> Vec<&str> {
    help.lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with('-'))
        .filter_map(|line| line.split([' ', ',']).find(|word| word.starts_with("--")))
        .collect()
}

fn check(command: &str, expected: &[&str]) {
    for flag in ["--help", "-h"] {
        let output = isotrail(&[command, flag]);
        assert!(output.status.success());
        let help = text(&output.stdout);
        assert_eq!(headings(&help), expected, "{help}");
        let options = options(&help);
        assert!(options.len() > 40, "{help}");
        for (i, option) in options.iter().enumerate() {
            assert!(!options[..i].contains(option), "{option} is listed twice");
        }
    }
}

#[test]
fn render_help_groups_every_option_under_one_heading() {
    check(
        "render",
        &[
            "Arguments",
            "Output",
            "Image",
            "Camera",
            "Terrain and look",
            "Scene, light and route",
            "Text and overlays",
            "Animation",
            "Encoding",
            "Data",
            "General",
        ],
    );
}

#[test]
fn embed_help_groups_every_option_under_one_heading() {
    check(
        "embed",
        &[
            "Arguments",
            "Output",
            "Image",
            "Camera",
            "Terrain and look",
            "Scene, light and route",
            "Text and overlays",
            "Playback and controls",
            "Data",
            "General",
        ],
    );
}

#[test]
fn batch_help_groups_every_option_under_one_heading() {
    check(
        "batch",
        &[
            "Arguments",
            "Output",
            "Image",
            "Camera",
            "Terrain and look",
            "Scene, light and route",
            "Text and overlays",
            "Data",
            "General",
        ],
    );
}

#[test]
fn the_top_level_help_starts_with_render_and_an_example() {
    let output = isotrail(&["--help"]);
    assert!(output.status.success());
    let help = text(&output.stdout);
    let commands = help.split("Commands:\n").nth(1).unwrap();
    assert!(commands.trim_start().starts_with("render "), "{help}");
    assert!(help.contains("isotrail render hike.gpx"), "{help}");
    assert_eq!(headings(&help), ["Commands", "General", "Example"]);
}

#[test]
fn lists_default_with_commas_as_typed() {
    let help = text(&isotrail(&["render", "--help"]).stdout);
    // As one line, whatever the wrapping
    let help = help.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        help.contains("[default: title,date,stats,profile]"),
        "{help}"
    );
    assert!(
        help.contains("[default: distance,gain,time,high]"),
        "{help}"
    );
}

#[test]
fn the_usage_line_leaves_out_options_from_the_environment() {
    let output = Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .arg("render")
        .env("COLUMNS", "100")
        .env("ISOTRAIL_CONFIG", "")
        .env("ISOTRAIL_CACHE_DIR", "somewhere")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("Usage: isotrail render [OPTIONS] <FILE>"),
        "{stderr}"
    );
    assert!(!stderr.contains("--cache-dir"), "{stderr}");
}
