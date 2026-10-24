use super::percentile;
use crate::Heightmap;

/// Low, because resampling blends the higher shore into the cells along it
pub const WATER_LEVEL_PERCENTILE: f64 = 0.1;
pub(super) const NO_LAKE: u32 = u32::MAX;

/// A lake or reservoir with a level surface
#[derive(Debug, Clone, PartialEq)]
pub struct WaterBody {
    pub level_m: f32,
    /// Row-major cell indices, ascending
    pub cells: Vec<u32>,
}

/// Splits each lake's cells into 4-connected bodies and flattens every body
/// to its level; `lakes` holds, per cell, the lake that claimed it first
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
        let mut heights: Vec<f32> = cells
            .iter()
            .map(|&c| heightmap.heights_m[c as usize])
            .collect();
        let level_m = percentile(&mut heights, WATER_LEVEL_PERCENTILE);
        for &c in &cells {
            heightmap.heights_m[c as usize] = level_m;
        }
        bodies.push(WaterBody { level_m, cells });
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
    fn diagonal_neighbours_are_separate_bodies() {
        let mut map = heightmap(2, vec![1.0, 2.0, 3.0, 4.0]);
        let bodies = level_lakes(&[0, N, N, 0], &mut map);
        assert_eq!(bodies.len(), 2);
        assert!(level_lakes(&[N; 4], &mut map).is_empty());
    }
}
