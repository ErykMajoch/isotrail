use glam::Vec2;

/// The top of a terrain mesh as a height field over fractional grid
/// positions, in the slab frame's exaggerated metres
#[derive(Clone, Debug, PartialEq)]
pub enum Surface {
    /// One flat top per cell, row-major
    Steps { cols: usize, tops: Vec<f32> },
    /// Heights at the `(cols + 1) × (rows + 1)` cell corners, row-major, with
    /// each cell split from its bottom-left to its top-right corner
    Triangles { cols: usize, corners: Vec<f32> },
}

impl Default for Surface {
    fn default() -> Surface {
        Surface::Steps {
            cols: 0,
            tops: Vec::new(),
        }
    }
}

impl Surface {
    /// Height at `at` of the face that holds `inside`, so a point on a step's
    /// edge can take either side's height
    pub fn height(&self, inside: Vec2, at: Vec2) -> f32 {
        match self {
            Surface::Steps { cols, tops } => {
                let rows = tops.len().checked_div(*cols).unwrap_or(0);
                if rows == 0 {
                    return 0.0;
                }
                let (col, row) = cell(inside, *cols, rows);
                tops[row * cols + col]
            }
            Surface::Triangles { cols, corners } => {
                let rows = (corners.len() / (cols + 1)).saturating_sub(1);
                if *cols == 0 || rows == 0 {
                    return 0.0;
                }
                let (col, row) = cell(inside, *cols, rows);
                let corner = |c: usize, r: usize| corners[r * (cols + 1) + c];
                let (top_left, top_right) = (corner(col, row), corner(col + 1, row));
                let (bottom_left, bottom_right) = (corner(col, row + 1), corner(col + 1, row + 1));
                let origin = Vec2::new(col as f32, row as f32);
                let (face, at) = (inside - origin, at - origin);
                if face.x + face.y >= 1.0 {
                    bottom_right
                        + (1.0 - at.x) * (bottom_left - bottom_right)
                        + (1.0 - at.y) * (top_right - bottom_right)
                } else {
                    top_left + at.x * (top_right - top_left) + at.y * (bottom_left - top_left)
                }
            }
        }
    }

    /// Where, as shares of `from` → `to`, the segment crosses from one face
    /// to the next, in order
    pub fn crossings(&self, from: Vec2, to: Vec2) -> Vec<f32> {
        let mut ts = Vec::new();
        let mut cross = |a: f32, b: f32| {
            if a == b {
                return;
            }
            let (lo, hi) = (a.min(b), a.max(b));
            let mut k = lo.floor() + 1.0;
            while k < hi {
                ts.push((k - a) / (b - a));
                k += 1.0;
            }
        };
        cross(from.x, to.x);
        cross(from.y, to.y);
        if matches!(self, Surface::Triangles { .. }) {
            cross(from.x + from.y, to.x + to.y);
        }
        ts.retain(|t| *t > 0.0 && *t < 1.0);
        ts.sort_by(f32::total_cmp);
        ts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        ts
    }
}

fn cell(p: Vec2, cols: usize, rows: usize) -> (usize, usize) {
    let index = |v: f32, n: usize| (v.floor().max(0.0) as usize).min(n - 1);
    (index(p.x, cols), index(p.y, rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_take_the_height_of_the_cell_holding_the_point() {
        let surface = Surface::Steps {
            cols: 2,
            tops: vec![1.0, 2.0, 3.0, 4.0],
        };
        let edge = Vec2::new(1.0, 0.5);
        assert_eq!(surface.height(Vec2::new(0.5, 0.5), edge), 1.0);
        assert_eq!(surface.height(Vec2::new(1.5, 0.5), edge), 2.0);
        assert_eq!(surface.height(Vec2::new(1.5, 1.5), edge), 4.0);
        // Outside the grid, the nearest edge cell
        assert_eq!(surface.height(Vec2::new(-3.0, 9.0), edge), 3.0);
        assert_eq!(Surface::default().height(edge, edge), 0.0);
    }

    #[test]
    fn triangles_interpolate_their_corners() {
        // One cell: top-left 0, top-right 10, bottom-left 20, bottom-right 50
        let surface = Surface::Triangles {
            cols: 1,
            corners: vec![0.0, 10.0, 20.0, 50.0],
        };
        let height = |x: f32, y: f32| surface.height(Vec2::new(x, y), Vec2::new(x, y));
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(close(height(0.0, 0.0), 0.0));
        assert!(close(height(0.99, 0.0), 9.9));
        assert!(close(height(1.0, 1.0), 50.0));
        assert!(close(height(0.25, 0.25), 7.5));
        assert!(close(height(0.75, 0.75), 32.5));
        // The two triangles meet along the diagonal
        assert!((height(0.5, 0.4999) - height(0.5, 0.5001)).abs() < 0.01);
    }

    #[test]
    fn crossings_cover_cell_edges_and_diagonals() {
        let steps = Surface::default();
        let from = Vec2::new(0.5, 0.5);
        let to = Vec2::new(2.5, 1.5);
        assert_eq!(steps.crossings(from, to), [0.25, 0.5, 0.75]);
        assert_eq!(steps.crossings(to, from), [0.25, 0.5, 0.75]);
        let triangles = Surface::Triangles {
            cols: 0,
            corners: Vec::new(),
        };
        // x + y goes from 1 to 4, crossing 2 and 3 at a third and two thirds
        let ts = triangles.crossings(from, to);
        assert_eq!(ts.len(), 5, "{ts:?}");
        assert!((ts[0] - 0.25).abs() < 1e-6 && (ts[1] - 1.0 / 3.0).abs() < 1e-6);
        assert!(steps.crossings(from, from).is_empty());
        assert!(steps.crossings(from, Vec2::new(0.9, 0.6)).is_empty());
    }
}
