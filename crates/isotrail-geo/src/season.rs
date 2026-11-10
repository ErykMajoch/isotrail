use chrono::{Datelike, NaiveDate};
use isotrail_bundle::{SceneBundle, Season, Weather};

use crate::{DaysBefore, WeatherRequest};

/// Nearer the equator than this, the trees keep their summer look
pub const TROPICS_DEG: f64 = 23.5;
/// Fall in temperature per metre of height
pub const LAPSE_C_PER_M: f32 = 0.0065;
/// Snow at least this deep counts as lying
pub const SNOWY_DEPTH_M: f32 = 0.01;
/// Deep snow reaches this far below the lowest height it was read at
const DEEP_SNOW_REACH_M: f32 = 150.0;
const DEEP_SNOW_M: f32 = 0.25;
/// Without lying snow at the heights walked, fresh snow this deep before or
/// during the hike still lies above the freezing level
const FRESH_SNOW_CM: f32 = 1.0;
const SNOW_ABOVE_FREEZING_M: f32 = 300.0;
const SNOW_ABOVE_BARE_M: f32 = 50.0;
/// Frost in the days before the hike (°C·days) that freezes a lake over
pub const ICE_DEGREE_DAYS: f32 = 30.0;
const LINE_RANGE_M: (f32, f32) = (-500.0, 9000.0);
const LINE_STEP_M: f32 = 10.0;

/// A season to show in place of the hike's own
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeasonKind {
    Spring,
    Summer,
    Autumn,
    Winter,
}

/// The snow depth read at a height during the hike
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnowReading {
    pub ele_m: f32,
    pub depth_m: f32,
    pub snowfall_cm: f32,
}

/// The day of the year counted as in the north, half a year on south of
/// the equator; none in the tropics
pub fn leaf_day(date: NaiveDate, lat: f64) -> Option<u16> {
    if lat.abs() < TROPICS_DEG {
        return None;
    }
    let day = date.ordinal() as u16;
    Some(if lat < 0.0 {
        (day - 1 + 182) % 365 + 1
    } else {
        day
    })
}

/// Each hour's snow at the height of the place it was read; the fallback
/// height is for tracks without elevations, whose places the days before
/// don't cover either
pub fn snow_readings(
    weather: &Weather,
    request: &WeatherRequest,
    before: &[DaysBefore],
    fallback_ele_m: f32,
) -> Vec<SnowReading> {
    weather
        .samples
        .iter()
        .zip(&request.hour_locations)
        .map(|(sample, &at)| SnowReading {
            ele_m: request
                .locations
                .get(at)
                .and_then(|l| l.ele_m)
                .map(|ele| ele as f32)
                .or_else(|| before.get(at).map(|d| d.ele_m))
                .unwrap_or(fallback_ele_m),
            depth_m: sample.snow_depth_m,
            snowfall_cm: sample.snowfall_cm,
        })
        .collect()
}

/// The height above which snow lies: just below the lowest snow walked
/// through (further for deep snow), or else, after a fresh fall, a little
/// above the mean freezing level of the days before
pub fn snow_line(readings: &[SnowReading], before: &[DaysBefore]) -> Option<f32> {
    let bare = readings
        .iter()
        .filter(|r| r.depth_m < SNOWY_DEPTH_M)
        .map(|r| r.ele_m)
        .reduce(f32::max);
    let lowest_snow = readings
        .iter()
        .filter(|r| r.depth_m >= SNOWY_DEPTH_M)
        .min_by(|a, b| a.ele_m.total_cmp(&b.ele_m));
    if let Some(snow) = lowest_snow {
        let reach = DEEP_SNOW_REACH_M * (snow.depth_m / DEEP_SNOW_M).min(1.0);
        let line = snow.ele_m - reach;
        return Some(bare.map_or(line, |bare| line.max(bare.min(snow.ele_m))));
    }
    let fresh_cm: f32 = readings.iter().map(|r| r.snowfall_cm).sum::<f32>()
        + before
            .iter()
            .map(|d| d.snowfall_cm.iter().sum::<f32>())
            .fold(0.0, f32::max);
    if fresh_cm < FRESH_SNOW_CM || before.is_empty() {
        return None;
    }
    let freezing = before
        .iter()
        .map(|d| d.ele_m + mean(&d.mean_c) / LAPSE_C_PER_M)
        .sum::<f32>()
        / before.len() as f32;
    let line = (freezing + SNOW_ABOVE_FREEZING_M).max(bare.unwrap_or(f32::MIN) + SNOW_ABOVE_BARE_M);
    (line < LINE_RANGE_M.1).then_some(line.max(LINE_RANGE_M.0))
}

/// The lowest height where the frost of the days before adds up to
/// [`ICE_DEGREE_DAYS`], each day's mean carried up or down by the lapse rate
pub fn ice_line(before: &[DaysBefore]) -> Option<f32> {
    if before.is_empty() {
        return None;
    }
    let frost = |z: f32| {
        before
            .iter()
            .flat_map(|d| {
                d.mean_c
                    .iter()
                    .map(move |t| (LAPSE_C_PER_M * (z - d.ele_m) - t).max(0.0))
            })
            .sum::<f32>()
            / before.len() as f32
    };
    let steps = ((LINE_RANGE_M.1 - LINE_RANGE_M.0) / LINE_STEP_M) as u32;
    (0..=steps)
        .map(|i| LINE_RANGE_M.0 + i as f32 * LINE_STEP_M)
        .find(|&z| frost(z) >= ICE_DEGREE_DAYS)
}

/// A season shown in place of the hike's own: a typical day for the
/// plants, and in winter snow over the whole slab and every lake frozen
pub fn forced_season(kind: SeasonKind, bundle: &SceneBundle) -> Season {
    let day = match kind {
        SeasonKind::Spring => 125,
        SeasonKind::Summer => 200,
        SeasonKind::Autumn => 290,
        SeasonKind::Winter => 15,
    };
    let leaf_day = Some(if bundle.grid.origin_lat < 0.0 {
        (day - 1 + 182) % 365 + 1
    } else {
        day
    });
    if kind != SeasonKind::Winter {
        return Season {
            leaf_day,
            ..Season::default()
        };
    }
    let terrain = &bundle.terrain;
    let low = (0..terrain.heights.len())
        .map(|cell| terrain.height_m(cell))
        .fold(f32::INFINITY, f32::min);
    Season {
        leaf_day,
        snow_line_m: Some(low - 1.0),
        ice_line_m: Some(low - 1.0),
    }
}

fn mean(values: &[f32]) -> f32 {
    values.iter().sum::<f32>() / values.len().max(1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2025, month, day).unwrap()
    }

    fn reading(ele_m: f32, depth_m: f32) -> SnowReading {
        SnowReading {
            ele_m,
            depth_m,
            snowfall_cm: 0.0,
        }
    }

    fn days(ele_m: f32, mean_c: f32, snowfall_cm: f32) -> DaysBefore {
        DaysBefore {
            ele_m,
            mean_c: vec![mean_c; 14],
            snowfall_cm: vec![snowfall_cm; 14],
        }
    }

    #[test]
    fn the_south_is_half_a_year_on_and_the_tropics_have_no_seasons() {
        assert_eq!(leaf_day(date(10, 15), 50.0), Some(288));
        assert_eq!(leaf_day(date(4, 15), -40.0), Some(287));
        assert_eq!(leaf_day(date(12, 31), -40.0), Some(182));
        assert_eq!(leaf_day(date(7, 3), -40.0), Some(1));
        assert_eq!(leaf_day(date(1, 10), 10.0), None);
        assert_eq!(leaf_day(date(1, 10), -20.0), None);
    }

    #[test]
    fn snow_walked_through_reaches_a_little_below() {
        let line = snow_line(&[reading(1200.0, 0.0), reading(1500.0, 0.1)], &[]).unwrap();
        assert_eq!(line, 1440.0);
        let deep = snow_line(&[reading(900.0, 0.5)], &[]).unwrap();
        assert_eq!(deep, 750.0);
        let bare_higher_up = snow_line(&[reading(1000.0, 0.3), reading(1100.0, 0.0)], &[]);
        assert_eq!(bare_higher_up, Some(1000.0));
    }

    #[test]
    fn fresh_snow_lies_above_the_freezing_level() {
        let before = [days(1000.0, -1.3, 0.0), days(1200.0, -2.6, 0.2)];
        let line = snow_line(&[reading(1000.0, 0.0)], &before).unwrap();
        assert!((line - 1100.0).abs() < 0.5, "{line}");
        let above_the_bare_track = snow_line(&[reading(1500.0, 0.0)], &before).unwrap();
        assert_eq!(above_the_bare_track, 1550.0);
    }

    #[test]
    fn no_snow_without_lying_or_fresh_snow_and_summer_snow_only_on_summits() {
        assert_eq!(
            snow_line(&[reading(1000.0, 0.0)], &[days(1000.0, -5.0, 0.0)]),
            None
        );
        assert_eq!(snow_line(&[], &[]), None);
        let summer = [days(1200.0, 12.0, 0.1)];
        let summits = snow_line(&[reading(1200.0, 0.0)], &summer).unwrap();
        assert!((3340.0..3350.0).contains(&summits), "{summits}");
    }

    #[test]
    fn a_fortnight_of_frost_freezes_lakes_from_where_it_adds_up() {
        assert_eq!(ice_line(&[days(500.0, -3.0, 0.0)]), Some(370.0));
        assert_eq!(ice_line(&[days(500.0, 15.0, 0.0)]), Some(3140.0));
        assert_eq!(ice_line(&[days(500.0, 60.0, 0.0)]), None);
        assert_eq!(ice_line(&[]), None);
    }
}
