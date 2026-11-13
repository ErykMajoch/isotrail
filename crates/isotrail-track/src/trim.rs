use crate::{MIN_SEGMENT_POINTS, Segment, Track, haversine_m};

impl Track {
    /// The track from the first recorded point at least `start_m` along it
    /// to the last one at least `end_m` before its end, measured as
    /// [`TrackStats::distance_m`](crate::TrackStats::distance_m) is, jumps
    /// between pieces included. Pieces left with too few points are dropped,
    /// and None means nothing is left
    pub fn trimmed(&self, start_m: f64, end_m: f64) -> Option<Track> {
        let mut along = Vec::with_capacity(self.point_count());
        let mut previous = None;
        let mut total = 0.0;
        for point in self.points() {
            if let Some(previous) = previous {
                total += haversine_m(previous, point);
            }
            along.push(total);
            previous = Some(point);
        }
        let mut along = along.into_iter();
        let segments: Vec<Segment> = self
            .segments
            .iter()
            .map(|segment| Segment {
                points: segment
                    .points
                    .iter()
                    .zip(along.by_ref())
                    .filter(|&(_, done)| done >= start_m && total - done >= end_m)
                    .map(|(point, _)| *point)
                    .collect(),
            })
            .filter(|segment| segment.points.len() >= MIN_SEGMENT_POINTS)
            .collect();
        (!segments.is_empty()).then(|| Track {
            name: self.name.clone(),
            desc: self.desc.clone(),
            segments,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TrackPoint;

    /// One degree of latitude, in metres along the haversine sphere
    const DEGREE_M: f64 = crate::EARTH_RADIUS_M * std::f64::consts::PI / 180.0;

    /// Pieces of points 100m apart going north, each piece starting at the
    /// given multiple of 100m
    fn track(pieces: &[(u32, u32)]) -> Track {
        let point = |step: u32| TrackPoint {
            lat: 46.0 + f64::from(step) * 100.0 / DEGREE_M,
            lon: 10.0,
            ele: Some(f64::from(step)),
            time: None,
        };
        Track {
            name: Some("Walk".into()),
            segments: pieces
                .iter()
                .map(|&(from, count)| Segment {
                    points: (from..from + count).map(point).collect(),
                })
                .collect(),
            ..Track::default()
        }
    }

    fn steps(track: &Track) -> Vec<Vec<u32>> {
        track
            .segments
            .iter()
            .map(|segment| {
                segment
                    .points
                    .iter()
                    .map(|point| point.ele.unwrap() as u32)
                    .collect()
            })
            .collect()
    }

    #[test]
    fn each_end_is_cut_at_a_recorded_point() {
        let walk = track(&[(0, 11)]);
        let trimmed = walk.trimmed(250.0, 250.0).unwrap();
        assert_eq!(steps(&trimmed), [vec![3, 4, 5, 6, 7]]);
        assert_eq!(trimmed.name.as_deref(), Some("Walk"));
        let stats = trimmed.stats().unwrap();
        assert!((stats.distance_m - 400.0).abs() < 1e-6);
    }

    #[test]
    fn a_cut_in_a_gap_starts_at_the_next_piece() {
        // 0–300m, a jump, then 1000–1500m
        let walk = track(&[(0, 4), (10, 6)]);
        assert_eq!(
            steps(&walk.trimmed(500.0, 0.0).unwrap()),
            [vec![10, 11, 12, 13, 14, 15]]
        );
        assert_eq!(
            steps(&walk.trimmed(0.0, 700.0).unwrap()),
            [vec![0, 1, 2, 3]]
        );
    }

    #[test]
    fn a_piece_left_with_one_point_is_dropped() {
        let walk = track(&[(0, 3), (5, 4)]);
        assert_eq!(
            steps(&walk.trimmed(150.0, 0.0).unwrap()),
            [vec![5, 6, 7, 8]]
        );
    }

    #[test]
    fn nothing_left_is_none_and_no_trim_keeps_everything() {
        let walk = track(&[(0, 4), (10, 3)]);
        assert_eq!(walk.trimmed(0.0, 0.0).unwrap(), walk);
        assert!(walk.trimmed(1250.0, 0.0).is_none());
        assert!(walk.trimmed(600.0, 600.0).is_none());
        assert!(walk.trimmed(1150.0, 0.0).is_none());
    }
}
