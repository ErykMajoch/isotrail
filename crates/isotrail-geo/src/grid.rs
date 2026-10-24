use std::f64::consts::TAU;
use std::num::NonZeroU32;

use crate::Footprint;

pub const DEFAULT_DETAIL: NonZeroU32 = NonZeroU32::new(256).unwrap();
pub const MAX_DEM_ZOOM: u8 = 15;
const WEB_MERCATOR_RADIUS_M: f64 = 6_378_137.0;
const TILE_SIZE_PX: f64 = 256.0;

/// Square cells over a footprint: `cols` along the long side, `rows` across,
/// with the width snapped to a whole number of cells
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub footprint: Footprint,
    pub cols: u32,
    pub rows: u32,
    pub cell_size_m: f64,
}

impl Grid {
    pub fn new(footprint: &Footprint, detail: NonZeroU32) -> Grid {
        let cols = detail.get();
        let cell_size_m = footprint.length_m / f64::from(cols);
        let rows = ((footprint.width_m / cell_size_m - 1e-9).ceil() as u32).max(1);
        Grid {
            footprint: Footprint {
                width_m: f64::from(rows) * cell_size_m,
                ..*footprint
            },
            cols,
            rows,
            cell_size_m,
        }
    }

    pub fn cell_center_local(&self, col: u32, row: u32) -> (f64, f64) {
        let u = (f64::from(col) + 0.5) * self.cell_size_m - self.footprint.length_m / 2.0;
        // Row 0 is the +v edge so rows read top to bottom like an image
        let v = self.footprint.width_m / 2.0 - (f64::from(row) + 0.5) * self.cell_size_m;
        self.footprint.to_local(u, v)
    }

    /// Fractional (col, row) of a local point; cell (c, r) spans [c, c + 1) × [r, r + 1)
    pub fn local_to_cell(&self, x: f64, y: f64) -> (f64, f64) {
        let (u, v) = self.footprint.from_local(x, y);
        (
            (u + self.footprint.length_m / 2.0) / self.cell_size_m,
            (self.footprint.width_m / 2.0 - v) / self.cell_size_m,
        )
    }

    pub fn cell_center_lat_lon(&self, col: u32, row: u32) -> (f64, f64) {
        let (x, y) = self.cell_center_local(col, row);
        self.footprint.projection.unproject(x, y)
    }

    pub fn dem_zoom(&self) -> u8 {
        dem_zoom(self.cell_size_m, self.footprint.projection.origin().0)
    }
}

pub fn ground_resolution_m(zoom: u8, lat_deg: f64) -> f64 {
    TAU * WEB_MERCATOR_RADIUS_M * lat_deg.to_radians().cos()
        / (TILE_SIZE_PX * 2f64.powi(zoom.into()))
}

/// Coarsest tile zoom whose pixels are at most half a cell, so bilinear
/// resampling has several samples per cell
pub fn dem_zoom(cell_size_m: f64, lat_deg: f64) -> u8 {
    (0..=MAX_DEM_ZOOM)
        .find(|&zoom| ground_resolution_m(zoom, lat_deg) <= cell_size_m / 2.0)
        .unwrap_or(MAX_DEM_ZOOM)
}

#[cfg(test)]
mod tests {
    use isotrail_track::{Segment, Track, TrackPoint};

    use super::*;
    use crate::LocalProjection;

    fn footprint(from: (f64, f64), to: (f64, f64)) -> Footprint {
        let projection = LocalProjection::new(46.5, 10.0);
        let points = (0..=20)
            .map(|i| {
                let t = f64::from(i) / 20.0;
                let (lat, lon) = projection
                    .unproject(from.0 + t * (to.0 - from.0), from.1 + t * (to.1 - from.1));
                TrackPoint {
                    lat,
                    lon,
                    ele: None,
                    time: None,
                }
            })
            .collect();
        let track = Track {
            segments: vec![Segment { points }],
            ..Track::default()
        };
        Footprint::from_track(&track).unwrap()
    }

    fn detail(n: u32) -> NonZeroU32 {
        NonZeroU32::new(n).unwrap()
    }

    fn close(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn default_detail_puts_256_cells_on_the_long_side() {
        let footprint = footprint((0.0, -5_000.0), (0.0, 5_000.0));
        let grid = Grid::new(&footprint, DEFAULT_DETAIL);
        assert_eq!((grid.cols, grid.rows), (256, 128));
        assert!(close(grid.cell_size_m, 13_000.0 / 256.0, 1e-6));
        assert!(close(grid.footprint.width_m, footprint.width_m, 1e-6));
    }

    #[test]
    fn rows_cover_the_width_with_whole_cells() {
        let footprint = footprint((0.0, 0.0), (8_000.0, 0.0));
        assert!(close(footprint.width_m, 5_200.0, 1e-3));
        let grid = Grid::new(&footprint, detail(100));
        assert!(close(grid.cell_size_m, 104.0, 1e-6));
        assert_eq!(grid.rows, 50);
        let grid = Grid::new(&footprint, detail(37));
        assert_eq!(grid.rows, 19);
        assert!(grid.footprint.width_m >= footprint.width_m);
        assert!(grid.footprint.width_m - footprint.width_m < grid.cell_size_m);
        assert_eq!(Grid::new(&footprint, detail(1)).rows, 1);
    }

    #[test]
    fn corner_cells_sit_half_a_cell_inside() {
        let footprint = footprint((-3_000.0, -1_000.0), (3_000.0, 2_000.0));
        let grid = Grid::new(&footprint, detail(64));
        let (half_u, half_v, half_cell) = (
            grid.footprint.length_m / 2.0,
            grid.footprint.width_m / 2.0,
            grid.cell_size_m / 2.0,
        );
        let slab = |col, row| {
            let (x, y) = grid.cell_center_local(col, row);
            grid.footprint.from_local(x, y)
        };
        let (u, v) = slab(0, 0);
        assert!(close(u, -half_u + half_cell, 1e-6) && close(v, half_v - half_cell, 1e-6));
        let (u, v) = slab(grid.cols - 1, grid.rows - 1);
        assert!(close(u, half_u - half_cell, 1e-6) && close(v, -half_v + half_cell, 1e-6));
    }

    #[test]
    fn columns_follow_the_slab_rotation() {
        let footprint = footprint((-3_000.0, -3_000.0), (3_000.0, 3_000.0));
        let grid = Grid::new(&footprint, detail(128));
        let (a, b) = (grid.cell_center_local(0, 0), grid.cell_center_local(1, 0));
        let step = grid.cell_size_m / 2.0_f64.sqrt();
        assert!(
            close(b.0 - a.0, step, 1e-3) && close(b.1 - a.1, step, 1e-3),
            "{a:?} {b:?}"
        );
        let c = grid.cell_center_local(0, 1);
        assert!(
            close(c.0 - a.0, step, 1e-3) && close(c.1 - a.1, -step, 1e-3),
            "{c:?}"
        );
    }

    #[test]
    fn cell_lat_lon_round_trips() {
        let grid = Grid::new(&footprint((-4_000.0, 0.0), (4_000.0, 1_000.0)), detail(32));
        for (col, row) in [(0, 0), (31, 0), (12, 7), (31, grid.rows - 1)] {
            let (lat, lon) = grid.cell_center_lat_lon(col, row);
            let (x, y) = grid.footprint.projection.project(lat, lon);
            let (ex, ey) = grid.cell_center_local(col, row);
            assert!((x - ex).hypot(y - ey) < 1e-6);
        }
    }

    #[test]
    fn local_points_map_back_to_cells() {
        let grid = Grid::new(
            &footprint((-4_000.0, -1_500.0), (4_000.0, 2_500.0)),
            detail(40),
        );
        for (col, row) in [(0, 0), (39, 0), (17, 9), (39, grid.rows - 1)] {
            let (x, y) = grid.cell_center_local(col, row);
            let (c, r) = grid.local_to_cell(x, y);
            assert!(close(c, f64::from(col) + 0.5, 1e-9) && close(r, f64::from(row) + 0.5, 1e-9));
        }
        let [corner, ..] = grid.footprint.corners();
        let (c, r) = grid.local_to_cell(corner.0, corner.1);
        assert!(
            close(c, 0.0, 1e-9) && close(r, f64::from(grid.rows), 1e-9),
            "{c} {r}"
        );
    }

    #[test]
    fn zoom_matches_day_hikes_and_long_treks() {
        for lat in [47.0, 50.0, 53.0] {
            assert_eq!(dem_zoom(30.0, lat), 13, "30 m at {lat}°");
            assert_eq!(dem_zoom(250.0, lat), 10, "250 m at {lat}°");
        }
        assert_eq!(dem_zoom(30.0, 0.0), 14);
    }

    #[test]
    fn zoom_is_clamped_to_available_tiles() {
        assert_eq!(dem_zoom(1.0, 50.0), MAX_DEM_ZOOM);
        assert_eq!(dem_zoom(0.0, 50.0), MAX_DEM_ZOOM);
        assert_eq!(dem_zoom(1_000_000.0, 50.0), 0);
    }

    #[test]
    fn ground_resolution_halves_per_zoom() {
        assert!(close(ground_resolution_m(0, 0.0), 156_543.033_928, 1e-3));
        assert!(close(
            ground_resolution_m(13, 60.0),
            156_543.033_928 / 8192.0 / 2.0,
            1e-6
        ));
    }

    #[test]
    fn grid_zoom_uses_its_cell_size_and_latitude() {
        let grid = Grid::new(&footprint((0.0, -5_000.0), (0.0, 5_000.0)), DEFAULT_DETAIL);
        assert_eq!(grid.dem_zoom(), dem_zoom(grid.cell_size_m, 46.5));
    }
}
