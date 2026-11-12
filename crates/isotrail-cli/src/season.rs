use clap::ValueEnum;
use isotrail_bundle::{SceneBundle, Season};
use isotrail_geo::{
    Fetcher, OpenMeteoSource, SeasonKind, WeatherError, WeatherRequest, WeatherServer,
    WeatherSource, forced_season, ice_line, snow_line, snow_readings,
};
use isotrail_render::foliage;
use isotrail_track::Track;
use tracing::warn;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum SeasonArg {
    #[default]
    Auto,
    Spring,
    Summer,
    Autumn,
    Winter,
}

/// Snow from the weather during the hike, and lakes frozen by the frost of
/// the days before; when the days before can't be found, it warns and
/// leaves the lakes open
pub fn estimate(
    bundle: &mut SceneBundle,
    track: &Track,
    fetcher: impl Fetcher,
    server: &WeatherServer,
) {
    let Some(weather) = &bundle.weather else {
        return;
    };
    let Ok(request) = WeatherRequest::from_track(track, crate::weather::now()) else {
        return;
    };
    let before = OpenMeteoSource::new(fetcher)
        .server(server)
        .days_before(&request)
        .unwrap_or_else(|err| {
            if matches!(err, WeatherError::Offline { .. }) {
                warn!(
                    "the weather before the hike isn't in the cache, so lakes are left open; run once without --offline to add it"
                );
            } else {
                warn!(
                    "can't get the weather before the hike, so lakes are left open: {}",
                    crate::describe(&err.into())
                );
            }
            Vec::new()
        });
    let route = &bundle.route.points;
    let walked = route.iter().map(|p| p.draped_m).sum::<f32>() / route.len().max(1) as f32;
    let readings = snow_readings(weather, &request, &before, walked);
    let terrain = &bundle.terrain;
    let top = (0..terrain.heights.len())
        .map(|cell| terrain.height_m(cell))
        .fold(f32::MIN, f32::max);
    let highest_lake = bundle
        .water
        .iter()
        .filter(|body| !body.sea)
        .map(|body| body.level_m)
        .fold(f32::MIN, f32::max);
    bundle.season.snow_line_m = snow_line(&readings, &before).filter(|&line| line < top);
    bundle.season.ice_line_m = ice_line(&before).filter(|&line| line <= highest_lake);
}

/// Shows the season `--season` asks for in place of the hike's own
pub fn apply(bundle: &mut SceneBundle, season: SeasonArg) {
    let kind = match season {
        SeasonArg::Auto => return,
        SeasonArg::Spring => SeasonKind::Spring,
        SeasonArg::Summer => SeasonKind::Summer,
        SeasonArg::Autumn => SeasonKind::Autumn,
        SeasonArg::Winter => SeasonKind::Winter,
    };
    bundle.season = forced_season(kind, bundle);
}

/// Snow and ice come from the weather, so they go with it
pub fn without_weather(season: &mut Season) {
    season.snow_line_m = None;
    season.ice_line_m = None;
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
        parts.push(format!("snow above {line:.0} m"));
    }
    if let Some(line) = season.ice_line_m {
        parts.push(format!("lakes frozen above {line:.0} m"));
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
            "autumn colours, snow above 1650 m, lakes frozen above 1400 m"
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
