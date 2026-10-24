use isotrail_bundle::Landcover;

use super::percentile;
use crate::Heightmap;

pub const ROCK_SLOPE_DEG: f64 = 40.0;
/// Above the treeline soil is thin, so gentler slopes are already bare
pub const ALPINE_ROCK_SLOPE_DEG: f64 = 30.0;
pub const ROCK_ABOVE_TREELINE_M: f64 = 800.0;
const TREELINE_PERCENTILE: f64 = 0.95;
const MIN_FOREST_SHARE: f64 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Treeline {
    /// Near the top of the mapped forest in the slab
    Forest(f64),
    /// No forest to go by: a rough figure for the latitude
    Latitude(f64),
}

impl Treeline {
    pub fn estimate(forest_heights_m: &mut [f32], cells: usize, lat_deg: f64) -> Treeline {
        if !forest_heights_m.is_empty()
            && forest_heights_m.len() as f64 >= MIN_FOREST_SHARE * cells as f64
        {
            Treeline::Forest(f64::from(percentile(forest_heights_m, TREELINE_PERCENTILE)))
        } else {
            Treeline::Latitude(latitude_treeline_m(lat_deg))
        }
    }

    pub fn m(self) -> f64 {
        match self {
            Treeline::Forest(m) | Treeline::Latitude(m) => m,
        }
    }
}

/// About 2100 m in the Alps, 1700 m in the Tatras and 670 m in Scotland
pub fn latitude_treeline_m(lat_deg: f64) -> f64 {
    (2100.0 - 130.0 * (lat_deg.abs() - 46.0)).clamp(0.0, 4000.0)
}

/// Slope at a cell from central differences of true heights, one-sided at
/// the grid's edges
pub fn slope_deg(heightmap: &Heightmap, cell_size_m: f64, col: u32, row: u32) -> f64 {
    let gradient = |lo: (u32, u32), hi: (u32, u32), steps: u32| {
        if steps == 0 {
            0.0
        } else {
            f64::from(heightmap.get(hi.0, hi.1) - heightmap.get(lo.0, lo.1))
                / (f64::from(steps) * cell_size_m)
        }
    };
    let (left, right) = (col.saturating_sub(1), (col + 1).min(heightmap.cols - 1));
    let (up, down) = (row.saturating_sub(1), (row + 1).min(heightmap.rows - 1));
    let dx = gradient((left, row), (right, row), right - left);
    let dy = gradient((col, up), (col, down), down - up);
    dx.hypot(dy).atan().to_degrees()
}

/// The class of a cell no map feature covers
pub fn fallback_class(slope_deg: f64, height_m: f64, treeline: Treeline) -> Landcover {
    let treeline = treeline.m();
    if slope_deg > ROCK_SLOPE_DEG
        || height_m > treeline + ROCK_ABOVE_TREELINE_M
        || (height_m > treeline && slope_deg > ALPINE_ROCK_SLOPE_DEG)
    {
        Landcover::Rock
    } else {
        Landcover::Grassland
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(rise_per_cell: f32) -> Heightmap {
        let (cols, rows) = (4, 3);
        Heightmap {
            cols,
            rows,
            heights_m: (0..rows)
                .flat_map(|_| (0..cols).map(move |c| c as f32 * rise_per_cell))
                .collect(),
        }
    }

    #[test]
    fn slope_comes_from_neighbouring_cells() {
        let flat = ramp(0.0);
        assert_eq!(slope_deg(&flat, 30.0, 1, 1), 0.0);
        let ramp = ramp(30.0);
        for (col, row) in [(0, 0), (1, 1), (3, 2)] {
            assert!((slope_deg(&ramp, 30.0, col, row) - 45.0).abs() < 1e-9);
        }
        assert!((slope_deg(&ramp, 60.0, 1, 1) - 0.5f64.atan().to_degrees()).abs() < 1e-9);
        let single = Heightmap {
            cols: 1,
            rows: 1,
            heights_m: vec![10.0],
        };
        assert_eq!(slope_deg(&single, 30.0, 0, 0), 0.0);
    }

    #[test]
    fn steep_or_high_ground_is_rock() {
        let treeline = Treeline::Forest(1500.0);
        assert_eq!(fallback_class(10.0, 900.0, treeline), Landcover::Grassland);
        assert_eq!(fallback_class(45.0, 900.0, treeline), Landcover::Rock);
        assert_eq!(fallback_class(20.0, 1700.0, treeline), Landcover::Grassland);
        assert_eq!(fallback_class(35.0, 1700.0, treeline), Landcover::Rock);
        assert_eq!(fallback_class(35.0, 1400.0, treeline), Landcover::Grassland);
        assert_eq!(fallback_class(5.0, 2400.0, treeline), Landcover::Rock);
    }

    #[test]
    fn treeline_sits_near_the_top_of_the_forest() {
        let mut heights: Vec<f32> = (0..=100).map(|i| 1000.0 + i as f32 * 5.0).collect();
        assert_eq!(
            Treeline::estimate(&mut heights, 1000, 49.0),
            Treeline::Forest(1475.0)
        );
    }

    #[test]
    fn little_or_no_forest_falls_back_to_latitude() {
        let mut few = vec![300.0; 9];
        assert_eq!(
            Treeline::estimate(&mut few, 1000, 57.0),
            Treeline::Latitude(latitude_treeline_m(57.0))
        );
        assert_eq!(
            Treeline::estimate(&mut [], 1000, -46.0),
            Treeline::Latitude(2100.0)
        );
        assert!((latitude_treeline_m(57.0) - 670.0).abs() < 1e-9);
        assert_eq!(latitude_treeline_m(0.0), 4000.0);
        assert_eq!(latitude_treeline_m(80.0), 0.0);
    }
}
