use anyhow::bail;
use clap::{Args, ValueEnum};
use isotrail_bundle::SceneBundle;
use isotrail_render::{
    DateFormat, OverlayStyle, OverlayText, PanelStyle, StatKind, Subtitle, TitleSize,
};
use tracing::warn;

use crate::UnitsArg;

/// What the render prints over the diorama
#[derive(Debug, Args)]
pub struct OverlayArgs {
    /// Text to print over the render, or none for the diorama alone
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        value_name = "PARTS",
        default_values_t = [Part::Title, Part::Date, Part::Stats]
    )]
    overlay: Vec<Part>,

    /// Put the text on frosted panels, or with none straight on the scene
    /// with a dark outline
    #[arg(long, value_enum, default_value_t = PanelArg::Frosted)]
    panel: PanelArg,

    /// Title to print instead of the hike's name
    #[arg(long, value_name = "TEXT")]
    title: Option<String>,

    /// Print the title large, or small like the subtitle and date
    #[arg(long, value_enum, default_value_t = TitleSizeArg::Large)]
    title_size: TitleSizeArg,

    /// Line to print under the title; by default, a name too long for two
    /// lines is split there at its first " - " or " | ", and none never
    /// splits it
    #[arg(long, value_name = "TEXT|none")]
    subtitle: Option<String>,

    /// Text to print instead of the hike's date
    #[arg(long, value_name = "TEXT", conflicts_with = "date_format")]
    date: Option<String>,

    /// How to print the date: long (Saturday 14 June 2025), short
    /// (14 Jun 2025) or iso (2025-06-14)
    #[arg(long, value_enum, default_value_t = DateFormatArg::Long)]
    date_format: DateFormatArg,

    /// Stats to print, in this order; time is the moving time, and speed and
    /// pace are averaged over it
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        value_name = "STATS",
        default_values_t = [StatArg::Distance, StatArg::Gain, StatArg::Time, StatArg::High]
    )]
    stats: Vec<StatArg>,

    /// Units for the stats
    #[arg(long, value_enum, default_value_t = UnitsArg::Metric)]
    units: UnitsArg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Part {
    Title,
    Date,
    Stats,
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

impl OverlayArgs {
    pub fn style(&self) -> OverlayStyle {
        OverlayStyle {
            panel: match self.panel {
                PanelArg::Frosted => PanelStyle::Frosted,
                PanelArg::None => PanelStyle::None,
            },
            title_size: match self.title_size {
                TitleSizeArg::Large => TitleSize::Large,
                TitleSizeArg::Small => TitleSize::Small,
            },
        }
    }

    /// The bundle's text with the flags applied, leaving out the parts that
    /// weren't asked for
    pub fn text(&self, bundle: &SceneBundle) -> anyhow::Result<OverlayText> {
        if self.overlay.contains(&Part::None) && self.overlay.len() > 1 {
            bail!("--overlay none can't be combined with other parts");
        }
        for (i, stat) in self.stats.iter().enumerate() {
            if self.stats[..i].contains(stat) {
                bail!("--stats lists {} twice", name(*stat));
            }
        }
        let show = |part| self.overlay.contains(&part);
        let kinds: Vec<StatKind> = self.stats.iter().map(|&stat| stat.into()).collect();
        let date_format = match self.date_format {
            DateFormatArg::Long => DateFormat::Long,
            DateFormatArg::Short => DateFormat::Short,
            DateFormatArg::Iso => DateFormat::Iso,
        };
        let units = self.units.into();
        let mut text = OverlayText::from_bundle(bundle, units, &kinds, date_format);

        if let Some(title) = &self.title {
            text.title.clone_from(title);
        }
        text.subtitle = match self.subtitle.as_deref() {
            None => Subtitle::Auto,
            Some(none) if none.eq_ignore_ascii_case("none") => Subtitle::None,
            Some(subtitle) => Subtitle::Text(subtitle.to_owned()),
        };
        if let Some(date) = &self.date {
            text.date = Some(date.clone());
        }

        if !show(Part::Title) {
            text.title.clear();
        }
        if !show(Part::Date) {
            text.date = None;
        }
        if show(Part::Stats) {
            for &stat in &self.stats {
                if StatKind::from(stat)
                    .stat(&bundle.meta.stats, units)
                    .is_none()
                {
                    warn!("the track has no {} to show", name(stat));
                }
            }
        } else {
            text.stats.clear();
        }
        Ok(text)
    }
}

fn name(stat: StatArg) -> String {
    stat.to_possible_value()
        .map_or_else(|| format!("{stat:?}"), |value| value.get_name().to_owned())
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use isotrail_testkit::diorama_bundle;

    use super::*;

    #[derive(Debug, Parser)]
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
        let stats = &mut bundle.meta.stats;
        (stats.distance_m, stats.gain_m) = (7_420.0, Some(612.0));
        (stats.moving_s, stats.max_ele_m) = (Some(9_000), Some(842.0));
        bundle
    }

    #[test]
    fn defaults_print_everything() {
        let args = parse(&[]).unwrap();
        let text = args.text(&bundle()).unwrap();
        assert_eq!(text.title, "Ridge | Valley");
        assert_eq!(text.subtitle, Subtitle::Auto);
        assert_eq!(text.date.as_deref(), Some("Saturday 14 June 2025"));
        let labels: Vec<&str> = text.stats.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["distance", "gain", "time", "high point"]);
        assert_eq!(args.style(), OverlayStyle::default());
    }

    #[test]
    fn overlay_picks_the_parts() {
        let text = parse(&["--overlay", "stats"])
            .unwrap()
            .text(&bundle())
            .unwrap();
        assert!(text.title.is_empty() && text.date.is_none());
        assert_eq!(text.stats.len(), 4);
        let text = parse(&["--overlay", "title,date"])
            .unwrap()
            .text(&bundle())
            .unwrap();
        assert!(!text.title.is_empty() && text.date.is_some() && text.stats.is_empty());
        let text = parse(&["--overlay", "none"])
            .unwrap()
            .text(&bundle())
            .unwrap();
        assert!(text.title.is_empty() && text.date.is_none() && text.stats.is_empty());
    }

    #[test]
    fn none_stands_alone() {
        let message = parse(&["--overlay", "none,title"])
            .unwrap()
            .text(&bundle())
            .unwrap_err()
            .to_string();
        assert!(message.contains("--overlay none"), "{message}");
    }

    #[test]
    fn stats_are_chosen_in_order_once_each() {
        let text = parse(&["--stats", "pace,low,distance"])
            .unwrap()
            .text(&bundle())
            .unwrap();
        // The bundle has no low point, so it's left out
        let labels: Vec<&str> = text.stats.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["pace", "distance"]);
        let message = parse(&["--stats", "gain,time,gain"])
            .unwrap()
            .text(&bundle())
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
        let text = args.text(&bundle()).unwrap();
        assert_eq!(text.title, "Łańcuch");
        assert_eq!(text.subtitle, Subtitle::Text("Tatry".into()));
        assert_eq!(text.date.as_deref(), Some("Summer 2025"));
        let text = parse(&["--subtitle", "None"])
            .unwrap()
            .text(&bundle())
            .unwrap();
        assert_eq!(text.subtitle, Subtitle::None);
        let text = parse(&["--date-format", "iso"])
            .unwrap()
            .text(&bundle())
            .unwrap();
        assert_eq!(text.date.as_deref(), Some("2025-06-14"));
        assert!(parse(&["--date", "May", "--date-format", "short"]).is_err());
    }

    #[test]
    fn style_flags_map_to_the_style() {
        let args = parse(&["--panel", "none", "--title-size", "small"]).unwrap();
        assert_eq!(
            args.style(),
            OverlayStyle {
                panel: PanelStyle::None,
                title_size: TitleSize::Small,
            }
        );
    }
}
