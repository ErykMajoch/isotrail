use std::path::Path;

use clap::{Args, ValueEnum};
use isotrail_bundle::SceneBundle;
use isotrail_render::{
    DateFormat, DontFit, FitStep, Fitted, Framing, Labels, Layout, OverlayOptions, OverlayPart,
    OverlayStyle, OverlayText, PanelStyle, StatKind, Subtitle, TextError, TextPart, TextWarning,
    Theme, TitleSize,
};
use tracing::warn;

use crate::{UnitsArg, usage};

/// What the render prints over the diorama
#[derive(Debug, Args)]
#[command(next_help_heading = crate::TEXT)]
pub struct OverlayArgs {
    /// Parts to print over the diorama: title, date, stats and profile, or none
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        value_name = "LIST|none",
        default_value = "title,date,stats,profile",
        hide_possible_values = true
    )]
    overlay: Vec<Part>,

    /// Leave the data credit off the image, and print it for the caption
    #[arg(long)]
    no_attribution: bool,

    /// Text on frosted panels, or straight on the scene with a dark outline
    #[arg(long, value_enum, value_name = "frosted|none", default_value_t = PanelArg::Frosted,
          hide_possible_values = true)]
    panel: PanelArg,

    /// Title to print instead of the hike's name
    #[arg(long, value_name = "TEXT")]
    title: Option<String>,

    /// Print the title large, or small like the date
    #[arg(long, value_enum, value_name = "large|small", default_value_t = TitleSizeArg::Large,
          hide_possible_values = true)]
    title_size: TitleSizeArg,

    /// Line under the title, or none to never split a long title into two
    #[arg(long, value_name = "TEXT|none")]
    subtitle: Option<String>,

    /// Text to print instead of the hike's date
    #[arg(long, value_name = "TEXT", conflicts_with = "date_format")]
    date: Option<String>,

    /// The date as Saturday 14 June 2025 (long), 14 Jun 2025 (short) or 2025-06-14
    #[arg(long, value_enum, value_name = "long|short|iso", default_value_t = DateFormatArg::Long,
          hide_possible_values = true)]
    date_format: DateFormatArg,

    /// Stats to print, in this order; time is the moving time
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        value_name = "LIST",
        default_value = "distance,gain,time,high"
    )]
    stats: Vec<StatArg>,

    /// Units for the stats
    #[arg(long, value_enum, value_name = "metric|imperial", default_value_t = UnitsArg::Metric,
          hide_possible_values = true)]
    units: UnitsArg,

    /// Where the text goes: chosen by the image's shape (auto), or one layout for all
    #[arg(long, value_name = "auto|stacked|column", default_value = "auto",
          value_parser = parse_layout)]
    layout: Layout,

    /// A line of your own under the data credit, such as your name or site
    #[arg(long, value_name = "TEXT")]
    credit_extra: Option<String>,
}

fn parse_layout(value: &str) -> Result<Layout, String> {
    value.parse()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Part {
    Title,
    Date,
    Stats,
    Profile,
    None,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PanelArg {
    Frosted,
    None,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TitleSizeArg {
    Large,
    Small,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum DateFormatArg {
    Long,
    Short,
    Iso,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum StatArg {
    Distance,
    Gain,
    Loss,
    Time,
    Elapsed,
    High,
    Low,
    Speed,
    Pace,
}

impl From<StatArg> for StatKind {
    fn from(stat: StatArg) -> Self {
        match stat {
            StatArg::Distance => StatKind::Distance,
            StatArg::Gain => StatKind::Gain,
            StatArg::Loss => StatKind::Loss,
            StatArg::Time => StatKind::Time,
            StatArg::Elapsed => StatKind::Elapsed,
            StatArg::High => StatKind::High,
            StatArg::Low => StatKind::Low,
            StatArg::Speed => StatKind::Speed,
            StatArg::Pace => StatKind::Pace,
        }
    }
}

impl From<Part> for OverlayPart {
    fn from(part: Part) -> Self {
        match part {
            Part::Title => OverlayPart::Title,
            Part::Date => OverlayPart::Date,
            Part::Stats => OverlayPart::Stats,
            Part::Profile => OverlayPart::Profile,
            Part::None => OverlayPart::None,
        }
    }
}

impl OverlayArgs {
    /// The flags as the renderer's options
    pub fn options(&self) -> OverlayOptions {
        OverlayOptions {
            parts: self.overlay.iter().map(|&part| part.into()).collect(),
            attribution: !self.no_attribution,
            panel: match self.panel {
                PanelArg::Frosted => PanelStyle::Frosted,
                PanelArg::None => PanelStyle::None,
            },
            title: self.title.clone(),
            title_size: match self.title_size {
                TitleSizeArg::Large => TitleSize::Large,
                TitleSizeArg::Small => TitleSize::Small,
            },
            subtitle: match self.subtitle.as_deref() {
                None => Subtitle::Auto,
                Some(none) if none.eq_ignore_ascii_case("none") => Subtitle::None,
                Some(subtitle) => Subtitle::Text(subtitle.to_owned()),
            },
            date: self.date.clone(),
            date_format: match self.date_format {
                DateFormatArg::Long => DateFormat::Long,
                DateFormatArg::Short => DateFormat::Short,
                DateFormatArg::Iso => DateFormat::Iso,
            },
            stats: self.stats.iter().map(|&stat| stat.into()).collect(),
            units: self.units.into(),
            layout: self.layout,
            credit_extra: self.credit_extra.clone(),
        }
    }

    /// The overlays' look in `theme`'s colours, with the profile walked in
    /// `route_colour`
    pub fn style(&self, theme: &Theme, route_colour: [u8; 3]) -> OverlayStyle {
        self.options().style(&theme.overlay, route_colour)
    }

    /// `--credit-extra` as the embed's attribute, so its live view shows the
    /// line too
    pub fn attributes(&self) -> Vec<(&'static str, String)> {
        self.credit_extra()
            .map(|extra| ("credit-extra", extra.to_owned()))
            .into_iter()
            .collect()
    }

    pub fn credit_extra(&self) -> Option<&str> {
        self.credit_extra
            .as_deref()
            .map(str::trim)
            .filter(|extra| !extra.is_empty())
    }

    /// Checks the lists of parts and stats, before any work
    pub fn check(&self) -> anyhow::Result<()> {
        self.options().check().map_err(worded)
    }

    /// The bundle's text in the words of `labels` with the flags applied,
    /// leaving out the parts that weren't asked for
    pub fn text(&self, bundle: &SceneBundle, labels: &Labels) -> anyhow::Result<OverlayText> {
        let composed = self.options().text(bundle, labels).map_err(worded)?;
        for warning in &composed.warnings {
            match warning {
                TextWarning::NoStat(stat) => {
                    warn!("the track has no {} to show", stat.name());
                }
                TextWarning::Undrawable { part, missing } => {
                    warn!("{}", undrawable(part_name(*part), missing));
                }
            }
        }
        Ok(composed.text)
    }

    /// The text in isotrail's own words when a theme changes them, so text
    /// that doesn't fit can be blamed on the theme's words
    pub fn built_in_text(&self, bundle: &SceneBundle, labels: &Labels) -> Option<OverlayText> {
        self.options().built_in_text(bundle, labels)
    }

    /// The overlays laid out for a frame, with a warning if they had to give
    /// something up to fit, or an error if they don't fit at all. `themed`
    /// holds the text in isotrail's own words and the theme file whose
    /// words replaced them, to say when only the theme's words don't fit
    pub fn fit(
        &self,
        text: &OverlayText,
        themed: Option<(&OverlayText, &Path)>,
        shape: &str,
        framing: &Framing,
        scale: u32,
        style: &OverlayStyle,
    ) -> anyhow::Result<Fitted> {
        let built_in = themed.map(|(built_in, _)| built_in);
        let fit = OverlayOptions::fit(text, built_in, style, framing).map_err(|dont| {
            match (dont, themed) {
                (DontFit::ThemeWords, Some((_, theme))) => usage(format!(
                    "the words in the [labels] table of the theme {} are too long for the \
                     {shape} image at --scale {scale}; shorten them or try a lower --scale",
                    theme.display()
                )),
                _ => usage(format!(
                    "the text doesn't fit the {shape} image at --scale {scale}; try a lower \
                     --scale, a larger --size, fewer --stats or --overlay none"
                )),
            }
        })?;
        if !fit.steps.is_empty() {
            let steps: Vec<String> = fit.steps.iter().map(describe).collect();
            warn!(
                "the text didn't fit the {shape} image at --scale {scale}, so it has {}",
                and_list(&steps)
            );
        }
        Ok(fit)
    }

    /// `--title`, when given
    pub fn given_title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The hike's name, as `--title` gives it or the bundle has it
    pub fn title<'a>(&'a self, bundle: &'a SceneBundle) -> &'a str {
        self.title.as_deref().unwrap_or(&bundle.meta.title)
    }

    /// The data credit to put in the caption, when the image leaves it out,
    /// as asked or because it didn't fit
    pub fn caption_credit(&self, bundle: &SceneBundle, dropped: bool) -> Option<String> {
        self.options().caption_credit(bundle, dropped)
    }
}

/// A mistake in the lists of parts and stats, in the flags' words
fn worded(error: TextError) -> anyhow::Error {
    match error {
        TextError::NoneWithOthers => usage("--overlay none can't be combined with other parts"),
        TextError::StatTwice(stat) => usage(format!("--stats lists {} twice", stat.name())),
    }
}

fn part_name(part: TextPart) -> &'static str {
    match part {
        TextPart::Title => "title",
        TextPart::Subtitle => "subtitle",
        TextPart::Date => "date",
        TextPart::Stats => "stats",
        TextPart::CreditLine => "credit line",
    }
}

fn describe(step: &FitStep) -> String {
    match step {
        FitStep::ShortDate => "the short date".into(),
        FitStep::SmallTitle => "a small title".into(),
        FitStep::NoSubtitle => "no subtitle".into(),
        FitStep::TightStats => "the stats closer together".into(),
        FitStep::NoProfile => "no profile".into(),
        FitStep::NoCredit => "no credit (it is printed below, for the caption)".into(),
        FitStep::LeftOutStats(labels) => {
            let noun = if labels.len() == 1 { "stat" } else { "stats" };
            format!("no {} {noun}", and_list(labels))
        }
    }
}

/// The warning for characters the font lacks in one part of the text
fn undrawable(field: &str, missing: &[char]) -> String {
    let missing: Vec<String> = missing.iter().map(|c| format!("{c:?}")).collect();
    let has = if field.ends_with('s') { "have" } else { "has" };
    let (noun, were) = if missing.len() == 1 {
        ("a character", "it was")
    } else {
        ("characters", "they were")
    };
    format!(
        "the {field} {has} {noun} the font can't draw, so {were} left out: {}",
        missing.join(" ")
    )
}

/// `a, b and c`
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use isotrail_bundle::ProfileSample;
    use isotrail_testkit::{diorama_bundle, synthetic_weather};

    use isotrail_render::{Aspect, ROUTE_COLOUR, Stat};

    use super::*;

    #[derive(Debug, Parser)]
    #[command(allow_negative_numbers = true)]
    struct Cli {
        #[command(flatten)]
        overlay: OverlayArgs,
    }

    fn parse(args: &[&str]) -> Result<OverlayArgs, clap::Error> {
        Cli::try_parse_from(std::iter::once("isotrail").chain(args.iter().copied()))
            .map(|cli| cli.overlay)
    }

    fn bundle() -> SceneBundle {
        let mut bundle = diorama_bundle();
        bundle.meta.title = "Ridge | Valley".into();
        bundle.meta.local_date = Some("2025-06-14".into());
        bundle.meta.attribution = vec!["© Mappers".into(), "Tiles".into()];
        bundle.profile = (0..4)
            .map(|i| ProfileSample {
                distance_m: i as f32 * 100.0,
                ele_m: 400.0 + i as f32,
            })
            .collect();
        let stats = &mut bundle.meta.stats;
        (stats.distance_m, stats.gain_m) = (7_420.0, Some(612.0));
        (stats.moving_s, stats.max_ele_m) = (Some(9_000), Some(842.0));
        bundle
    }

    #[test]
    fn defaults_print_everything() {
        let args = parse(&[]).unwrap();
        let text = args.text(&bundle(), Labels::built_in()).unwrap();
        assert_eq!(text.title, "Ridge | Valley");
        assert_eq!(text.subtitle, Subtitle::Auto);
        assert_eq!(text.date.as_deref(), Some("Saturday 14 June 2025"));
        let labels: Vec<&str> = text.stats.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["distance", "gain", "time", "high point"]);
        assert_eq!(text.profile, [400.0, 401.0, 402.0, 403.0]);
        assert_eq!(text.attribution.as_deref(), Some("© Mappers · Tiles"));
        assert_eq!(args.caption_credit(&bundle(), false), None);
        assert_eq!(
            args.style(&Theme::default(), ROUTE_COLOUR),
            OverlayStyle::default()
        );
    }

    #[test]
    fn the_credit_moves_to_the_caption_when_left_off() {
        for flags in [&["--no-attribution"][..], &["--overlay", "none"][..]] {
            let args = parse(flags).unwrap();
            assert_eq!(
                args.text(&bundle(), Labels::built_in())
                    .unwrap()
                    .attribution,
                None
            );
            assert_eq!(
                args.caption_credit(&bundle(), false).as_deref(),
                Some("© Mappers · Tiles")
            );
        }
        let args = parse(&["--overlay", "title"]).unwrap();
        assert!(
            args.text(&bundle(), Labels::built_in())
                .unwrap()
                .attribution
                .is_some()
        );
        let mut bare = bundle();
        bare.meta.attribution.clear();
        let args = parse(&["--no-attribution"]).unwrap();
        assert_eq!(args.caption_credit(&bare, false), None);
        let mut with_weather = bundle();
        with_weather.weather = Some(synthetic_weather(1_750_064_400, 3));
        assert_eq!(
            args.caption_credit(&with_weather, false).as_deref(),
            Some("© Mappers · Tiles · Weather data by Open-Meteo.com")
        );
        let drawn = parse(&[])
            .unwrap()
            .text(&with_weather, Labels::built_in())
            .unwrap()
            .attribution;
        assert_eq!(drawn.as_deref(), Some("© Mappers · Tiles · Open-Meteo.com"));
        // Or when the frame had no room for it
        assert_eq!(
            parse(&[])
                .unwrap()
                .caption_credit(&bundle(), true)
                .as_deref(),
            Some("© Mappers · Tiles")
        );
    }

    #[test]
    fn a_coarse_frame_gives_parts_up_or_is_refused() {
        let args = parse(&["--stats", "distance,gain,time,high,low,speed"]).unwrap();
        let mut bundle = bundle();
        bundle.meta.stats.min_ele_m = Some(300.0);
        let text = args.text(&bundle, Labels::built_in()).unwrap();
        assert_eq!(text.short_date.as_deref(), Some("14 Jun 2025"));
        let fit = |aspect: Aspect, scale| {
            args.fit(
                &text,
                None,
                aspect.name(),
                &aspect.framing(scale).unwrap(),
                scale,
                &OverlayStyle::default(),
            )
        };
        assert_eq!(fit(Aspect::Story, 3).unwrap().steps, []);
        let coarse = fit(Aspect::Story, 10).unwrap();
        assert!(coarse.credit_dropped());
        assert!(matches!(
            coarse.steps.last(),
            Some(FitStep::LeftOutStats(_))
        ));
        // Nothing gives way for a stat wider than the frame
        let mut long = text.clone();
        long.stats[0] = Stat::new("123456789012345 km", "distance");
        let framing = Aspect::Square.framing(10).unwrap();
        let refused = args
            .fit(
                &long,
                None,
                "square",
                &framing,
                10,
                &OverlayStyle::default(),
            )
            .unwrap_err();
        assert!(refused.is::<crate::UsageError>());
        let message = refused.to_string();
        assert!(
            message.contains("the square image at --scale 10"),
            "{message}"
        );
        assert!(message.contains("--stats"), "{message}");
        // A date given as text has no shorter form
        let given = parse(&["--date", "Summer 2025"]).unwrap();
        assert_eq!(
            given.text(&bundle, Labels::built_in()).unwrap().short_date,
            None
        );
    }

    #[test]
    fn steps_read_as_a_list() {
        let steps = [
            FitStep::NoProfile,
            FitStep::LeftOutStats(vec!["time".into(), "high point".into()]),
        ];
        let described: Vec<String> = steps.iter().map(describe).collect();
        assert_eq!(
            and_list(&described),
            "no profile and no time and high point stats"
        );
        assert_eq!(
            undrawable("stats", &['✓']),
            "the stats have a character the font can't draw, so it was left out: '✓'"
        );
        assert_eq!(
            undrawable("title", &['😀', '✓']),
            "the title has characters the font can't draw, so they were left out: '😀' '✓'"
        );
        assert_eq!(
            describe(&FitStep::LeftOutStats(vec!["speed".into()])),
            "no speed stat"
        );
    }

    #[test]
    fn overlay_picks_the_parts() {
        let text = parse(&["--overlay", "stats"])
            .unwrap()
            .text(&bundle(), Labels::built_in())
            .unwrap();
        assert!(text.title.is_empty() && text.date.is_none());
        assert_eq!(text.stats.len(), 4);
        let text = parse(&["--overlay", "title,date"])
            .unwrap()
            .text(&bundle(), Labels::built_in())
            .unwrap();
        assert!(!text.title.is_empty() && text.date.is_some() && text.stats.is_empty());
        assert!(text.profile.is_empty());
        let text = parse(&["--overlay", "profile"])
            .unwrap()
            .text(&bundle(), Labels::built_in())
            .unwrap();
        assert!(text.stats.is_empty() && text.profile.len() == 4);
        let text = parse(&["--overlay", "none"])
            .unwrap()
            .text(&bundle(), Labels::built_in())
            .unwrap();
        assert!(text.title.is_empty() && text.date.is_none() && text.stats.is_empty());
        assert!(text.profile.is_empty());
    }

    #[test]
    fn none_stands_alone() {
        let message = parse(&["--overlay", "none,title"])
            .unwrap()
            .text(&bundle(), Labels::built_in())
            .unwrap_err()
            .to_string();
        assert!(message.contains("--overlay none"), "{message}");
    }

    #[test]
    fn stats_are_chosen_in_order_once_each() {
        let text = parse(&["--stats", "pace,low,distance"])
            .unwrap()
            .text(&bundle(), Labels::built_in())
            .unwrap();
        // The bundle has no low point, so it's left out
        let labels: Vec<&str> = text.stats.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["pace", "distance"]);
        let message = parse(&["--stats", "gain,time,gain"])
            .unwrap()
            .text(&bundle(), Labels::built_in())
            .unwrap_err()
            .to_string();
        assert!(message.contains("gain twice"), "{message}");
        assert!(parse(&["--stats", "steps"]).is_err());
    }

    #[test]
    fn title_subtitle_and_date_can_be_replaced() {
        let args = parse(&[
            "--title",
            "Łańcuch",
            "--subtitle",
            "Tatry",
            "--date",
            "Summer 2025",
        ])
        .unwrap();
        let text = args.text(&bundle(), Labels::built_in()).unwrap();
        assert_eq!(text.title, "Łańcuch");
        assert_eq!(text.subtitle, Subtitle::Text("Tatry".into()));
        assert_eq!(text.date.as_deref(), Some("Summer 2025"));
        let text = parse(&["--subtitle", "None"])
            .unwrap()
            .text(&bundle(), Labels::built_in())
            .unwrap();
        assert_eq!(text.subtitle, Subtitle::None);
        let text = parse(&["--date-format", "iso"])
            .unwrap()
            .text(&bundle(), Labels::built_in())
            .unwrap();
        assert_eq!(text.date.as_deref(), Some("2025-06-14"));
        assert!(parse(&["--date", "May", "--date-format", "short"]).is_err());
    }

    #[test]
    fn stats_are_named_as_typed() {
        for stat in StatArg::value_variants() {
            let typed = stat.to_possible_value().unwrap();
            assert_eq!(typed.get_name(), StatKind::from(*stat).name());
        }
    }

    #[test]
    fn style_flags_map_to_the_style() {
        let args = parse(&["--panel", "none", "--title-size", "small"]).unwrap();
        assert_eq!(
            args.style(&Theme::default(), ROUTE_COLOUR),
            OverlayStyle {
                panel: PanelStyle::None,
                title_size: TitleSize::Small,
                ..OverlayStyle::default()
            }
        );
    }
}
