/// Elevations in metres for every grid cell, row-major with row 0 at the
/// grid's +v edge
#[derive(Debug, Clone, PartialEq)]
pub struct Heightmap {
    pub cols: u32,
    pub rows: u32,
    pub heights_m: Vec<f32>,
}

impl Heightmap {
    pub fn get(&self, col: u32, row: u32) -> f32 {
        self.heights_m[(row * self.cols + col) as usize]
    }

    pub fn range(&self) -> (f32, f32) {
        self.heights_m
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), &h| {
                (min.min(h), max.max(h))
            })
    }

    /// Lowest cell black, highest white
    pub fn greyscale(&self) -> Vec<u8> {
        let (min, max) = self.range();
        let span = max - min;
        self.heights_m
            .iter()
            .map(|&h| {
                if span > 0.0 {
                    ((h - min) / span * 255.0).round() as u8
                } else {
                    0
                }
            })
            .collect()
    }

    /// Grey shading lit from the image's top-left at 45° (Horn gradients),
    /// with heights scaled by `exaggeration`
    pub fn hillshade(&self, cell_size_m: f64, exaggeration: f64) -> Vec<u8> {
        let (cols, rows) = (self.cols as i64, self.rows as i64);
        let at = |col: i64, row: i64| {
            f64::from(self.get(col.clamp(0, cols - 1) as u32, row.clamp(0, rows - 1) as u32))
        };
        let scale = exaggeration / (8.0 * cell_size_m);
        let light = {
            let (sin, cos) = 45f64.to_radians().sin_cos();
            let flat = cos / 2f64.sqrt();
            (-flat, flat, sin)
        };

        let mut shade = Vec::with_capacity(self.heights_m.len());
        for row in 0..rows {
            for col in 0..cols {
                let h = |dc, dr| at(col + dc, row + dr);
                let d_col =
                    (h(1, -1) + 2.0 * h(1, 0) + h(1, 1)) - (h(-1, -1) + 2.0 * h(-1, 0) + h(-1, 1));
                let d_row =
                    (h(-1, 1) + 2.0 * h(0, 1) + h(1, 1)) - (h(-1, -1) + 2.0 * h(0, -1) + h(1, -1));
                // Image y points down, so the normal's "up" component flips sign
                let normal = (-d_col * scale, d_row * scale, 1.0);
                let length = (normal.0 * normal.0 + normal.1 * normal.1 + 1.0).sqrt();
                let lit = (normal.0 * light.0 + normal.1 * light.1 + normal.2 * light.2) / length;
                shade.push((lit.max(0.0) * 255.0).round() as u8);
            }
        }
        shade
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heightmap(cols: u32, rows: u32, f: impl Fn(u32, u32) -> f32) -> Heightmap {
        let heights_m = (0..rows)
            .flat_map(|row| (0..cols).map(move |col| (col, row)))
            .map(|(col, row)| f(col, row))
            .collect();
        Heightmap {
            cols,
            rows,
            heights_m,
        }
    }

    #[test]
    fn indexes_row_major() {
        let map = heightmap(3, 2, |col, row| (row * 10 + col) as f32);
        assert_eq!(map.get(2, 0), 2.0);
        assert_eq!(map.get(1, 1), 11.0);
        assert_eq!(map.range(), (0.0, 12.0));
    }

    #[test]
    fn greyscale_stretches_the_range() {
        let map = heightmap(3, 1, |col, _| 100.0 + 50.0 * col as f32);
        assert_eq!(map.greyscale(), [0, 128, 255]);
        assert_eq!(heightmap(2, 2, |_, _| 7.0).greyscale(), [0; 4]);
    }

    #[test]
    fn flat_ground_is_evenly_lit() {
        let shade = heightmap(4, 3, |_, _| 500.0).hillshade(30.0, 1.0);
        assert!(shade.iter().all(|&s| s == 180), "{shade:?}");
    }

    #[test]
    fn slopes_facing_the_light_are_brighter() {
        let facing = heightmap(5, 5, |col, row| (col + row) as f32 * 5.0);
        let away = heightmap(5, 5, |col, row| -((col + row) as f32) * 5.0);
        let (lit, dark) = (
            facing.hillshade(10.0, 1.0)[12],
            away.hillshade(10.0, 1.0)[12],
        );
        assert!(lit > 180 && dark < 180, "{lit} {dark}");
        let steeper = facing.hillshade(10.0, 3.0)[12];
        assert!(steeper != lit, "exaggeration changes the shading");
    }
}
