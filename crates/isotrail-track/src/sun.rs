use chrono::{DateTime, Utc};

/// Where the sun stands as seen from the ground, with atmospheric refraction
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunPosition {
    /// Compass bearing, clockwise from north
    pub azimuth_deg: f64,
    /// Above the horizon; negative below it
    pub elevation_deg: f64,
}

/// The sun's position at `time` over a point, by NOAA's solar calculator
/// equations (Meeus, Astronomical Algorithms)
pub fn sun_position(lat: f64, lon: f64, time: DateTime<Utc>) -> SunPosition {
    let unix_s = unix_seconds(time);
    let sun = Solar::at(julian_day(unix_s));
    let day_minutes = unix_s.rem_euclid(86_400.0) / 60.0;
    let solar_minutes = day_minutes + sun.equation_of_time_min + 4.0 * lon;
    let hour_angle = (solar_minutes / 4.0 - 180.0).to_radians();
    let (lat, decl) = (lat.to_radians(), sun.declination_deg.to_radians());
    let cos_zenith = lat.sin() * decl.sin() + lat.cos() * decl.cos() * hour_angle.cos();
    let elevation = 90.0 - cos_zenith.clamp(-1.0, 1.0).acos().to_degrees();
    let azimuth = hour_angle
        .sin()
        .atan2(hour_angle.cos() * lat.sin() - decl.tan() * lat.cos())
        .to_degrees()
        + 180.0;
    SunPosition {
        azimuth_deg: azimuth.rem_euclid(360.0),
        elevation_deg: elevation + refraction_deg(elevation),
    }
}

pub(crate) fn unix_seconds(time: DateTime<Utc>) -> f64 {
    time.timestamp() as f64 + f64::from(time.timestamp_subsec_millis()) / 1000.0
}

pub(crate) fn julian_day(unix_s: f64) -> f64 {
    unix_s / 86_400.0 + 2_440_587.5
}

pub(crate) struct Solar {
    pub declination_deg: f64,
    pub equation_of_time_min: f64,
    pub apparent_longitude_deg: f64,
    pub obliquity_deg: f64,
}

impl Solar {
    pub fn at(julian_day: f64) -> Solar {
        let t = (julian_day - 2_451_545.0) / 36_525.0;
        let mean_longitude = (280.46646 + t * (36000.76983 + t * 0.0003032)).rem_euclid(360.0);
        let mean_anomaly = 357.52911 + t * (35999.05029 - t * 0.0001537);
        let eccentricity = 0.016708634 - t * (0.000042037 + t * 0.0000001267);
        let m = mean_anomaly.to_radians();
        let centre = m.sin() * (1.914602 - t * (0.004817 + t * 0.000014))
            + (2.0 * m).sin() * (0.019993 - t * 0.000101)
            + (3.0 * m).sin() * 0.000289;
        let omega = (125.04 - 1934.136 * t).to_radians();
        let apparent_longitude = mean_longitude + centre - 0.00569 - 0.00478 * omega.sin();
        let mean_obliquity =
            23.0 + (26.0 + (21.448 - t * (46.815 + t * (0.00059 - t * 0.001813))) / 60.0) / 60.0;
        let obliquity = (mean_obliquity + 0.00256 * omega.cos()).to_radians();
        let declination = (obliquity.sin() * apparent_longitude.to_radians().sin()).asin();
        let y = (obliquity / 2.0).tan().powi(2);
        let l0 = mean_longitude.to_radians();
        let e = eccentricity;
        let equation_of_time = y * (2.0 * l0).sin() - 2.0 * e * m.sin()
            + 4.0 * e * y * m.sin() * (2.0 * l0).cos()
            - 0.5 * y * y * (4.0 * l0).sin()
            - 1.25 * e * e * (2.0 * m).sin();
        Solar {
            declination_deg: declination.to_degrees(),
            equation_of_time_min: 4.0 * equation_of_time.to_degrees(),
            apparent_longitude_deg: apparent_longitude,
            obliquity_deg: obliquity.to_degrees(),
        }
    }
}

/// NOAA's approximation for a standard atmosphere
pub(crate) fn refraction_deg(elevation_deg: f64) -> f64 {
    let e = elevation_deg;
    let tan = e.to_radians().tan();
    let arcsec = if e > 85.0 {
        0.0
    } else if e > 5.0 {
        58.1 / tan - 0.07 / tan.powi(3) + 0.000086 / tan.powi(5)
    } else if e > -0.575 {
        1735.0 + e * (-518.2 + e * (103.4 + e * (-12.79 + e * 0.711)))
    } else {
        -20.772 / tan
    };
    arcsec / 3600.0
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
    }

    fn day(time: DateTime<Utc>) -> f64 {
        julian_day(unix_seconds(time))
    }

    #[test]
    fn matches_meeus_example_25a() {
        // 1992 October 13.0 TD: declination −7.78507°
        let sun = Solar::at(day(utc(1992, 10, 13, 0, 0, 0)));
        assert!(
            (sun.declination_deg + 7.78507).abs() < 0.002,
            "{}",
            sun.declination_deg
        );
    }

    #[test]
    fn matches_meeus_example_28a() {
        // 1992 October 13.0 TD: equation of time +13m 42.7s
        let sun = Solar::at(day(utc(1992, 10, 13, 0, 0, 0)));
        let expected = 13.0 + 42.7 / 60.0;
        assert!(
            (sun.equation_of_time_min - expected).abs() < 0.02,
            "{}",
            sun.equation_of_time_min
        );
    }

    #[test]
    fn matches_the_spa_reference_example() {
        // Reda & Andreas (2004), NREL/TP-560-34302: 2003-10-17 12:30:30 at
        // UTC−7, 39.742476° N 105.1786° W; zenith 50.11162°, azimuth 194.34024°
        let sun = sun_position(39.742476, -105.1786, utc(2003, 10, 17, 19, 30, 30));
        let zenith = 90.0 - sun.elevation_deg;
        assert!((zenith - 50.11162).abs() < 0.03, "{zenith}");
        assert!(
            (sun.azimuth_deg - 194.34024).abs() < 0.03,
            "{}",
            sun.azimuth_deg
        );
    }

    #[test]
    fn noon_sun_stands_south_in_the_north_and_north_in_the_south() {
        // Around solar noon at Greenwich on the March equinox
        let north = sun_position(51.48, 0.0, utc(2024, 3, 20, 12, 7, 0));
        assert!((north.azimuth_deg - 180.0).abs() < 1.0, "{north:?}");
        assert!(
            (north.elevation_deg - (90.0 - 51.48)).abs() < 0.5,
            "{north:?}"
        );
        let south = sun_position(-33.87, 151.21, utc(2024, 6, 21, 1, 57, 0));
        assert!(
            south.azimuth_deg < 1.0 || south.azimuth_deg > 359.0,
            "{south:?}"
        );
        let equator = sun_position(0.0, 0.0, utc(2024, 3, 20, 12, 7, 0));
        assert!(equator.elevation_deg > 89.5, "{equator:?}");
    }

    #[test]
    fn the_sun_rises_in_the_east_and_is_down_at_midnight() {
        let morning = sun_position(46.5, 10.0, utc(2025, 6, 15, 4, 0, 0));
        assert!((40.0..90.0).contains(&morning.azimuth_deg), "{morning:?}");
        let midnight = sun_position(46.5, 10.0, utc(2025, 6, 14, 23, 20, 0));
        assert!(midnight.elevation_deg < -15.0, "{midnight:?}");
    }

    #[test]
    fn refraction_lifts_the_horizon_by_about_half_a_degree() {
        assert!((refraction_deg(0.0) - 0.482).abs() < 0.01);
        assert_eq!(refraction_deg(89.0), 0.0);
        assert!((0.0..0.05).contains(&refraction_deg(-10.0)));
    }
}
