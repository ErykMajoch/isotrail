mod error;
mod heightmap;
mod terrarium;
mod tiles;

pub use error::DemError;
pub use heightmap::Heightmap;
pub use terrarium::{TERRARIUM_URL, TerrariumSource};
pub use tiles::{MAX_MERCATOR_LAT, TILE_SIZE, TileId, lat_lon_to_pixel, pixel_to_lat_lon};

use crate::Grid;

/// Where terrain elevations come from, sampled onto a grid's cell centres
pub trait DemSource {
    fn heightmap(&self, grid: &Grid) -> Result<Heightmap, DemError>;
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::num::NonZeroU32;

    use isotrail_track::{Segment, Track, TrackPoint};
    use png::ColorType;

    use super::terrarium::tests::{encode, terrarium_rgb};
    use super::*;
    use crate::{Footprint, LocalProjection, RecordedFetcher};

    fn grid(from: (f64, f64), to: (f64, f64), detail: u32) -> Grid {
        let projection = LocalProjection::new(46.5, 10.0);
        let points = [from, to]
            .map(|(x, y)| {
                let (lat, lon) = projection.unproject(x, y);
                TrackPoint {
                    lat,
                    lon,
                    ele: None,
                    time: None,
                }
            })
            .to_vec();
        let track = Track {
            segments: vec![Segment { points }],
            ..Track::default()
        };
        let footprint = Footprint::from_track(&track).unwrap();
        Grid::new(&footprint, NonZeroU32::new(detail).unwrap())
    }

    /// A tilted plane in global pixel space, on the terrarium 1/256 m step
    fn plane(origin: (f64, f64), px: f64, py: f64) -> f64 {
        1000.0 + 0.5 * (px - origin.0) - 0.25 * (py - origin.1)
    }

    #[test]
    fn resamples_a_plane_exactly_across_tile_seams() {
        let grid = grid((-2_000.0, -9_500.0), (4_000.0, -5_000.0), 48);
        let zoom = grid.dem_zoom();
        let tiles = TerrariumSource::<RecordedFetcher>::tiles(&grid);
        assert!(tiles.len() >= 4, "{tiles:?}");
        let first = tiles[0];
        let origin = (
            f64::from(first.x * TILE_SIZE),
            f64::from(first.y * TILE_SIZE),
        );

        let temp = tempfile::tempdir().unwrap();
        let fetcher = RecordedFetcher::new(temp.path());
        for &tile in &tiles {
            let mut rgb = Vec::with_capacity((TILE_SIZE * TILE_SIZE * 3) as usize);
            for y in 0..TILE_SIZE {
                for x in 0..TILE_SIZE {
                    let (px, py) = (tile.x * TILE_SIZE + x, tile.y * TILE_SIZE + y);
                    rgb.extend(terrarium_rgb(plane(origin, px.into(), py.into())));
                }
            }
            let url = TerrariumSource::<RecordedFetcher>::url(tile);
            let path = fetcher.path_for(&url, None).unwrap();
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, encode(TILE_SIZE, TILE_SIZE, ColorType::Rgb, &rgb)).unwrap();
        }

        let heightmap = TerrariumSource::new(&fetcher).heightmap(&grid).unwrap();
        assert_eq!(heightmap.heights_m.len(), (grid.cols * grid.rows) as usize);
        let mut straddling = 0;
        for row in 0..grid.rows {
            for col in 0..grid.cols {
                let (lat, lon) = grid.cell_center_lat_lon(col, row);
                let (px, py) = lat_lon_to_pixel(lat, lon, zoom);
                let (px, py) = (px - 0.5, py - 0.5);
                let expected = plane(origin, px, py);
                let got = f64::from(heightmap.get(col, row));
                assert!(
                    (got - expected).abs() < 1e-3,
                    "{col},{row}: {got} vs {expected}"
                );
                let tile = |x: f64, y: f64| TileId::containing(zoom, x as i64, y as i64).0;
                if tile(px.floor(), py.floor()) != tile(px.floor() + 1.0, py.floor() + 1.0) {
                    straddling += 1;
                }
            }
        }
        assert!(straddling > 0, "no cell sampled across a seam");
    }

    #[test]
    fn rotated_slabs_skip_unneeded_corner_tiles() {
        let grid = grid((-8_000.0, -8_000.0), (8_000.0, 8_000.0), 256);
        let tiles = TerrariumSource::<RecordedFetcher>::tiles(&grid);
        let (xs, ys): (Vec<_>, Vec<_>) = tiles.iter().map(|t| (t.x, t.y)).unzip();
        let span = |v: &[u32]| v.iter().max().unwrap() - v.iter().min().unwrap() + 1;
        assert!(
            (tiles.len() as u32) < span(&xs) * span(&ys),
            "{} tiles in a {} × {} box",
            tiles.len(),
            span(&xs),
            span(&ys)
        );
        let mut sorted = tiles.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted, tiles);
    }

    #[test]
    fn missing_tiles_name_the_tile() {
        let grid = grid((0.0, 0.0), (1_000.0, 0.0), 16);
        let temp = tempfile::tempdir().unwrap();
        let error = TerrariumSource::new(RecordedFetcher::new(temp.path()))
            .heightmap(&grid)
            .unwrap_err();
        let tile = TerrariumSource::<RecordedFetcher>::tiles(&grid)[0];
        assert!(matches!(&error, DemError::Fetch { tile: t, .. } if *t == tile));
        assert!(error.to_string().contains(&tile.to_string()), "{error}");
    }
}
