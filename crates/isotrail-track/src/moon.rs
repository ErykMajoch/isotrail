use chrono::{DateTime, Utc};

use crate::sun::{Solar, julian_day, refraction_deg, unix_seconds};

/// Where the moon stands as seen from the ground, and how much of it is lit
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoonPosition {
    /// Compass bearing, clockwise from north
    pub azimuth_deg: f64,
    /// Above the horizon, with parallax and refraction; negative below it
    pub elevation_deg: f64,
    /// Share of the disc in sunlight, from 0 at new moon to 1 at full
    pub illuminated: f64,
}

const SUN_DISTANCE_KM: f64 = 149_597_870.7;
const EARTH_RADIUS_KM: f64 = 6378.14;

/// The moon's position and phase at `time` over a point, from the main terms
/// of Meeus's lunar theory (Astronomical Algorithms, chapters 47 and 48),
/// good to a few tenths of a degree
pub fn moon_position(lat: f64, lon: f64, time: DateTime<Utc>) -> MoonPosition {
    let day = julian_day(unix_seconds(time));
    let moon = Lunar::at(day);
    let sun = Solar::at(day);
    let (lambda, beta) = (
        moon.longitude_deg.to_radians(),
        moon.latitude_deg.to_radians(),
    );
    let obliquity = sun.obliquity_deg.to_radians();
    let right_ascension =
        (lambda.sin() * obliquity.cos() - beta.tan() * obliquity.sin()).atan2(lambda.cos());
    let declination =
        (beta.sin() * obliquity.cos() + beta.cos() * obliquity.sin() * lambda.sin()).asin();

    let t = (day - 2_451_545.0) / 36_525.0;
    let sidereal_deg = 280.46061837
        + 360.98564736629 * (day - 2_451_545.0)
        + t * t * (0.000387933 - t / 38_710_000.0);
    let hour_angle = (sidereal_deg + lon).to_radians() - right_ascension;
    let lat = lat.to_radians();
    let sin_elevation =
        lat.sin() * declination.sin() + lat.cos() * declination.cos() * hour_angle.cos();
    let geocentric = sin_elevation.clamp(-1.0, 1.0).asin().to_degrees();
    let parallax =
        (EARTH_RADIUS_KM / moon.distance_km).asin().to_degrees() * geocentric.to_radians().cos();
    let elevation = geocentric - parallax;
    let azimuth = hour_angle
        .sin()
        .atan2(hour_angle.cos() * lat.sin() - declination.tan() * lat.cos())
        .to_degrees()
        + 180.0;

    let elongation = (beta.cos()
        * (moon.longitude_deg - sun.apparent_longitude_deg)
            .to_radians()
            .cos())
    .clamp(-1.0, 1.0)
    .acos();
    let phase_angle = (SUN_DISTANCE_KM * elongation.sin())
        .atan2(moon.distance_km - SUN_DISTANCE_KM * elongation.cos());
    MoonPosition {
        azimuth_deg: azimuth.rem_euclid(360.0),
        elevation_deg: elevation + refraction_deg(elevation),
        illuminated: (1.0 + phase_angle.cos()) / 2.0,
    }
}

struct Lunar {
    longitude_deg: f64,
    latitude_deg: f64,
    distance_km: f64,
}

/// Multiples of D, M, M' and F, and the coefficient of each term
type Term = ([i8; 4], f64);

const LONGITUDE: [Term; 24] = [
    ([0, 0, 1, 0], 6_288_774.0),
    ([2, 0, -1, 0], 1_274_027.0),
    ([2, 0, 0, 0], 658_314.0),
    ([0, 0, 2, 0], 213_618.0),
    ([0, 1, 0, 0], -185_116.0),
    ([0, 0, 0, 2], -114_332.0),
    ([2, 0, -2, 0], 58_793.0),
    ([2, -1, -1, 0], 57_066.0),
    ([2, 0, 1, 0], 53_322.0),
    ([2, -1, 0, 0], 45_758.0),
    ([0, 1, -1, 0], -40_923.0),
    ([1, 0, 0, 0], -34_720.0),
    ([0, 1, 1, 0], -30_383.0),
    ([2, 0, 0, -2], 15_327.0),
    ([0, 0, 1, 2], -12_528.0),
    ([0, 0, 1, -2], 10_980.0),
    ([4, 0, -1, 0], 10_675.0),
    ([0, 0, 3, 0], 10_034.0),
    ([4, 0, -2, 0], 8_548.0),
    ([2, 1, -1, 0], -7_888.0),
    ([2, 1, 0, 0], -6_766.0),
    ([1, 0, -1, 0], -5_163.0),
    ([1, 1, 0, 0], 4_987.0),
    ([2, -1, 1, 0], 4_036.0),
];

const DISTANCE: [Term; 13] = [
    ([0, 0, 1, 0], -20_905_355.0),
    ([2, 0, -1, 0], -3_699_111.0),
    ([2, 0, 0, 0], -2_955_968.0),
    ([0, 0, 2, 0], -569_925.0),
    ([0, 1, 0, 0], 48_888.0),
    ([0, 0, 0, 2], -3_149.0),
    ([2, 0, -2, 0], 246_158.0),
    ([2, -1, -1, 0], -152_138.0),
    ([2, 0, 1, 0], -170_733.0),
    ([2, -1, 0, 0], -204_586.0),
    ([0, 1, -1, 0], -129_620.0),
    ([1, 0, 0, 0], 108_743.0),
    ([0, 1, 1, 0], 104_755.0),
];

const LATITUDE: [Term; 13] = [
    ([0, 0, 0, 1], 5_128_122.0),
    ([0, 0, 1, 1], 280_602.0),
    ([0, 0, 1, -1], 277_693.0),
    ([2, 0, 0, -1], 173_237.0),
    ([2, 0, -1, 1], 55_413.0),
    ([2, 0, -1, -1], 46_271.0),
    ([2, 0, 0, 1], 32_573.0),
    ([0, 0, 2, 1], 17_198.0),
    ([2, 0, 1, -1], 9_266.0),
    ([0, 0, 2, -1], 8_822.0),
    ([2, -1, 0, -1], 8_216.0),
    ([2, 0, -2, -1], 4_324.0),
    ([2, 0, 1, 1], 4_200.0),
];

impl Lunar {
    fn at(julian_day: f64) -> Lunar {
        let t = (julian_day - 2_451_545.0) / 36_525.0;
        let mean_longitude = 218.3164477 + 481_267.88123421 * t;
        let elongation = 297.8501921 + 445_267.1114034 * t;
        let sun_anomaly = 357.5291092 + 35_999.0502909 * t;
        let moon_anomaly = 134.9633964 + 477_198.8675055 * t;
        let node_distance = 93.2720950 + 483_202.0175233 * t;
        let arguments = [elongation, sun_anomaly, moon_anomaly, node_distance];
        // The Earth's orbit grows rounder, which weakens terms with the Sun's anomaly
        let eccentricity = 1.0 - t * (0.002516 + t * 0.0000074);
        let sum = |terms: &[Term], wave: fn(f64) -> f64| -> f64 {
            terms
                .iter()
                .map(|(multiples, coefficient)| {
                    let angle: f64 = multiples
                        .iter()
                        .zip(arguments)
                        .map(|(&k, argument)| f64::from(k) * argument)
                        .sum();
                    coefficient
                        * eccentricity.powi(i32::from(multiples[1].unsigned_abs()))
                        * wave(angle.to_radians())
                })
                .sum()
        };
        let a1 = (119.75 + 131.849 * t).to_radians();
        let a2 = (53.09 + 479_264.290 * t).to_radians();
        let a3 = (313.45 + 481_266.484 * t).to_radians();
        let (l, f, m) = (
            mean_longitude.to_radians(),
            node_distance.to_radians(),
            moon_anomaly.to_radians(),
        );
        let longitude = sum(&LONGITUDE, f64::sin)
            + 3958.0 * a1.sin()
            + 1962.0 * (l - f).sin()
            + 318.0 * a2.sin();
        let latitude = sum(&LATITUDE, f64::sin) - 2235.0 * l.sin()
            + 382.0 * a3.sin()
            + 175.0 * (a1 - f).sin()
            + 175.0 * (a1 + f).sin()
            + 127.0 * (l - m).sin()
            - 115.0 * (l + m).sin();
        Lunar {
            longitude_deg: (mean_longitude + longitude / 1e6).rem_euclid(360.0),
            latitude_deg: latitude / 1e6,
            distance_km: 385_000.56 + sum(&DISTANCE, f64::cos) / 1000.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn matches_meeus_example_47a() {
        // 1992 April 12, 0h TD: λ 133.162655°, β −3.229126°, Δ 368409.7km
        let moon = Lunar::at(2_448_724.5);
        assert!(
            (moon.longitude_deg - 133.162655).abs() < 0.02,
            "{}",
            moon.longitude_deg
        );
        assert!(
            (moon.latitude_deg + 3.229126).abs() < 0.02,
            "{}",
            moon.latitude_deg
        );
        assert!(
            (moon.distance_km - 368_409.7).abs() < 30.0,
            "{}",
            moon.distance_km
        );
    }

    #[test]
    fn matches_meeus_example_48a() {
        // 1992 April 12, 0h TD: 0.6786 of the disc lit
        let moon = moon_position(0.0, 0.0, utc(1992, 4, 12, 0, 0));
        assert!((moon.illuminated - 0.6786).abs() < 0.005, "{moon:?}");
    }

    #[test]
    fn full_and_new_moons_are_lit_and_dark() {
        // Full moon 2024-04-23 23:49 UTC, new moon (an eclipse) 2024-04-08 18:21
        let full = moon_position(51.5, 0.0, utc(2024, 4, 23, 23, 49));
        assert!(full.illuminated > 0.995, "{full:?}");
        let new = moon_position(51.5, 0.0, utc(2024, 4, 8, 18, 21));
        assert!(new.illuminated < 0.005, "{new:?}");
    }

    #[test]
    fn matches_pyephem_positions() {
        // PyEphem 4.2.1 with refraction: (lat, lon, time, azimuth, elevation)
        let cases = [
            (51.48, 0.0, utc(2024, 4, 24, 0, 0), 181.031, 23.134),
            (0.0, 0.0, utc(1992, 4, 12, 0, 0), 285.039, 22.634),
            (51.5074, -0.1278, utc(2024, 4, 23, 18, 58), 111.989, -0.300),
        ];
        for (lat, lon, time, azimuth, elevation) in cases {
            let moon = moon_position(lat, lon, time);
            assert!((moon.azimuth_deg - azimuth).abs() < 0.3, "{time} {moon:?}");
            assert!(
                (moon.elevation_deg - elevation).abs() < 0.3,
                "{time} {moon:?}"
            );
        }
        let noon = moon_position(51.48, 0.0, utc(2024, 4, 24, 12, 0));
        assert!(noon.elevation_deg < -50.0, "{noon:?}");
    }
}
