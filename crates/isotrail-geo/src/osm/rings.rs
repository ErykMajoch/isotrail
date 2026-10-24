use super::{Polygon, Ring};

type Point = (f64, f64);

/// Builds polygons from the way geometries of a multipolygon relation and
/// returns how many pieces or rings had to be dropped
pub(super) fn assemble(outer: Vec<Vec<Point>>, inner: Vec<Vec<Point>>) -> (Vec<Polygon>, usize) {
    let (outers, mut dropped) = join(outer);
    let (inners, dropped_inner) = join(inner);
    dropped += dropped_inner;

    let areas: Vec<f64> = outers.iter().map(|ring| area(ring)).collect();
    let mut polygons: Vec<Polygon> = outers
        .into_iter()
        .map(|outer| Polygon {
            outer,
            inner: Vec::new(),
        })
        .collect();
    for hole in inners {
        let owner = (0..polygons.len())
            .filter(|&i| mostly_inside(&hole, &polygons[i].outer))
            .min_by(|&a, &b| areas[a].total_cmp(&areas[b]));
        match owner {
            Some(i) => polygons[i].inner.push(hole),
            None => dropped += 1,
        }
    }
    (polygons, dropped)
}

/// Joins open ways end to end, flipping them as needed, until each chain
/// closes. Shared nodes have identical coordinates, so exact equality
/// works. Returns the closed rings and the number of chains that never closed
fn join(pieces: Vec<Vec<Point>>) -> (Vec<Ring>, usize) {
    let mut pending: Vec<Vec<Point>> = pieces.into_iter().filter(|p| p.len() >= 2).collect();
    let (mut rings, mut dropped) = (Vec::new(), 0);
    while !pending.is_empty() {
        let mut chain = pending.remove(0);
        loop {
            let end = chain[chain.len() - 1];
            if chain[0] == end {
                if chain.len() >= 4 {
                    rings.push(chain);
                } else {
                    dropped += 1;
                }
                break;
            }
            let next = pending
                .iter()
                .position(|p| p[0] == end || p[p.len() - 1] == end);
            let Some(next) = next else {
                dropped += 1;
                break;
            };
            let mut next = pending.remove(next);
            if next[0] != end {
                next.reverse();
            }
            chain.extend_from_slice(&next[1..]);
        }
    }
    (rings, dropped)
}

/// More than half of the hole's vertices inside, so a hole that touches its
/// outer ring at a node still counts
fn mostly_inside(hole: &[Point], outer: &[Point]) -> bool {
    let vertices = &hole[..hole.len() - 1];
    let inside = vertices.iter().filter(|&&p| contains(outer, p)).count();
    inside * 2 > vertices.len()
}

/// Even-odd ray casting on a closed ring
fn contains(ring: &[Point], (a, b): Point) -> bool {
    let mut inside = false;
    for edge in ring.windows(2) {
        let ((a0, b0), (a1, b1)) = (edge[0], edge[1]);
        if (b0 > b) != (b1 > b) && a < a0 + (b - b0) / (b1 - b0) * (a1 - a0) {
            inside = !inside;
        }
    }
    inside
}

fn area(ring: &[Point]) -> f64 {
    ring.windows(2)
        .map(|e| e[0].0 * e[1].1 - e[1].0 * e[0].1)
        .sum::<f64>()
        .abs()
        / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, size: f64) -> Vec<Point> {
        vec![
            (x0, y0),
            (x0 + size, y0),
            (x0 + size, y0 + size),
            (x0, y0 + size),
            (x0, y0),
        ]
    }

    fn same_ring(a: &[Point], b: &[Point]) -> bool {
        let mut a: Vec<_> = a[..a.len() - 1].to_vec();
        let mut b: Vec<_> = b[..b.len() - 1].to_vec();
        a.sort_by(|p, q| p.partial_cmp(q).unwrap());
        b.sort_by(|p, q| p.partial_cmp(q).unwrap());
        a == b
    }

    #[test]
    fn joins_split_and_reversed_ways() {
        let pieces = vec![
            vec![(0.0, 0.0), (1.0, 0.0)],
            vec![(0.0, 1.0), (1.0, 1.0)],
            vec![(1.0, 1.0), (1.0, 0.0)],
            vec![(0.0, 0.0), (0.0, 1.0)],
        ];
        let (rings, dropped) = join(pieces);
        assert_eq!(dropped, 0);
        assert_eq!(rings.len(), 1);
        let ring = &rings[0];
        assert_eq!(ring.len(), 5);
        assert_eq!(ring[0], ring[4]);
        assert!(same_ring(ring, &square(0.0, 0.0, 1.0)));
        assert!(ring.windows(2).all(|e| e[0] != e[1]));
    }

    #[test]
    fn closed_ways_pass_through_and_open_chains_are_dropped() {
        let pieces = vec![
            square(0.0, 0.0, 1.0),
            vec![(5.0, 5.0), (6.0, 5.0), (6.0, 6.0)],
            vec![(9.0, 9.0)],
            vec![(7.0, 7.0), (8.0, 7.0), (7.0, 7.0)],
        ];
        let (rings, dropped) = join(pieces);
        assert_eq!(rings, vec![square(0.0, 0.0, 1.0)]);
        assert_eq!(dropped, 2);
    }

    #[test]
    fn holes_go_to_the_smallest_outer_that_holds_them() {
        let lake = square(0.0, 0.0, 10.0);
        let island = square(3.0, 3.0, 4.0);
        let outers = vec![lake.clone(), island.clone()];
        let inners = vec![square(1.0, 1.0, 8.0), square(4.0, 4.0, 1.0)];
        let (polygons, dropped) = assemble(outers, inners);
        assert_eq!(dropped, 0);
        assert_eq!(polygons[0].outer, lake);
        assert_eq!(polygons[0].inner, vec![square(1.0, 1.0, 8.0)]);
        assert_eq!(polygons[1].outer, island);
        assert_eq!(polygons[1].inner, vec![square(4.0, 4.0, 1.0)]);
    }

    #[test]
    fn holes_touching_the_outer_ring_and_orphans() {
        let outer = square(0.0, 0.0, 4.0);
        let touching = vec![(0.0, 0.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0), (0.0, 0.0)];
        let orphan = square(10.0, 10.0, 1.0);
        let (polygons, dropped) = assemble(vec![outer], vec![touching.clone(), orphan]);
        assert_eq!(polygons.len(), 1);
        assert_eq!(polygons[0].inner, vec![touching]);
        assert_eq!(dropped, 1);
    }

    #[test]
    fn point_in_polygon_and_area() {
        let l_shape = vec![
            (0.0, 0.0),
            (2.0, 0.0),
            (2.0, 1.0),
            (1.0, 1.0),
            (1.0, 2.0),
            (0.0, 2.0),
            (0.0, 0.0),
        ];
        assert!(contains(&l_shape, (0.5, 1.5)));
        assert!(contains(&l_shape, (1.5, 0.5)));
        assert!(!contains(&l_shape, (1.5, 1.5)));
        assert!(!contains(&l_shape, (-0.5, 0.5)));
        assert_eq!(area(&l_shape), 3.0);
        let mut reversed = l_shape.clone();
        reversed.reverse();
        assert_eq!(area(&reversed), 3.0);
    }
}
