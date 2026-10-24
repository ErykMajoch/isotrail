use std::f64::consts::{FRAC_PI_2, PI};

use isotrail_track::Track;

use crate::{GeoError, LocalProjection};

pub const PADDING_FRACTION: f64 = 0.15;
pub const MIN_PADDING_M: f64 = 500.0;
pub const MAX_ASPECT: f64 = 2.0;

/// The slab of terrain cut out around a route: a rectangle in local metres,
/// turned so its long side follows the route
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Footprint {
    pub projection: LocalProjection,
    pub center: (f64, f64),
    /// Angle of the long side from east, counter-clockwise, in (-π/2, π/2]
    pub rotation_rad: f64,
    pub length_m: f64,
    pub width_m: f64,
}

impl Footprint {
    pub fn from_track(track: &Track) -> Result<Footprint, GeoError> {
        let first = track.points().next().ok_or(GeoError::EmptyTrack)?;
        let provisional = LocalProjection::new(first.lat, first.lon);
        let (min, max) = bounds(track.points().map(|p| provisional.project(p.lat, p.lon)));
        let (lat0, lon0) = provisional.unproject((min.0 + max.0) / 2.0, (min.1 + max.1) / 2.0);
        let projection = LocalProjection::new(lat0, lon0);

        let segments: Vec<Vec<(f64, f64)>> = track
            .segments
            .iter()
            .map(|s| {
                s.points
                    .iter()
                    .map(|p| projection.project(p.lat, p.lon))
                    .collect()
            })
            .collect();
        let angle = principal_angle(&segments);
        let (cos, sin) = (angle.cos(), angle.sin());
        let (min, max) = bounds(
            segments
                .iter()
                .flatten()
                .map(|&(x, y)| (x * cos + y * sin, -x * sin + y * cos)),
        );
        let (mid_u, mid_v) = ((min.0 + max.0) / 2.0, (min.1 + max.1) / 2.0);
        let center = (mid_u * cos - mid_v * sin, mid_u * sin + mid_v * cos);

        let pad = |extent: f64| extent + 2.0 * (PADDING_FRACTION * extent).max(MIN_PADDING_M);
        let (mut length_m, mut width_m) = (pad(max.0 - min.0), pad(max.1 - min.1));
        let mut rotation_rad = angle;
        if width_m > length_m {
            (length_m, width_m) = (width_m, length_m);
            rotation_rad += FRAC_PI_2;
        }
        width_m = width_m.max(length_m / MAX_ASPECT);
        while rotation_rad > FRAC_PI_2 {
            rotation_rad -= PI;
        }
        while rotation_rad <= -FRAC_PI_2 {
            rotation_rad += PI;
        }

        Ok(Footprint {
            projection,
            center,
            rotation_rad,
            length_m,
            width_m,
        })
    }

    /// Compass bearing of the long side, in [0, 180)
    pub fn long_axis_bearing_deg(&self) -> f64 {
        (90.0 - self.rotation_rad.to_degrees()).rem_euclid(180.0)
    }

    /// Compass bearing the default isometric camera faces, with the long side
    /// on a 2:1 screen diagonal; an east-west slab gives the classic 45°
    pub fn camera_yaw_deg(&self) -> f64 {
        (self.long_axis_bearing_deg() - 45.0).rem_euclid(360.0)
    }

    /// Slab coordinates (u along the long side, v across, from the centre) to local metres
    pub fn to_local(&self, u: f64, v: f64) -> (f64, f64) {
        let (sin, cos) = self.rotation_rad.sin_cos();
        (
            self.center.0 + u * cos - v * sin,
            self.center.1 + u * sin + v * cos,
        )
    }

    pub fn from_local(&self, x: f64, y: f64) -> (f64, f64) {
        let (sin, cos) = self.rotation_rad.sin_cos();
        let (dx, dy) = (x - self.center.0, y - self.center.1);
        (dx * cos + dy * sin, -dx * sin + dy * cos)
    }

    /// Local corners, counter-clockwise from the (-u, -v) corner
    pub fn corners(&self) -> [(f64, f64); 4] {
        let (u, v) = (self.length_m / 2.0, self.width_m / 2.0);
        [(-u, -v), (u, -v), (u, v), (-u, v)].map(|(u, v)| self.to_local(u, v))
    }
}

fn bounds(points: impl Iterator<Item = (f64, f64)>) -> ((f64, f64), (f64, f64)) {
    points.fold(
        (
            (f64::INFINITY, f64::INFINITY),
            (f64::NEG_INFINITY, f64::NEG_INFINITY),
        ),
        |(min, max), (x, y)| ((min.0.min(x), min.1.min(y)), (max.0.max(x), max.1.max(y))),
    )
}

/// Main direction of the route treated as a uniform line, so pauses and dense
/// sampling don't pull the axis
fn principal_angle(segments: &[Vec<(f64, f64)>]) -> f64 {
    let mut lines = Vec::new();
    for segment in segments {
        for pair in segment.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let length = (b.0 - a.0).hypot(b.1 - a.1);
            if length > 0.0 {
                lines.push((a, b, length));
            }
        }
    }
    if lines.is_empty() {
        lines = segments.iter().flatten().map(|&p| (p, p, 1.0)).collect();
    }

    let total: f64 = lines.iter().map(|l| l.2).sum();
    let (mut mx, mut my) = (0.0, 0.0);
    for &(a, b, w) in &lines {
        mx += w * (a.0 + b.0) / 2.0;
        my += w * (a.1 + b.1) / 2.0;
    }
    let (mx, my) = (mx / total, my / total);

    // Second moment of a uniform segment about its midpoint is d·dᵀ/12
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for &(a, b, w) in &lines {
        let (cx, cy) = ((a.0 + b.0) / 2.0 - mx, (a.1 + b.1) / 2.0 - my);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        sxx += w * (cx * cx + dx * dx / 12.0);
        syy += w * (cy * cy + dy * dy / 12.0);
        sxy += w * (cx * cy + dx * dy / 12.0);
    }
    0.5 * (2.0 * sxy).atan2(sxx - syy)
}

#[cfg(test)]
mod tests {
    use isotrail_track::{Segment, TrackPoint};

    use super::*;

    const ORIGIN: (f64, f64) = (46.5, 10.0);

    fn track(segments: &[Vec<(f64, f64)>]) -> Track {
        let projection = LocalProjection::new(ORIGIN.0, ORIGIN.1);
        Track {
            segments: segments
                .iter()
                .map(|offsets| Segment {
                    points: offsets
                        .iter()
                        .map(|&(x, y)| {
                            let (lat, lon) = projection.unproject(x, y);
                            TrackPoint {
                                lat,
                                lon,
                                ele: None,
                                time: None,
                            }
                        })
                        .collect(),
                })
                .collect(),
            ..Track::default()
        }
    }

    fn line(from: (f64, f64), to: (f64, f64), steps: u32) -> Vec<(f64, f64)> {
        (0..=steps)
            .map(|i| {
                let t = f64::from(i) / f64::from(steps);
                (from.0 + t * (to.0 - from.0), from.1 + t * (to.1 - from.1))
            })
            .collect()
    }

    fn close(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    fn assert_contains(track: &Track, footprint: &Footprint) {
        let (half_u, half_v) = (footprint.length_m / 2.0, footprint.width_m / 2.0);
        for p in track.points() {
            let (x, y) = footprint.projection.project(p.lat, p.lon);
            let (u, v) = footprint.from_local(x, y);
            assert!(
                half_u - u.abs() >= MIN_PADDING_M - 1e-6,
                "u {u} of ±{half_u}"
            );
            assert!(
                half_v - v.abs() >= MIN_PADDING_M - 1e-6,
                "v {v} of ±{half_v}"
            );
        }
    }

    #[test]
    fn north_south_line_gets_padded_and_widened_to_two_to_one() {
        let track = track(&[line((0.0, -5_000.0), (0.0, 5_000.0), 100)]);
        let footprint = Footprint::from_track(&track).unwrap();
        assert!(close(footprint.long_axis_bearing_deg(), 0.0, 1e-9));
        assert!(close(footprint.length_m, 13_000.0, 1e-3), "{footprint:?}");
        assert!(close(footprint.width_m, 6_500.0, 1e-3));
        assert_eq!(footprint.length_m / footprint.width_m, MAX_ASPECT);
        assert!(footprint.center.0.hypot(footprint.center.1) < 1e-3);
        assert_contains(&track, &footprint);
    }

    #[test]
    fn diagonal_route_turns_the_slab() {
        let rising = track(&[line((-3_000.0, -3_000.0), (3_000.0, 3_000.0), 60)]);
        let footprint = Footprint::from_track(&rising).unwrap();
        assert!(
            close(footprint.long_axis_bearing_deg(), 45.0, 0.01),
            "{footprint:?}"
        );
        assert!(close(footprint.rotation_rad.to_degrees(), 45.0, 0.01));
        let diagonal = 6_000.0 * 2.0_f64.sqrt();
        assert!(close(footprint.length_m, 1.3 * diagonal, 1.0));
        assert!(close(footprint.width_m, footprint.length_m / 2.0, 1e-6));
        assert_contains(&rising, &footprint);

        let reversed = track(&[line((3_000.0, -3_000.0), (-3_000.0, 3_000.0), 60)]);
        let footprint = Footprint::from_track(&reversed).unwrap();
        assert!(close(footprint.long_axis_bearing_deg(), 135.0, 0.01));
        assert!(close(footprint.rotation_rad.to_degrees(), -45.0, 0.01));
    }

    #[test]
    fn compact_loop_gets_the_minimum_padding_and_keeps_its_aspect() {
        let loop_ = [
            line((-150.0, -100.0), (150.0, -100.0), 10),
            line((150.0, -100.0), (150.0, 100.0), 10),
            line((150.0, 100.0), (-150.0, 100.0), 10),
            line((-150.0, 100.0), (-150.0, -100.0), 10),
        ]
        .concat();
        let track = track(&[loop_]);
        let footprint = Footprint::from_track(&track).unwrap();
        assert!(close(footprint.rotation_rad, 0.0, 1e-6), "{footprint:?}");
        assert!(close(footprint.length_m, 1_300.0, 1e-3));
        assert!(close(footprint.width_m, 1_200.0, 1e-3));
        assert_contains(&track, &footprint);
    }

    #[test]
    fn padding_scales_with_each_axis() {
        let zigzag: Vec<_> = (0..=40)
            .map(|i| {
                (
                    f64::from(i) * 500.0,
                    if i % 2 == 0 { -6_000.0 } else { 6_000.0 },
                )
            })
            .collect();
        let track = track(&[zigzag]);
        let footprint = Footprint::from_track(&track).unwrap();
        assert!(
            close(footprint.length_m, 1.3 * 20_000.0, 1.0),
            "{footprint:?}"
        );
        assert!(close(footprint.width_m, 1.3 * 12_000.0, 1.0));
        assert_contains(&track, &footprint);
    }

    #[test]
    fn aspect_is_clamped_for_any_shape() {
        let shapes = [
            vec![line((0.0, 0.0), (20_000.0, 0.0), 50)],
            vec![line((0.0, 0.0), (10_000.0, 4_000.0), 50)],
            vec![line((0.0, 0.0), (-2_000.0, 30_000.0), 50)],
            vec![
                line((0.0, 0.0), (8_000.0, 1_000.0), 20),
                line((8_100.0, 1_000.0), (8_100.0, 9_000.0), 20),
            ],
            vec![
                (0..200)
                    .map(|i| {
                        let a = f64::from(i) * 0.1;
                        (a.cos() * 3_000.0 + f64::from(i) * 40.0, a.sin() * 1_500.0)
                    })
                    .collect(),
            ],
        ];
        for shape in shapes {
            let track = track(&shape);
            let footprint = Footprint::from_track(&track).unwrap();
            let aspect = footprint.length_m / footprint.width_m;
            assert!((1.0..=MAX_ASPECT + 1e-9).contains(&aspect), "{footprint:?}");
            assert!(footprint.rotation_rad > -FRAC_PI_2 && footprint.rotation_rad <= FRAC_PI_2);
            assert_contains(&track, &footprint);
        }
    }

    #[test]
    fn long_side_follows_the_wider_extent_when_the_axis_is_crowded() {
        let mut shuttle = Vec::new();
        for _ in 0..20 {
            shuttle.extend(line((0.0, -750.0), (1_000.0, -750.0), 10));
            shuttle.extend(line((1_000.0, -750.0), (0.0, -750.0), 10));
        }
        let spur = line((500.0, -750.0), (500.0, 750.0), 10);
        let track = track(&[shuttle, spur]);
        let footprint = Footprint::from_track(&track).unwrap();
        assert!(
            close(footprint.long_axis_bearing_deg(), 0.0, 1e-6),
            "{footprint:?}"
        );
        assert!(close(footprint.length_m, 2_500.0, 1e-3));
        assert!(close(footprint.width_m, 2_000.0, 1e-3));
        assert_contains(&track, &footprint);
    }

    #[test]
    fn single_spot_gets_a_square_kilometre() {
        let track = track(&[vec![(0.0, 0.0); 3], vec![(0.0, 0.0)]]);
        let footprint = Footprint::from_track(&track).unwrap();
        assert_eq!((footprint.length_m, footprint.width_m), (1_000.0, 1_000.0));
        assert_eq!(footprint.rotation_rad, 0.0);
        assert_contains(&track, &footprint);
    }

    #[test]
    fn empty_track_has_no_footprint() {
        assert!(matches!(
            Footprint::from_track(&Track::default()),
            Err(GeoError::EmptyTrack)
        ));
        let empty_segments = Track {
            segments: vec![Segment::default(); 2],
            ..Track::default()
        };
        assert!(matches!(
            Footprint::from_track(&empty_segments),
            Err(GeoError::EmptyTrack)
        ));
    }

    #[test]
    fn camera_yaw_keeps_the_long_side_on_a_diagonal() {
        let east_west = track(&[line((-4_000.0, 0.0), (4_000.0, 0.0), 20)]);
        let footprint = Footprint::from_track(&east_west).unwrap();
        assert!(close(footprint.long_axis_bearing_deg(), 90.0, 1e-6));
        assert!(close(footprint.camera_yaw_deg(), 45.0, 1e-6));

        let north_south = track(&[line((0.0, -4_000.0), (0.0, 4_000.0), 20)]);
        let footprint = Footprint::from_track(&north_south).unwrap();
        assert!(close(footprint.camera_yaw_deg(), 315.0, 1e-6));
    }

    #[test]
    fn corners_and_slab_coordinates_agree() {
        let track = track(&[line((-3_000.0, -1_000.0), (3_000.0, 2_000.0), 30)]);
        let footprint = Footprint::from_track(&track).unwrap();
        let [a, b, c, d] = footprint.corners();
        let side = |p: (f64, f64), q: (f64, f64)| (q.0 - p.0).hypot(q.1 - p.1);
        assert!(close(side(a, b), footprint.length_m, 1e-6));
        assert!(close(side(b, c), footprint.width_m, 1e-6));
        assert!(close(side(c, d), footprint.length_m, 1e-6));
        let (u, v) = footprint.from_local(c.0, c.1);
        assert!(close(u, footprint.length_m / 2.0, 1e-6));
        assert!(close(v, footprint.width_m / 2.0, 1e-6));
    }
}
