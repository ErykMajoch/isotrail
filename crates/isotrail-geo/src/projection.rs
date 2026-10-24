use isotrail_track::EARTH_RADIUS_M;

/// Equirectangular metres around an origin: x east, y north
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalProjection {
    lat0: f64,
    lon0: f64,
    cos_lat0: f64,
}

impl LocalProjection {
    pub fn new(lat0: f64, lon0: f64) -> Self {
        Self {
            lat0,
            lon0,
            cos_lat0: lat0.to_radians().cos(),
        }
    }

    pub fn origin(&self) -> (f64, f64) {
        (self.lat0, self.lon0)
    }

    pub fn project(&self, lat: f64, lon: f64) -> (f64, f64) {
        let dlon = wrap_lon(lon - self.lon0);
        let x = EARTH_RADIUS_M * dlon.to_radians() * self.cos_lat0;
        let y = EARTH_RADIUS_M * (lat - self.lat0).to_radians();
        (x, y)
    }

    pub fn unproject(&self, x: f64, y: f64) -> (f64, f64) {
        let lat = self.lat0 + (y / EARTH_RADIUS_M).to_degrees();
        let lon = self.lon0 + (x / (EARTH_RADIUS_M * self.cos_lat0)).to_degrees();
        (lat, wrap_lon(lon))
    }
}

fn wrap_lon(lon: f64) -> f64 {
    (lon + 180.0).rem_euclid(360.0) - 180.0
}

#[cfg(test)]
mod tests {
    use isotrail_track::{TrackPoint, haversine_m};

    use super::*;

    fn at(lat: f64, lon: f64) -> TrackPoint {
        TrackPoint {
            lat,
            lon,
            ele: None,
            time: None,
        }
    }

    fn offsets_within(radius_m: f64) -> Vec<(f64, f64)> {
        let mut offsets = Vec::new();
        for ring in 1..=5 {
            let r = radius_m * f64::from(ring) / 5.0;
            for step in 0..16 {
                let a = f64::from(step) * std::f64::consts::TAU / 16.0;
                offsets.push((r * a.cos(), r * a.sin()));
            }
        }
        offsets
    }

    #[test]
    fn round_trip_stays_under_half_a_metre_within_50_km() {
        for (lat0, lon0) in [(0.0, 0.0), (47.0, 10.0), (70.0, -20.0), (-33.0, 179.9)] {
            let projection = LocalProjection::new(lat0, lon0);
            for (x, y) in offsets_within(50_000.0) {
                let (lat, lon) = projection.unproject(x, y);
                let (bx, by) = projection.project(lat, lon);
                let (lat2, lon2) = projection.unproject(bx, by);
                let error = haversine_m(&at(lat, lon), &at(lat2, lon2));
                assert!(error < 0.5, "{error} m at ({lat0}, {lon0}) + ({x}, {y})");
                assert!((bx - x).hypot(by - y) < 0.5);
                assert!((-180.0..180.0).contains(&lon));
            }
        }
    }

    #[test]
    fn distances_agree_with_haversine_within_one_percent_at_50_km() {
        for (lat0, lon0) in [(0.0, 0.0), (47.0, 10.0), (60.0, 5.0)] {
            let projection = LocalProjection::new(lat0, lon0);
            for (x, y) in offsets_within(50_000.0) {
                let (lat, lon) = projection.unproject(x, y);
                let great_circle = haversine_m(&at(lat0, lon0), &at(lat, lon));
                let flat = x.hypot(y);
                assert!(
                    (flat - great_circle).abs() < 0.01 * great_circle,
                    "{flat} vs {great_circle} at ({lat0}, {lon0}) + ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn crossing_the_antimeridian_stays_local() {
        let projection = LocalProjection::new(-17.0, 179.95);
        let (x, _) = projection.project(-17.0, -179.95);
        let expected = EARTH_RADIUS_M * 0.1_f64.to_radians() * 17.0_f64.to_radians().cos();
        assert!((x - expected).abs() < 1e-6, "{x} vs {expected}");
        let (_, lon) = projection.unproject(x, 0.0);
        assert!((lon + 179.95).abs() < 1e-9, "{lon}");
    }

    #[test]
    fn axes_point_east_and_north() {
        let projection = LocalProjection::new(46.5, 10.0);
        assert_eq!(projection.project(46.5, 10.0), (0.0, 0.0));
        let (x, y) = projection.project(46.5, 10.01);
        assert!(x > 0.0 && y == 0.0);
        let (x, y) = projection.project(46.51, 10.0);
        assert!(x == 0.0 && y > 0.0);
        assert_eq!(projection.origin(), (46.5, 10.0));
    }
}
