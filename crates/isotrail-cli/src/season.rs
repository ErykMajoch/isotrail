use isotrail_bundle::Season;
use isotrail_render::foliage;

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
}
