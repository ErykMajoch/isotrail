pub use isotrail_bundle::WaterBody;

use super::percentile;
use crate::Heightmap;

/// Low, because resampling blends the higher shore into the cells along it
pub const WATER_LEVEL_PERCENTILE: f64 = 0.1;
pub(super) const NO_LAKE: u32 = u32::MAX;
/// The id of the sea's cells in place of a lake's
pub(super) const SEA: u32 = u32::MAX - 1;
pub const SEA_LEVEL_M: f32 = 0.0;

/// Splits each lake's cells into 4-connected bodies and flattens every body
/// to its level; `lakes` holds, per cell, the lake that claimed it first,
/// or [`SEA`], which lies at [`SEA_LEVEL_M`] whatever the sea bed's depth
///
/// Bodies never join across lakes, so two reservoirs either side of a
/// narrow dam keep their own levels
pub(super) fn level_lakes(lakes: &[u32], heightmap: &mut Heightmap) -> Vec<WaterBody> {
    let (cols, rows) = (heightmap.cols, heightmap.rows);
    let mut seen = vec![false; lakes.len()];
    let mut bodies = Vec::new();
    for start in 0..lakes.len() {
        let lake = lakes[start];
        if lake == NO_LAKE || seen[start] {
            continue;
        }
        seen[start] = true;
        let mut cells = vec![start as u32];
        let mut next = 0;
        while let Some(&cell) = cells.get(next) {
            next += 1;
            let (col, row) = (cell % cols, cell / cols);
            let neighbours = [
                (col > 0).then(|| cell - 1),
                (col + 1 < cols).then(|| cell + 1),
                (row > 0).then(|| cell - cols),
                (row + 1 < rows).then(|| cell + cols),
            ];
            for n in neighbours.into_iter().flatten() {
                if !seen[n as usize] && lakes[n as usize] == lake {
                    seen[n as usize] = true;
                    cells.push(n);
                }
            }
        }
        cells.sort_unstable();
        let sea = lake == SEA;
        let level_m = if sea {
            SEA_LEVEL_M
        } else {
            let mut heights: Vec<f32> = cells
                .iter()
                .map(|&c| heightmap.heights_m[c as usize])
                .collect();
            percentile(&mut heights, WATER_LEVEL_PERCENTILE)
        };
        for &c in &cells {
            heightmap.heights_m[c as usize] = level_m;
        }
        bodies.push(WaterBody {
            level_m,
            cells,
            sea,
        });
    }
    bodies
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: u32 = NO_LAKE;

    fn heightmap(cols: u32, heights_m: Vec<f32>) -> Heightmap {
        Heightmap {
            cols,
            rows: heights_m.len() as u32 / cols,
            heights_m,
        }
    }

    #[test]
    fn levels_each_body_at_a_low_percentile() {
        let mut map = heightmap(5, (0..20).map(|i| 100.0 + i as f32).collect());
        let lakes = [
            N, 0, 0, 0, N, //
            N, 0, 0, 0, N, //
            N, 0, 0, 0, N, //
            N, 0, 0, 0, 0,
        ];
        let bodies = level_lakes(&lakes, &mut map);
        let [body] = &bodies[..] else {
            panic!("{bodies:?}");
        };
        assert_eq!(body.cells, [1, 2, 3, 6, 7, 8, 11, 12, 13, 16, 17, 18, 19]);
        assert_eq!(body.level_m, 102.0);
        for (i, &h) in map.heights_m.iter().enumerate() {
            let expected = if lakes[i] == 0 {
                102.0
            } else {
                100.0 + i as f32
            };
            assert_eq!(h, expected, "cell {i}");
        }
    }

    #[test]
    fn neighbouring_lakes_and_separate_parts_stay_apart() {
        let mut map = heightmap(
            6,
            vec![
                50.0, 50.0, 20.0, 20.0, 9.0, 9.0, //
                50.0, 50.0, 20.0, 20.0, 9.0, 9.0,
            ],
        );
        let lakes = [
            0, 0, 1, 1, N, 0, //
            0, 0, 1, 1, N, 0,
        ];
        let bodies = level_lakes(&lakes, &mut map);
        let levels: Vec<_> = bodies
            .iter()
            .map(|b| (b.cells.clone(), b.level_m))
            .collect();
        assert_eq!(
            levels,
            [
                (vec![0, 1, 6, 7], 50.0),
                (vec![2, 3, 8, 9], 20.0),
                (vec![5, 11], 9.0),
            ]
        );
    }

    #[test]
    fn the_sea_lies_at_sea_level_over_any_sea_bed() {
        let mut map = heightmap(4, vec![-40.0, 3.0, 12.0, 30.0, -900.0, -2.0, 8.0, 30.0]);
        let lakes = [
            SEA, SEA, 0, N, //
            SEA, SEA, 0, N,
        ];
        let bodies = level_lakes(&lakes, &mut map);
        let summary: Vec<_> = bodies
            .iter()
            .map(|b| (b.cells.clone(), b.level_m, b.sea))
            .collect();
        assert_eq!(
            summary,
            [(vec![0, 1, 4, 5], 0.0, true), (vec![2, 6], 8.0, false)]
        );
        assert_eq!(map.heights_m, [0.0, 0.0, 8.0, 30.0, 0.0, 0.0, 8.0, 30.0]);
    }

    #[test]
    fn diagonal_neighbours_are_separate_bodies() {
        let mut map = heightmap(2, vec![1.0, 2.0, 3.0, 4.0]);
        let bodies = level_lakes(&[0, N, N, 0], &mut map);
        assert_eq!(bodies.len(), 2);
        assert!(level_lakes(&[N; 4], &mut map).is_empty());
    }
}
