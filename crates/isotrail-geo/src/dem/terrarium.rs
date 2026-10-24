use std::collections::{BTreeSet, HashMap};
use std::io::Cursor;

use png::{ColorType, Transformations};
use tracing::{debug, info};

use super::{DemError, DemSource, Heightmap, TILE_SIZE, TileId, lat_lon_to_pixel};
use crate::{Fetcher, Grid};

pub const TERRARIUM_URL: &str =
    "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png";

/// AWS Terrain Tiles in the terrarium encoding, where a pixel's height is
/// `R·256 + G + B/256 − 32768` metres
#[derive(Debug, Clone)]
pub struct TerrariumSource<F> {
    fetcher: F,
}

impl<F: Fetcher> TerrariumSource<F> {
    pub fn new(fetcher: F) -> TerrariumSource<F> {
        TerrariumSource { fetcher }
    }

    pub fn url(tile: TileId) -> String {
        TERRARIUM_URL
            .replace("{z}", &tile.z.to_string())
            .replace("{x}", &tile.x.to_string())
            .replace("{y}", &tile.y.to_string())
    }

    /// Every tile the grid's bilinear samples touch, in a stable order. Only
    /// these are fetched, so a rotated slab skips the corners of its bbox
    pub fn tiles(grid: &Grid) -> Vec<TileId> {
        let zoom = grid.dem_zoom();
        let tiles: BTreeSet<TileId> = sample_points(grid, zoom)
            .flat_map(|(px, py)| neighbours(px, py).map(|(ix, iy, _)| (ix, iy)))
            .map(|(ix, iy)| TileId::containing(zoom, ix, iy).0)
            .collect();
        tiles.into_iter().collect()
    }

    fn fetch(&self, tile: TileId) -> Result<Vec<f32>, DemError> {
        let bytes = self
            .fetcher
            .get(&Self::url(tile))
            .map_err(|source| DemError::Fetch { tile, source })?;
        debug!(%tile, bytes = bytes.len(), "elevation tile");
        decode(tile, &bytes)
    }
}

impl<F: Fetcher> DemSource for TerrariumSource<F> {
    fn heightmap(&self, grid: &Grid) -> Result<Heightmap, DemError> {
        let zoom = grid.dem_zoom();
        let tiles = Self::tiles(grid);
        info!(
            zoom,
            tiles = tiles.len(),
            "fetching terrarium elevation tiles"
        );
        let mosaic = tiles
            .into_iter()
            .map(|tile| Ok((tile, self.fetch(tile)?)))
            .collect::<Result<HashMap<_, _>, DemError>>()?;

        let heights_m = sample_points(grid, zoom)
            .map(|(px, py)| {
                neighbours(px, py)
                    .into_iter()
                    .map(|(ix, iy, weight)| {
                        let (tile, offset) = TileId::containing(zoom, ix, iy);
                        weight * f64::from(mosaic[&tile][offset])
                    })
                    .sum::<f64>() as f32
            })
            .collect();
        Ok(Heightmap {
            cols: grid.cols,
            rows: grid.rows,
            heights_m,
        })
    }
}

/// Cell centres as global pixel coordinates, shifted so whole numbers land
/// on pixel centres
fn sample_points(grid: &Grid, zoom: u8) -> impl Iterator<Item = (f64, f64)> {
    (0..grid.rows)
        .flat_map(move |row| (0..grid.cols).map(move |col| (col, row)))
        .map(move |(col, row)| {
            let (lat, lon) = grid.cell_center_lat_lon(col, row);
            let (px, py) = lat_lon_to_pixel(lat, lon, zoom);
            (px - 0.5, py - 0.5)
        })
}

fn neighbours(px: f64, py: f64) -> [(i64, i64, f64); 4] {
    let (x0, y0) = (px.floor(), py.floor());
    let (fx, fy) = (px - x0, py - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    [
        (x0, y0, (1.0 - fx) * (1.0 - fy)),
        (x0 + 1, y0, fx * (1.0 - fy)),
        (x0, y0 + 1, (1.0 - fx) * fy),
        (x0 + 1, y0 + 1, fx * fy),
    ]
}

fn decode(tile: TileId, bytes: &[u8]) -> Result<Vec<f32>, DemError> {
    let decode_error = |source| DemError::Decode { tile, source };
    let format_error = |reason: String| DemError::Format { tile, reason };

    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(decode_error)?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| format_error("the image is too large".into()))?;
    let mut buf = vec![0; size];
    let frame = reader.next_frame(&mut buf).map_err(decode_error)?;

    if (frame.width, frame.height) != (TILE_SIZE, TILE_SIZE) {
        return Err(format_error(format!(
            "{} × {} pixels instead of {TILE_SIZE} × {TILE_SIZE}",
            frame.width, frame.height
        )));
    }
    let channels = match frame.color_type {
        ColorType::Rgb => 3,
        ColorType::Rgba => 4,
        other => return Err(format_error(format!("{other:?} instead of RGB"))),
    };
    Ok(buf[..frame.buffer_size()]
        .chunks_exact(channels)
        .map(|px| f32::from(px[0]) * 256.0 + f32::from(px[1]) + f32::from(px[2]) / 256.0 - 32768.0)
        .collect())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub fn encode(width: u32, height: u32, color: ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(data).unwrap();
        writer.finish().unwrap();
        out
    }

    /// Exact for heights on a 1/256 m step within the terrarium range
    pub fn terrarium_rgb(height_m: f64) -> [u8; 3] {
        let v = ((height_m + 32768.0) * 256.0).round() as u32;
        [(v >> 16) as u8, (v >> 8) as u8, v as u8]
    }

    const TILE: TileId = TileId { z: 3, x: 1, y: 2 };

    #[test]
    fn decodes_known_pixels() {
        let mut rgb = vec![0; (TILE_SIZE * TILE_SIZE * 3) as usize];
        let samples = [(0, 0.0), (1, 1085.5), (2, -10.25), (65_535, 8848.0)];
        for (index, height) in samples {
            rgb[index * 3..index * 3 + 3].copy_from_slice(&terrarium_rgb(height));
        }
        rgb[3 * 3..3 * 3 + 3].copy_from_slice(&[128, 0, 1]);
        let heights = decode(TILE, &encode(256, 256, ColorType::Rgb, &rgb)).unwrap();
        assert_eq!(heights.len(), 65_536);
        for (index, height) in samples {
            assert_eq!(f64::from(heights[index]), height, "pixel {index}");
        }
        assert_eq!(heights[3], 1.0 / 256.0);
        assert_eq!(heights[4], -32768.0);
    }

    #[test]
    fn reads_rgba_tiles() {
        let mut rgba = vec![255; (TILE_SIZE * TILE_SIZE * 4) as usize];
        rgba[..4].copy_from_slice(&[128, 100, 128, 255]);
        let heights = decode(TILE, &encode(256, 256, ColorType::Rgba, &rgba)).unwrap();
        assert_eq!(heights[0], 100.5);
    }

    #[test]
    fn rejects_other_formats() {
        let small = encode(16, 16, ColorType::Rgb, &[0; 16 * 16 * 3]);
        assert!(matches!(decode(TILE, &small), Err(DemError::Format { .. })));
        let grey = encode(256, 256, ColorType::Grayscale, &[0; 256 * 256]);
        assert!(matches!(decode(TILE, &grey), Err(DemError::Format { .. })));
        assert!(matches!(
            decode(TILE, b"<html>not a tile</html>"),
            Err(DemError::Decode { .. })
        ));
    }

    #[test]
    fn urls_follow_the_template() {
        assert_eq!(
            TerrariumSource::<crate::RecordedFetcher>::url(TileId {
                z: 12,
                x: 2200,
                y: 1400
            }),
            "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/12/2200/1400.png"
        );
    }

    #[test]
    fn bilinear_weights_sum_to_one() {
        let n = neighbours(10.25, -3.75);
        assert_eq!((n[0].0, n[0].1), (10, -4));
        assert_eq!((n[3].0, n[3].1), (11, -3));
        assert!((n.iter().map(|n| n.2).sum::<f64>() - 1.0).abs() < 1e-12);
        assert!((n[0].2 - 0.75 * 0.75).abs() < 1e-12);
    }
}
