use geo::{Coord, LineString, SimplifyIdx};

use crate::geodesy::local_xy;
use crate::{Segment, Track};

impl Track {
    /// Douglas-Peucker per segment, keeping original points and boundaries;
    /// compute stats on the full track instead
    pub fn simplified(&self, tolerance_m: f64) -> Track {
        let segments = self
            .segments
            .iter()
            .map(|segment| {
                let Some(first) = segment.points.first() else {
                    return Segment::default();
                };
                let line: LineString = segment
                    .points
                    .iter()
                    .map(|p| {
                        let (x, y) = local_xy(p, first.lat, first.lon);
                        Coord { x, y }
                    })
                    .collect();
                Segment {
                    points: line
                        .simplify_idx(tolerance_m)
                        .into_iter()
                        .map(|i| segment.points[i])
                        .collect(),
                }
            })
            .collect();
        Track {
            name: self.name.clone(),
            desc: self.desc.clone(),
            segments,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::TrackPoint;

    use super::*;

    fn at(lat: f64, lon: f64) -> TrackPoint {
        TrackPoint {
            lat,
            lon,
            ele: Some(lat),
            time: None,
        }
    }

    fn meridian(n: usize, lon: f64) -> Segment {
        Segment {
            points: (0..n).map(|i| at(46.5 + i as f64 * 0.001, lon)).collect(),
        }
    }

    #[test]
    fn collinear_points_collapse_to_their_endpoints() {
        let track = Track {
            segments: vec![meridian(5, 10.0)],
            ..Track::default()
        };
        let simple = track.simplified(1.0);
        assert_eq!(
            simple.segments[0].points,
            [track.segments[0].points[0], track.segments[0].points[4]]
        );
        assert_eq!(track.point_count(), 5);
    }

    #[test]
    fn a_spike_above_the_tolerance_survives() {
        let mut segment = meridian(5, 10.0);
        segment.points[2].lon += 0.001;
        let track = Track {
            segments: vec![segment.clone()],
            ..Track::default()
        };
        assert_eq!(
            track.simplified(50.0).segments[0].points,
            [segment.points[0], segment.points[2], segment.points[4]]
        );
        assert_eq!(track.simplified(100.0).point_count(), 2);
    }

    #[test]
    fn segment_boundaries_and_names_are_kept() {
        let track = Track {
            name: Some("Two parts".into()),
            desc: None,
            segments: vec![meridian(4, 10.0), meridian(6, 10.01)],
        };
        let simple = track.simplified(1.0);
        assert_eq!(simple.name.as_deref(), Some("Two parts"));
        assert_eq!(simple.segments.len(), 2);
        assert_eq!(simple.point_count(), 4);
    }
}
