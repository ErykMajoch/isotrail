use glam::Vec3;

use super::*;
use crate::DateFormat;

const LOUD: &str = include_str!("../../../../fixtures/themes/loud.toml");

fn error(text: &str) -> String {
    Theme::parse(text).unwrap_err().to_string()
}

#[test]
fn the_printed_built_in_theme_reads_back_as_it_is() {
    let built_in = Theme::default();
    assert_eq!(built_in.look(), Look::default());
    assert_eq!(built_in.terrain(), Terrain::default());
    assert_eq!(built_in.conditions(), Conditions::default());
    assert_eq!(built_in.overlay_style(), OverlayStyle::default());
    let printed = built_in.to_toml();
    assert_eq!(Theme::parse(&printed), Ok(Theme::default()));
    assert!(printed.contains("\n[light.night]\n"), "{printed}");
    assert!(printed.contains("\ncolour = \"#d64834\"\n"), "{printed}");
    assert!(printed.contains("\nside-shade = 0.75\n"), "{printed}");
    assert!(printed.contains("\ndither = 0.08\n"), "{printed}");
    assert!(
        printed.contains("\ncolour = [1.0, 0.96, 0.88]\n"),
        "{printed}"
    );
    assert!(printed.contains("seconds\npause-cut = 60.0\n"), "{printed}");
    for line in printed.lines().filter(|line| line.starts_with('#')) {
        assert!(!line.ends_with('.'), "{line}");
        assert!(!line.contains(';'), "{line}");
        assert!(line.chars().count() <= 80, "{line}");
    }
}

#[test]
fn a_theme_round_trips_through_its_file() {
    let loud = Theme::parse(LOUD).unwrap();
    assert_ne!(loud, Theme::default());
    assert_eq!(Theme::parse(&loud.to_toml()), Ok(loud));
}

#[test]
fn keys_left_out_keep_their_built_in_values() {
    assert_eq!(Theme::parse(""), Ok(Theme::default()));
    let theme = Theme::parse("\u{feff}[route]\ncolour = \"#123456\"\n").unwrap();
    let expected = Theme {
        route: RouteStyle {
            colour: [0x12, 0x34, 0x56],
            ..RouteStyle::default()
        },
        ..Theme::default()
    };
    assert_eq!(theme, expected);
    assert_eq!(theme.look().route.colour, [0x12, 0x34, 0x56]);
    assert_eq!(theme.overlay_style().route_colour, [0x12, 0x34, 0x56]);
}

#[test]
fn the_loud_theme_reaches_every_part() {
    let theme = Theme::parse(LOUD).unwrap();
    assert_eq!(theme.slab.landcover[4], [0xe0, 0x40, 0xfb, 255]);
    assert_eq!(theme.terrain().style.side_shade, 0.5);
    assert_eq!(
        theme.light.ramp[4].sky,
        [[0xff, 0x40, 0x81], [0xff, 0xe5, 0x7f]]
    );
    assert_eq!(theme.light.ramp[0].sky[0], [0, 0, 0]);
    assert_eq!(theme.conditions().light.bands, 3);
    assert_eq!(theme.light.ramp[4].colour, Vec3::new(1.0, 0.8, 1.0));
    assert_eq!(theme.look().sky_dither, 0.0);
    assert_eq!(theme.look().route.width_px, 3);
    assert_eq!(theme.overlay_style().theme.profile_rows, 16);
    assert_eq!(theme.pacing.night_share, 0.1);
    assert_eq!(
        theme.labels.date("2025-10-05", DateFormat::Long).as_deref(),
        Some("niedziela, 5 października 2025")
    );
    assert_eq!(theme.labels.duration(4 * 3600 + 5 * 60), "4 godz. 05 min");
}

#[test]
fn unknown_tables_and_keys_are_named() {
    assert_eq!(error("[glow]\nx = 1\n"), "unknown table [glow]");
    assert_eq!(
        error("[water]\nglow = \"#ffffff\"\n"),
        "unknown key \"glow\" in the [water] table"
    );
    assert_eq!(
        error("[light.dusk]\nambient = 0.3\n"),
        "unknown table [light.dusk]"
    );
    assert_eq!(
        error("[light.night]\nfill = 0.3\n"),
        "unknown key \"fill\" in the [light.night] table"
    );
    assert_eq!(
        error("colour = \"#ffffff\"\n"),
        "\"colour\" is outside a table; keys go in tables such as [route]"
    );
    assert_eq!(error("route = 3\n"), "route: expected a table");
    assert!(
        error("[route\n").contains("line 1"),
        "{}",
        error("[route\n")
    );
}

#[test]
fn values_are_checked() {
    let cases = [
        (
            "[route]\ncolour = \"#12345\"",
            "route.colour: expected a colour such as #d64834",
        ),
        (
            "[route]\ncolour = 3",
            "route.colour: expected a colour such as \"#d64834\", found a number",
        ),
        ("[route]\nwidth = 5", "route.width: must be between 1 and 4"),
        (
            "[route]\nwidth = 2.5",
            "route.width: expected a whole number, found a number",
        ),
        (
            "[light]\nbands = 1",
            "light.bands: must be between 2 and 16",
        ),
        (
            "[terrain]\nside-shade = 1.5",
            "terrain.side-shade: must be between 0 and 1",
        ),
        (
            "[season]\nautumn = [\"#ffffff\"]",
            "season.autumn: expected a list of 4, found 1 item",
        ),
        (
            "[labels]\nmonths = [\"a\", \"b\"]",
            "labels.months: expected a list of 12, found 2 items",
        ),
        (
            "[labels]\ndate-long = \"{weekdy} {day}\"",
            "labels.date-long: unknown placeholder {weekdy}; this pattern takes {weekday}",
        ),
        (
            "[labels]\ndistance = 4",
            "labels.distance: expected text in quotes, found a number",
        ),
        (
            "[light]\ncolour = [1.0, 0.5]",
            "light.colour: expected [red, green, blue] strengths",
        ),
        (
            "[light]\nuntimed-sun = [400.0, 20.0]",
            "light.untimed-sun: bearing must be between 0 and 360",
        ),
        (
            "[animation]\npause-cut = 0",
            "animation.pause-cut: must be between 1 and 3600",
        ),
        (
            "[sky]\ndither = nan",
            "sky.dither: expected a finite number",
        ),
    ];
    for (text, expected) in cases {
        let message = error(text);
        assert!(message.starts_with(expected), "{text}: {message}");
    }
}

#[test]
fn light_colours_take_strengths_or_hex() {
    let theme = Theme::parse("[light]\ncolour = \"#ff8000\"\nmoon-colour = [2, 1, 0.5]\n").unwrap();
    assert_eq!(
        theme.light.ramp[4].colour,
        Vec3::new(1.0, 128.0 / 255.0, 0.0)
    );
    assert_eq!(theme.light.moon_colour, Vec3::new(2.0, 1.0, 0.5));
}

#[test]
fn numbers_keep_the_value_written() {
    let theme = Theme::parse("[props]\njitter = 0.35\n[route]\nhidden-opacity = 0.3\n").unwrap();
    assert_eq!(theme.slab.props.jitter, 0.35_f32);
    assert_eq!(theme.route.hidden_opacity, 0.3_f32);
    let theme = Theme::parse("[props]\nspacing = 5\n").unwrap();
    assert_eq!(theme.slab.props.spacing_px, 5.0);
}
