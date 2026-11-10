use crate::Surface;

const DIRECTIONS: usize = 8;
/// The fewest and most cells looked along for a horizon
const REACH_CELLS: (usize, usize) = (8, 40);

/// How much of the sky each cell sees, from 0 (none) to 255 (all of it):
/// the terrain around it, looked at along eight directions, hides the part
/// of the sky below its highest horizon
pub(crate) fn sky_view(surface: &Surface, cell_m: f32) -> (usize, usize, Vec<u8>) {
    let cols = surface.cols();
    let heights = cell_heights(surface);
    let rows = heights.len().checked_div(cols).unwrap_or(0);
    if cols == 0 || rows == 0 {
        return (1, 1, vec![255]);
    }
    let reach = (cols.max(rows) / 6).clamp(REACH_CELLS.0, REACH_CELLS.1);
    let at = |c: isize, r: isize| {
        let c = c.clamp(0, cols as isize - 1) as usize;
        let r = r.clamp(0, rows as isize - 1) as usize;
        heights[r * cols + c]
    };
    let mut view = Vec::with_capacity(cols * rows);
    for row in 0..rows {
        for col in 0..cols {
            let here = heights[row * cols + col];
            let mut hidden = 0.0;
            for k in 0..DIRECTIONS {
                let angle = k as f32 * std::f32::consts::TAU / DIRECTIONS as f32;
                let (dx, dy) = (angle.cos(), angle.sin());
                let mut steepest = 0.0f32;
                for step in 1..=reach {
                    let (x, y) = (dx * step as f32, dy * step as f32);
                    let (c, r) = (col as f32 + x, row as f32 + y);
                    if c < 0.0 || r < 0.0 || c >= cols as f32 || r >= rows as f32 {
                        break;
                    }
                    let rise = at(c.round() as isize, r.round() as isize) - here;
                    steepest = steepest.max(rise / (step as f32 * cell_m));
                }
                // The sine of the horizon's elevation
                hidden += steepest / (1.0 + steepest * steepest).sqrt();
            }
            let seen = 1.0 - hidden / DIRECTIONS as f32;
            view.push((seen.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    (cols, rows, view)
}

fn cell_heights(surface: &Surface) -> Vec<f32> {
    match surface {
        Surface::Steps { tops, .. } => tops.clone(),
        Surface::Triangles { cols, corners } => {
            let rows = (corners.len() / (cols + 1)).saturating_sub(1);
            let corner = |c: usize, r: usize| corners[r * (cols + 1) + c];
            (0..rows)
                .flat_map(|r| (0..*cols).map(move |c| (r, c)))
                .map(|(r, c)| {
                    (corner(c, r) + corner(c + 1, r) + corner(c, r + 1) + corner(c + 1, r + 1))
                        / 4.0
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(cols: usize, tops: Vec<f32>) -> Surface {
        Surface::Steps { cols, tops }
    }

    #[test]
    fn open_ground_sees_the_whole_sky() {
        let (cols, rows, view) = sky_view(&steps(10, vec![5.0; 100]), 10.0);
        assert_eq!((cols, rows), (10, 10));
        assert!(view.iter().all(|&v| v == 255));
    }

    #[test]
    fn a_pit_sees_less_sky_than_the_rim_around_it() {
        let mut tops = vec![50.0; 21 * 21];
        tops[10 * 21 + 10] = 0.0;
        let (_, _, view) = sky_view(&steps(21, tops), 10.0);
        let (pit, rim) = (view[10 * 21 + 10], view[10 * 21 + 12]);
        assert!(pit < 100, "{pit}");
        assert_eq!(rim, 255);
    }

    #[test]
    fn the_foot_of_a_cliff_sees_less_sky_than_its_top() {
        let tops: Vec<f32> = (0..20 * 20)
            .map(|i| if i % 20 < 10 { 0.0 } else { 100.0 })
            .collect();
        let (_, _, view) = sky_view(&steps(20, tops), 10.0);
        let (foot, top, far) = (view[5 * 20 + 9], view[5 * 20 + 10], view[5 * 20 + 4]);
        assert!(foot < far && far < top, "{foot} {far} {top}");
    }
}
