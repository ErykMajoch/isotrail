use serde::{Deserialize, Serialize};

/// Everything a renderer needs, with no further downloads
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneBundle {
    pub meta: Metadata,
    pub grid: GridInfo,
    pub terrain: Terrain,
    pub water: Vec<WaterBody>,
    pub prop_seed: u64,
    pub route: Route,
    pub profile: Vec<ProfileSample>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    pub title: String,
    pub name: Option<String>,
    pub desc: Option<String>,
    /// IANA name of the zone at the start
    pub timezone: Option<String>,
    /// Local start date as `YYYY-MM-DD`
    pub local_date: Option<String>,
    pub stats: HikeStats,
    pub attribution: Vec<String>,
    pub provenance: Provenance,
}

/// Statistics of the full-resolution track; times are Unix seconds
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HikeStats {
    pub points: u32,
    pub segments: u32,
    pub distance_m: f64,
    pub gain_m: Option<f64>,
    pub loss_m: Option<f64>,
    pub min_ele_m: Option<f64>,
    pub max_ele_m: Option<f64>,
    pub bounds: Bounds,
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub elapsed_s: Option<i64>,
    pub moving_s: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min_lat: f64,
    pub min_lon: f64,
    pub max_lat: f64,
    pub max_lon: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub generator: String,
    pub elevation: String,
    pub map_data: String,
}

/// Square cells over a slab turned by `rotation_rad` (from east,
/// counter-clockwise) around `center_m`, in local equirectangular metres
/// about the origin; row 0 is the slab's +v edge
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GridInfo {
    pub cols: u32,
    pub rows: u32,
    pub cell_size_m: f64,
    pub origin_lat: f64,
    pub origin_lon: f64,
    pub center_m: (f64, f64),
    pub rotation_rad: f64,
    pub exaggeration: f64,
}

impl GridInfo {
    pub fn cells(&self) -> usize {
        (self.cols as usize).saturating_mul(self.rows as usize)
    }

    pub fn length_m(&self) -> f64 {
        f64::from(self.cols) * self.cell_size_m
    }

    pub fn width_m(&self) -> f64 {
        f64::from(self.rows) * self.cell_size_m
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
}

/// Per-cell layers, row-major
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Terrain {
    /// Height is `offset + value × scale` metres
    pub heights: Vec<u16>,
    pub height_scale_m: f32,
    pub height_offset_m: f32,
    /// [`Landcover`](crate::Landcover) values
    pub landcover: Vec<u8>,
    /// [`line_mask`](crate::line_mask) bits
    pub lines: Vec<u8>,
}

impl Terrain {
    /// Spreads the height range over every u16 step, leaving the class
    /// layers empty
    pub fn quantise(heights_m: &[f32]) -> Terrain {
        let (min, max) = heights_m
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), &h| {
                (min.min(h), max.max(h))
            });
        let (offset, span) = if min <= max {
            (min, max - min)
        } else {
            (0.0, 0.0)
        };
        let scale = if span > 0.0 {
            span / f32::from(u16::MAX)
        } else {
            1.0
        };
        Terrain {
            heights: heights_m
                .iter()
                .map(|&h| ((h - offset) / scale).round().clamp(0.0, 65_535.0) as u16)
                .collect(),
            height_scale_m: scale,
            height_offset_m: offset,
            landcover: Vec::new(),
            lines: Vec::new(),
        }
    }

    pub fn height_m(&self, cell: usize) -> f32 {
        self.height_offset_m + f32::from(self.heights[cell]) * self.height_scale_m
    }
}

/// A lake or reservoir with a level surface
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaterBody {
    pub level_m: f32,
    /// Row-major cell indices, ascending
    pub cells: Vec<u32>,
}

/// The recorded route as one polyline, simplified for drawing
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Route {
    pub points: Vec<RoutePoint>,
    /// Indices of points that start a new piece, after a gap that is not drawn
    pub breaks: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RoutePoint {
    /// Fractional grid position; cell (c, r) spans [c, c + 1) × [r, r + 1)
    pub col: f32,
    pub row: f32,
    /// Along the full-resolution track
    pub distance_m: f32,
    /// Unix seconds
    pub time: Option<i64>,
    pub gps_ele_m: Option<f32>,
    /// Ground height under the point, without exaggeration
    pub draped_m: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProfileSample {
    pub distance_m: f32,
    pub ele_m: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantised_heights_stay_within_half_a_step() {
        let heights: Vec<f32> = (0..1000)
            .map(|i| 980.0 + (i as f32 * 0.731) % 429.0)
            .collect();
        let terrain = Terrain::quantise(&heights);
        assert_eq!(terrain.height_offset_m, 980.0);
        assert!(terrain.heights.contains(&0) && terrain.heights.contains(&u16::MAX));
        for (i, &h) in heights.iter().enumerate() {
            let error = (terrain.height_m(i) - h).abs();
            assert!(
                error <= terrain.height_scale_m * 0.5 + 1e-3,
                "{error} at {i}"
            );
        }
    }

    #[test]
    fn camera_yaw_puts_the_long_side_on_a_diagonal() {
        let grid = |rotation_deg: f64| GridInfo {
            cols: 8,
            rows: 4,
            cell_size_m: 10.0,
            origin_lat: 0.0,
            origin_lon: 0.0,
            center_m: (0.0, 0.0),
            rotation_rad: rotation_deg.to_radians(),
            exaggeration: 1.0,
        };
        for (rotation, bearing, yaw) in [
            (0.0, 90.0, 45.0),
            (90.0, 0.0, 315.0),
            (30.0, 60.0, 15.0),
            (-60.0, 150.0, 105.0),
        ] {
            let grid = grid(rotation);
            assert!((grid.long_axis_bearing_deg() - bearing).abs() < 1e-9);
            assert!((grid.camera_yaw_deg() - yaw).abs() < 1e-9, "{rotation}");
        }
        assert_eq!((grid(0.0).length_m(), grid(0.0).width_m()), (80.0, 40.0));
    }

    #[test]
    fn flat_and_empty_heights_quantise_to_zero() {
        let flat = Terrain::quantise(&[312.5; 4]);
        assert_eq!(flat.heights, [0; 4]);
        assert_eq!(flat.height_m(3), 312.5);
        assert!(Terrain::quantise(&[]).heights.is_empty());
    }
}
