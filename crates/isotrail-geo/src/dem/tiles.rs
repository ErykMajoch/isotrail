use std::f64::consts::PI;
use std::fmt;

pub const TILE_SIZE: u32 = 256;
pub const MAX_MERCATOR_LAT: f64 = 85.051_128_779_806_59;

/// A Web-Mercator (slippy map) tile
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileId {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

impl TileId {
    /// The tile and in-tile offset of a global pixel, with x wrapped round
    /// the antimeridian and y clamped at the poles
    pub fn containing(z: u8, px: i64, py: i64) -> (TileId, usize) {
        let size = i64::from(TILE_SIZE);
        let world = size << z;
        let (px, py) = (px.rem_euclid(world), py.clamp(0, world - 1));
        let tile = TileId {
            z,
            x: (px / size) as u32,
            y: (py / size) as u32,
        };
        (tile, ((py % size) * size + px % size) as usize)
    }
}

impl fmt::Display for TileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.z, self.x, self.y)
    }
}

/// Global pixel coordinates at zoom `z`, from the north-west corner of the world
pub fn lat_lon_to_pixel(lat: f64, lon: f64, z: u8) -> (f64, f64) {
    let world = world_px(z);
    let lat = lat.clamp(-MAX_MERCATOR_LAT, MAX_MERCATOR_LAT).to_radians();
    let x = (lon + 180.0) / 360.0 * world;
    let y = (1.0 - lat.tan().asinh() / PI) / 2.0 * world;
    (x, y)
}

pub fn pixel_to_lat_lon(px: f64, py: f64, z: u8) -> (f64, f64) {
    let world = world_px(z);
    let lon = px / world * 360.0 - 180.0;
    let lat = (PI * (1.0 - 2.0 * py / world)).sinh().atan().to_degrees();
    (lat, lon)
}

fn world_px(z: u8) -> f64 {
    f64::from(TILE_SIZE) * 2f64.powi(z.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile_of(lat: f64, lon: f64, z: u8) -> TileId {
        let (px, py) = lat_lon_to_pixel(lat, lon, z);
        TileId::containing(z, px.floor() as i64, py.floor() as i64).0
    }

    #[test]
    fn known_tiles() {
        assert_eq!(tile_of(0.0, 0.0, 0), TileId { z: 0, x: 0, y: 0 });
        assert_eq!(tile_of(0.0, 0.0, 1), TileId { z: 1, x: 1, y: 1 });
        assert_eq!(tile_of(1.0, -1.0, 1), TileId { z: 1, x: 0, y: 0 });
        assert_eq!(
            tile_of(51.5074, -0.1278, 10),
            TileId {
                z: 10,
                x: 511,
                y: 340
            }
        );
        assert_eq!(
            tile_of(35.6762, 139.6503, 10),
            TileId {
                z: 10,
                x: 909,
                y: 403
            }
        );
        assert_eq!(
            tile_of(-33.8688, 151.2093, 8),
            TileId {
                z: 8,
                x: 235,
                y: 153
            }
        );
    }

    #[test]
    fn world_corners_and_centre() {
        assert_eq!(lat_lon_to_pixel(0.0, 0.0, 3), (1024.0, 1024.0));
        let (x, y) = lat_lon_to_pixel(MAX_MERCATOR_LAT, -180.0, 2);
        assert!(x.abs() < 1e-9 && y.abs() < 1e-6, "{x} {y}");
        let (_, y) = lat_lon_to_pixel(-90.0, 0.0, 2);
        assert!((y - 1024.0).abs() < 1e-6, "{y}");
    }

    #[test]
    fn pixels_round_trip() {
        for (lat, lon) in [(0.0, 0.0), (47.3, 10.9), (-41.2, 174.8), (69.6, -18.9)] {
            for z in [0, 7, 13, 15] {
                let (px, py) = lat_lon_to_pixel(lat, lon, z);
                let (lat2, lon2) = pixel_to_lat_lon(px, py, z);
                assert!((lat - lat2).abs() < 1e-9 && (lon - lon2).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn x_wraps_and_y_clamps() {
        let world = i64::from(TILE_SIZE) << 2;
        let (tile, offset) = TileId::containing(2, -1, 5);
        assert_eq!(tile, TileId { z: 2, x: 3, y: 0 });
        assert_eq!(offset, 5 * 256 + 255);
        assert_eq!(TileId::containing(2, world, 0), TileId::containing(2, 0, 0));
        let (tile, offset) = TileId::containing(2, 0, world + 40);
        assert_eq!(tile.y, 3);
        assert_eq!(offset, 255 * 256);
        let (tile, _) = TileId::containing(2, 0, -3);
        assert_eq!(tile.y, 0);
    }

    #[test]
    fn displays_as_a_url_path() {
        assert_eq!(
            TileId {
                z: 12,
                x: 2200,
                y: 1400
            }
            .to_string(),
            "12/2200/1400"
        );
    }
}
