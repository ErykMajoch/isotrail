use clap::ValueEnum;
use isotrail_bundle::{SceneBundle, Season};
pub use isotrail_geo::without_weather;
use isotrail_geo::{SeasonKind, forced_season};
use isotrail_render::foliage;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum SeasonArg {
    #[default]
    Auto,
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl SeasonArg {
    /// The season to force, or none for the hike's own
    pub fn kind(self) -> Option<SeasonKind> {
        match self {
            SeasonArg::Auto => None,
            SeasonArg::Spring => Some(SeasonKind::Spring),
            SeasonArg::Summer => Some(SeasonKind::Summer),
            SeasonArg::Autumn => Some(SeasonKind::Autumn),
            SeasonArg::Winter => Some(SeasonKind::Winter),
        }
    }
}

/// Shows the season `--season` asks for in place of the hike's own
pub fn apply(bundle: &mut SceneBundle, season: SeasonArg) {
    if let Some(kind) = season.kind() {
        bundle.season = forced_season(kind, bundle);
    }
}

/// One line for the `bundle` report
pub fn describe(season: &Season) -> String {
    let mut parts: Vec<String> = season
        .leaf_day
        .map(foliage)
        .into_iter()
        .map(String::from)
        .collect();
    if let Some(line) = season.snow_line_m {
        parts.push(format!("snow above {line:.0}m"));
    }
    if let Some(line) = season.ice_line_m {
        parts.push(format!("lakes frozen above {line:.0}m"));
    }
    if parts.is_empty() {
        "none".into()
    } else {
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use isotrail_testkit::diorama_bundle;

    use super::*;

    #[test]
    fn the_report_names_what_the_season_shows() {
        assert_eq!(describe(&Season::default()), "none");
        let season = Season {
            leaf_day: Some(290),
            snow_line_m: Some(1649.6),
            ice_line_m: Some(1400.0),
        };
        assert_eq!(
            describe(&season),
            "autumn colours, snow above 1650m, lakes frozen above 1400m"
        );
    }

    #[test]
    fn forcing_a_season_replaces_the_hike_s_own() {
        let mut bundle = diorama_bundle();
        bundle.season.snow_line_m = Some(400.0);
        apply(&mut bundle, SeasonArg::Auto);
        assert_eq!(bundle.season.snow_line_m, Some(400.0));
        apply(&mut bundle, SeasonArg::Autumn);
        assert_eq!(
            bundle.season,
            Season {
                leaf_day: Some(290),
                ..Season::default()
            }
        );
        apply(&mut bundle, SeasonArg::Winter);
        assert_eq!(bundle.season.leaf_day, Some(15));
        assert!(bundle.season.snow_line_m.is_some() && bundle.season.ice_line_m.is_some());
        without_weather(&mut bundle.season);
        assert_eq!(
            bundle.season,
            Season {
                leaf_day: Some(15),
                ..Season::default()
            }
        );
    }
}
