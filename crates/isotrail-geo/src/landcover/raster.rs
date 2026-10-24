//! Rasterising in fractional cell space, where cell (c, r) spans
//! [c, c + 1) × [r, r + 1)

/// Calls `cell` for every cell whose centre is inside the rings, even-odd,
/// so inner rings cut holes
pub(super) fn fill_polygon(
    rings: &[Vec<(f64, f64)>],
    cols: u32,
    rows: u32,
    mut cell: impl FnMut(u32, u32),
) {
    let (y_min, y_max) = rings
        .iter()
        .flatten()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &(_, y)| {
            (lo.min(y), hi.max(y))
        });
    // Rows whose centre r + 0.5 lies in [y_min, y_max)
    let first = (y_min - 0.5).ceil().max(0.0);
    let end = (y_max - 0.5).ceil().min(f64::from(rows));
    if first >= end {
        return;
    }
    let (first, end) = (first as u32, end as u32);

    let mut crossings = vec![Vec::new(); (end - first) as usize];
    for ring in rings {
        for (&a, &b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
            let (lo, hi) = if a.1 <= b.1 { (a, b) } else { (b, a) };
            let from = (lo.1 - 0.5).ceil().max(f64::from(first));
            let to = (hi.1 - 0.5).ceil().min(f64::from(end));
            let mut row = from;
            while row < to {
                let y = row + 0.5;
                let x = lo.0 + (y - lo.1) / (hi.1 - lo.1) * (hi.0 - lo.0);
                crossings[(row as u32 - first) as usize].push(x);
                row += 1.0;
            }
        }
    }

    for (offset, xs) in crossings.iter_mut().enumerate() {
        xs.sort_by(f64::total_cmp);
        let row = first + offset as u32;
        for &[enter, leave] in xs.as_chunks::<2>().0 {
            let from = (enter - 0.5).ceil().max(0.0);
            let to = (leave - 0.5).ceil().min(f64::from(cols));
            let mut col = from;
            while col < to {
                cell(col as u32, row);
                col += 1.0;
            }
        }
    }
}

/// Calls `cell` for every cell the polyline passes through, stepping one
/// axis at a time so the cells are 4-connected
pub(super) fn trace_line(
    points: &[(f64, f64)],
    cols: u32,
    rows: u32,
    mut cell: impl FnMut(u32, u32),
) {
    let (cols, rows) = (i64::from(cols), i64::from(rows));
    let mut visit = |c: i64, r: i64| {
        if (0..cols).contains(&c) && (0..rows).contains(&r) {
            cell(c as u32, r as u32);
        }
    };
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let outside = |lo: f64, hi: f64, n: i64| lo.max(hi) < 0.0 || lo.min(hi) >= n as f64;
        if outside(a.0, b.0, cols) || outside(a.1, b.1, rows) {
            continue;
        }
        let (mut c, mut r) = (a.0.floor() as i64, a.1.floor() as i64);
        let (end_c, end_r) = (b.0.floor() as i64, b.1.floor() as i64);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let axis = |d: f64, from: f64, cell: i64| {
            if d > 0.0 {
                (1, (cell as f64 + 1.0 - from) / d, 1.0 / d)
            } else if d < 0.0 {
                (-1, (cell as f64 - from) / d, -1.0 / d)
            } else {
                (0, f64::INFINITY, f64::INFINITY)
            }
        };
        let (step_c, mut next_c, delta_c) = axis(dx, a.0, c);
        let (step_r, mut next_r, delta_r) = axis(dy, a.1, r);
        visit(c, r);
        for _ in 0..(end_c - c).abs() + (end_r - r).abs() {
            if c != end_c && (r == end_r || next_c < next_r) {
                c += step_c;
                next_c += delta_c;
            } else {
                r += step_r;
                next_r += delta_r;
            }
            visit(c, r);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn filled(rings: &[Vec<(f64, f64)>], cols: u32, rows: u32) -> BTreeSet<(u32, u32)> {
        let mut cells = BTreeSet::new();
        fill_polygon(rings, cols, rows, |c, r| {
            assert!(cells.insert((c, r)), "{c},{r} twice");
        });
        cells
    }

    fn traced(points: &[(f64, f64)], cols: u32, rows: u32) -> Vec<(u32, u32)> {
        let mut cells = Vec::new();
        trace_line(points, cols, rows, |c, r| cells.push((c, r)));
        cells
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<(f64, f64)> {
        vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)]
    }

    fn block(cols: std::ops::Range<u32>, rows: std::ops::Range<u32>) -> BTreeSet<(u32, u32)> {
        rows.flat_map(|r| cols.clone().map(move |c| (c, r)))
            .collect()
    }

    #[test]
    fn fills_cells_whose_centres_are_inside() {
        let cells = filled(&[rect(1.2, 2.0, 4.6, 3.9)], 10, 10);
        assert_eq!(cells, block(1..5, 2..4));
        let unclosed = vec![(1.2, 2.0), (4.6, 2.0), (4.6, 3.9), (1.2, 3.9)];
        assert_eq!(filled(&[unclosed], 10, 10), cells);
    }

    #[test]
    fn inner_rings_cut_holes() {
        let cells = filled(&[rect(0.0, 0.0, 6.0, 6.0), rect(2.0, 2.0, 4.0, 4.0)], 8, 8);
        let expected: BTreeSet<_> = block(0..6, 0..6)
            .difference(&block(2..4, 2..4))
            .copied()
            .collect();
        assert_eq!(cells, expected);
    }

    #[test]
    fn clips_to_the_grid() {
        assert_eq!(
            filled(&[rect(-5.0, -5.0, 2.0, 1.0)], 4, 4),
            block(0..2, 0..1)
        );
        assert_eq!(
            filled(&[rect(-5.0, -5.0, 50.0, 50.0)], 3, 2),
            block(0..3, 0..2)
        );
        assert!(filled(&[rect(5.0, 5.0, 9.0, 9.0)], 4, 4).is_empty());
        assert!(
            filled(&[rect(1.6, 1.6, 2.4, 2.4)], 4, 4).is_empty(),
            "no centre inside"
        );
    }

    #[test]
    fn fills_triangles_by_their_edges() {
        let cells = filled(&[vec![(0.0, 0.0), (4.0, 0.0), (0.0, 4.0)]], 4, 4);
        let expected: BTreeSet<_> = block(0..4, 0..4)
            .into_iter()
            .filter(|&(c, r)| c + r < 3)
            .collect();
        assert_eq!(cells, expected);
    }

    #[test]
    fn lines_are_four_connected() {
        let cells = traced(&[(0.3, 0.2), (7.9, 3.1), (2.5, 6.5)], 10, 10);
        assert_eq!(cells[0], (0, 0));
        assert_eq!(*cells.last().unwrap(), (2, 6));
        assert!(cells.contains(&(7, 3)));
        for pair in cells.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let step = a.0.abs_diff(b.0) + a.1.abs_diff(b.1);
            assert!(step <= 1, "{a:?} → {b:?}");
        }
    }

    #[test]
    fn exact_diagonals_still_step_one_axis_at_a_time() {
        let cells = traced(&[(0.5, 0.5), (3.5, 3.5)], 4, 4);
        assert_eq!(cells.len(), 7);
        assert_eq!(*cells.last().unwrap(), (3, 3));
    }

    #[test]
    fn lines_are_clipped_to_the_grid() {
        let cells = traced(&[(-3.5, 1.5), (5.5, 1.5)], 4, 3);
        assert_eq!(cells, [(0, 1), (1, 1), (2, 1), (3, 1)]);
        assert!(traced(&[(-3.0, 1.0), (-1.0, 2.0)], 4, 3).is_empty());
        assert_eq!(traced(&[(1.5, 1.5)], 4, 3), []);
    }
}
