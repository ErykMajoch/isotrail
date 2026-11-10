use anyhow::{Context, bail};
use chrono::{NaiveDate, NaiveTime, TimeZone};
use isotrail_bundle::SceneBundle;
use isotrail_render::SunState;
use isotrail_track::{Tz, sun_position, timezone_at};
use tracing::info;

pub fn parse_time(value: &str) -> Result<NaiveTime, String> {
    NaiveTime::parse_from_str(value, "%H:%M")
        .map_err(|_| "expected a local time such as 20:30".to_owned())
}

/// The sun at the start of the hike, or at `time` on the hike's start date in
/// its time zone
pub fn hike_sun(bundle: &SceneBundle, time: Option<NaiveTime>) -> anyhow::Result<SunState> {
    let grid = &bundle.grid;
    let Some(time) = time else {
        if let Some(sun) = bundle.sun.first() {
            info!(
                azimuth = sun.azimuth_deg,
                elevation = sun.elevation_deg,
                "sun at the start"
            );
        }
        return Ok(SunState::for_hike(bundle));
    };
    let Some(date) = bundle.meta.local_date.as_deref() else {
        bail!("--time needs a track with times, to know which day to light");
    };
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .with_context(|| format!("the bundle's date {date:?} is not a date"))?;
    let zone: Tz = bundle
        .meta
        .timezone
        .as_deref()
        .and_then(|name| name.parse().ok())
        .or_else(|| timezone_at(grid.origin_lat, grid.origin_lon))
        .context("could not find the hike's time zone for --time")?;
    let local = date.and_time(time);
    let at = zone
        .from_local_datetime(&local)
        .earliest()
        .with_context(|| {
            format!(
                "{} {} does not exist in {} (the clocks skip it)",
                date,
                time.format("%H:%M"),
                zone.name()
            )
        })?;
    let sun = sun_position(grid.origin_lat, grid.origin_lon, at.to_utc());
    info!(
        azimuth = sun.azimuth_deg,
        elevation = sun.elevation_deg,
        "sun at {}",
        at.format("%Y-%m-%d %H:%M %Z")
    );
    Ok(SunState::at(grid, sun.azimuth_deg, sun.elevation_deg))
}

#[cfg(test)]
mod tests {
    use isotrail_bundle::SunSample;
    use isotrail_testkit::hike_bundle;

    use super::*;

    fn bundle() -> SceneBundle {
        let mut bundle = hike_bundle();
        bundle.grid.origin_lat = 46.5;
        bundle.grid.origin_lon = 10.0;
        bundle.meta.timezone = Some("Europe/Rome".into());
        bundle.sun = vec![SunSample {
            time: 1_749_880_800,
            azimuth_deg: 120.0,
            elevation_deg: 35.0,
        }];
        bundle
    }

    #[test]
    fn times_are_hours_and_minutes() {
        assert_eq!(
            parse_time("20:30"),
            NaiveTime::from_hms_opt(20, 30, 0).ok_or(String::new())
        );
        assert_eq!(
            parse_time("07:05"),
            NaiveTime::from_hms_opt(7, 5, 0).ok_or(String::new())
        );
        for bad in ["24:00", "20:30:15", "20h30", "8pm", "2025-06-14 20:30", ""] {
            assert!(parse_time(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn hikes_are_lit_by_their_start_unless_told_otherwise() {
        let bundle = bundle();
        let start = hike_sun(&bundle, None).unwrap();
        assert_eq!(start, SunState::at(&bundle.grid, 120.0, 35.0));

        // 14 June 2025 in the Alps, CEST: the sun sets around 21:15
        let at = |hhmm| hike_sun(&bundle, Some(parse_time(hhmm).unwrap())).unwrap();
        let expected = |utc_hour, minute| {
            let utc = chrono::Utc
                .with_ymd_and_hms(2025, 6, 14, utc_hour, minute, 0)
                .unwrap();
            let sun = sun_position(46.5, 10.0, utc);
            SunState::at(&bundle.grid, sun.azimuth_deg, sun.elevation_deg)
        };
        assert_eq!(at("20:30"), expected(18, 30));
        assert_eq!(at("13:00"), expected(11, 0));
        assert_ne!(at("20:30"), start);
        let sky = |sun: SunState| sun.sky[0].map(u32::from).iter().sum::<u32>();
        assert!(sky(at("23:00")) < sky(at("13:00")));
    }

    #[test]
    fn times_need_a_date_and_must_exist() {
        let mut untimed = bundle();
        untimed.meta.local_date = None;
        let message = hike_sun(&untimed, parse_time("12:00").ok())
            .unwrap_err()
            .to_string();
        assert!(message.contains("track with times"), "{message}");

        let mut spring = bundle();
        spring.meta.local_date = Some("2025-03-30".into());
        let message = hike_sun(&spring, parse_time("02:30").ok())
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("does not exist in Europe/Rome"),
            "{message}"
        );
        assert!(hike_sun(&spring, parse_time("03:30").ok()).is_ok());
    }
}
