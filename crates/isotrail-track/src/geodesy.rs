use crate::TrackPoint;

pub const EARTH_RADIUS_M: f64 = 6_371_008.8;

pub fn haversine_m(a: &TrackPoint, b: &TrackPoint) -> f64 {
    let (lat1, lat2) = (a.lat.to_radians(), b.lat.to_radians());
    let dlat = lat2 - lat1;
    let dlon = (b.lon - a.lon).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().min(1.0).asin()
}

/// Equirectangular metres around `(lat0, lon0)`, accurate enough for short spans
pub(crate) fn local_xy(point: &TrackPoint, lat0: f64, lon0: f64) -> (f64, f64) {
    let x = EARTH_RADIUS_M * (point.lon - lon0).to_radians() * lat0.to_radians().cos();
    let y = EARTH_RADIUS_M * (point.lat - lat0).to_radians();
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(lat: f64, lon: f64) -> TrackPoint {
        TrackPoint {
            lat,
            lon,
            ele: None,
            time: None,
        }
    }

    #[test]
    fn haversine_matches_arc_length_along_a_meridian() {
        let d = haversine_m(&at(46.5, 10.0), &at(46.501, 10.0));
        let arc = EARTH_RADIUS_M * 0.001_f64.to_radians();
        assert!((d - arc).abs() < 1e-6, "{d} vs {arc}");
        assert!((d - 111.195).abs() < 1e-3);
    }

    #[test]
    fn haversine_shrinks_with_latitude_along_a_parallel() {
        let equator = haversine_m(&at(0.0, 10.0), &at(0.0, 10.001));
        let sixty = haversine_m(&at(60.0, 10.0), &at(60.0, 10.001));
        assert!((sixty / equator - 0.5).abs() < 1e-6);
        assert_eq!(haversine_m(&at(46.5, 10.0), &at(46.5, 10.0)), 0.0);
    }

    #[test]
    fn local_xy_agrees_with_haversine_over_short_distances() {
        let (a, b) = (at(46.5, 10.0), at(46.51, 10.02));
        let (x, y) = local_xy(&b, a.lat, a.lon);
        let flat = x.hypot(y);
        assert!((flat - haversine_m(&a, &b)).abs() < 2.0);
    }
}
