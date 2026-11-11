//! The sea, from OpenStreetMap's coastlines, which keep the land on their
//! left; points are in fractional cell space, where rows run south, so the
//! map's left is the side where the cross product is negative

use std::collections::{BTreeMap, HashMap};

use super::raster::trace_line;
use crate::Heightmap;

type Point = (f64, f64);

/// Which cells are sea. With no coastline in the map data, the slab is sea
/// only when none of it is above sea level and nothing on it is a land
/// feature, so a polder or a shore below sea level stays land
pub(super) fn sea_cells(
    coasts: &[Vec<Point>],
    cols: u32,
    rows: u32,
    land_features: bool,
    heights_m: &[f32],
) -> Vec<bool> {
    let cells = (cols * rows) as usize;
    let coasts = join(coasts);
    if coasts.is_empty() {
        let sea = !land_features && heights_m.iter().all(|&h| h <= super::SEA_LEVEL_M);
        return vec![sea; cells];
    }

    let mut shore: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
    for (way, coast) in coasts.iter().enumerate() {
        for seg in 0..coast.len() - 1 {
            trace_line(&coast[seg..=seg + 1], cols, rows, |col, row| {
                let crossing = shore.entry((row * cols + col) as usize).or_default();
                if !crossing.contains(&(way, seg)) {
                    crossing.push((way, seg));
                }
            });
        }
    }
    let centre = |cell: usize| {
        (
            (cell % cols as usize) as f64 + 0.5,
            (cell / cols as usize) as f64 + 0.5,
        )
    };
    if shore.is_empty() {
        let middle = (f64::from(cols) / 2.0, f64::from(rows) / 2.0);
        return vec![!is_land_nearest(middle, &coasts, all_segments(&coasts)); cells];
    }

    let neighbours = |cell: usize| {
        let (col, row, cols, rows) = (
            cell % cols as usize,
            cell / cols as usize,
            cols as usize,
            rows as usize,
        );
        [
            (col > 0).then(|| cell - 1),
            (col + 1 < cols).then(|| cell + 1),
            (row > 0).then(|| cell - cols),
            (row + 1 < rows).then(|| cell + cols),
        ]
        .into_iter()
        .flatten()
    };

    const UNSEEN: u32 = u32::MAX;
    let mut component = vec![UNSEEN; cells];
    let mut firsts = Vec::new();
    for start in 0..cells {
        if component[start] != UNSEEN || shore.contains_key(&start) {
            continue;
        }
        let id = firsts.len() as u32;
        firsts.push(start);
        component[start] = id;
        let mut todo = vec![start];
        while let Some(cell) = todo.pop() {
            for n in neighbours(cell) {
                if component[n] == UNSEEN && !shore.contains_key(&n) {
                    component[n] = id;
                    todo.push(n);
                }
            }
        }
    }

    // Land and sea votes from the cells along the coast
    let mut votes = vec![(0u32, 0u32); firsts.len()];
    for (&cell, crossing) in &shore {
        for n in neighbours(cell) {
            if shore.contains_key(&n) {
                continue;
            }
            let vote = &mut votes[component[n] as usize];
            if is_land_nearest(centre(n), &coasts, crossing.iter().copied()) {
                vote.0 += 1;
            } else {
                vote.1 += 1;
            }
        }
    }
    let sea_components: Vec<bool> = votes
        .iter()
        .zip(&firsts)
        .map(|(&(land, sea), &first)| match land.cmp(&sea) {
            std::cmp::Ordering::Less => true,
            std::cmp::Ordering::Greater => false,
            std::cmp::Ordering::Equal => {
                !is_land_nearest(centre(first), &coasts, all_segments(&coasts))
            }
        })
        .collect();

    (0..cells)
        .map(|cell| match shore.get(&cell) {
            Some(crossing) => !is_land_nearest(centre(cell), &coasts, crossing.iter().copied()),
            None => sea_components[component[cell] as usize],
        })
        .collect()
}

/// Raises land beside the sea to sea level: resampling blends the sea bed
/// into the first cell of land, which would leave a pit beside the water
pub(super) fn raise_shore(sea: &[bool], heightmap: &mut Heightmap) {
    let cols = heightmap.cols as usize;
    for cell in 0..sea.len() {
        let (col, row) = (cell % cols, cell / cols);
        let beside_sea = (col > 0 && sea[cell - 1])
            || (col + 1 < cols && sea[cell + 1])
            || (row > 0 && sea[cell - cols])
            || sea.get(cell + cols) == Some(&true);
        if !sea[cell] && beside_sea {
            let height = &mut heightmap.heights_m[cell];
            *height = height.max(super::SEA_LEVEL_M);
        }
    }
}

/// Joins ways that continue one another, so turns where two ways meet are
/// judged like any other corner; repeated points are dropped
fn join(coasts: &[Vec<Point>]) -> Vec<Vec<Point>> {
    let key = |p: Point| (p.0.to_bits(), p.1.to_bits());
    let ways: Vec<Vec<Point>> = coasts
        .iter()
        .map(|way| {
            let mut way = way.clone();
            way.dedup();
            way
        })
        .filter(|way| way.len() >= 2)
        .collect();
    let open = |way: &Vec<Point>| way[0] != way[way.len() - 1];
    let starts: HashMap<_, usize> = ways
        .iter()
        .enumerate()
        .filter(|(_, way)| open(way))
        .map(|(i, way)| (key(way[0]), i))
        .collect();
    let continued: Vec<bool> = ways
        .iter()
        .map(|way| open(way) && ways.iter().any(|w| open(w) && w[w.len() - 1] == way[0]))
        .collect();
    let mut taken = vec![false; ways.len()];
    let mut joined = Vec::new();
    // Heads of chains first, then whatever is left: rings and loops of ways
    for i in (0..ways.len())
        .filter(|&i| !continued[i])
        .chain(0..ways.len())
    {
        if taken[i] {
            continue;
        }
        taken[i] = true;
        let mut chain = ways[i].clone();
        while open(&chain)
            && let Some(&next) = starts.get(&key(chain[chain.len() - 1]))
            && !taken[next]
        {
            taken[next] = true;
            chain.extend_from_slice(&ways[next][1..]);
        }
        joined.push(chain);
    }
    joined
}

fn all_segments(coasts: &[Vec<Point>]) -> impl Iterator<Item = (usize, usize)> + '_ {
    coasts
        .iter()
        .enumerate()
        .flat_map(|(way, coast)| (0..coast.len() - 1).map(move |seg| (way, seg)))
}

/// The side of `p` that the nearest of `segments` puts it on
fn is_land_nearest(
    p: Point,
    coasts: &[Vec<Point>],
    segments: impl Iterator<Item = (usize, usize)>,
) -> bool {
    let nearest = segments.min_by(|&(w1, s1), &(w2, s2)| {
        let d1 = distance_sq(p, coasts[w1][s1], coasts[w1][s1 + 1]);
        let d2 = distance_sq(p, coasts[w2][s2], coasts[w2][s2 + 1]);
        d1.total_cmp(&d2)
    });
    nearest.is_some_and(|(way, seg)| is_land(p, &coasts[way], seg))
}

fn cross(a: Point, b: Point) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

fn sub(a: Point, b: Point) -> Point {
    (a.0 - b.0, a.1 - b.1)
}

/// Where along a → b the point nearest `p` lies, from 0 to 1
fn along(p: Point, a: Point, b: Point) -> f64 {
    let (d, ap) = (sub(b, a), sub(p, a));
    ((ap.0 * d.0 + ap.1 * d.1) / (d.0 * d.0 + d.1 * d.1)).clamp(0.0, 1.0)
}

fn distance_sq(p: Point, a: Point, b: Point) -> f64 {
    let t = along(p, a, b);
    let (x, y) = (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1));
    (p.0 - x).powi(2) + (p.1 - y).powi(2)
}

fn left_of(p: Point, a: Point, b: Point) -> bool {
    cross(sub(b, a), sub(p, a)) < 0.0
}

/// Whether `p` is on the land side of segment `seg`; where its nearest
/// point is a corner, both segments at that corner decide
fn is_land(p: Point, coast: &[Point], seg: usize) -> bool {
    let last = coast.len() - 2;
    let ring = coast.len() > 3 && coast[0] == coast[coast.len() - 1];
    let t = along(p, coast[seg], coast[seg + 1]);
    let corner = if t <= 0.0 {
        match seg {
            0 if ring => Some((last, 0)),
            0 => None,
            _ => Some((seg - 1, seg)),
        }
    } else if t >= 1.0 {
        match seg {
            _ if seg == last && ring => Some((last, 0)),
            _ if seg == last => None,
            _ => Some((seg, seg + 1)),
        }
    } else {
        None
    };
    let Some((before, after)) = corner else {
        return left_of(p, coast[seg], coast[seg + 1]);
    };
    let (a, v, b) = (coast[before], coast[before + 1], coast[after + 1]);
    let (on_before, on_after) = (left_of(p, a, v), left_of(p, v, b));
    // A turn to the map's left wraps round the land, a turn to its right
    // round the sea
    if cross(sub(v, a), sub(b, v)) < 0.0 {
        on_before && on_after
    } else {
        on_before || on_after
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sea(coasts: &[Vec<Point>], cols: u32, rows: u32) -> Vec<bool> {
        sea_cells(
            coasts,
            cols,
            rows,
            true,
            &vec![10.0; (cols * rows) as usize],
        )
    }

    fn picture(sea: &[bool], cols: usize) -> Vec<String> {
        sea.chunks(cols)
            .map(|row| row.iter().map(|&s| if s { '~' } else { '#' }).collect())
            .collect()
    }

    #[test]
    fn land_lies_left_of_the_coast_on_the_map() {
        // Rows run south, so a coast heading east has the land above it
        let east = vec![(-1.0, 2.0), (7.0, 2.0)];
        assert_eq!(
            picture(&sea(std::slice::from_ref(&east), 6, 4), 6),
            ["######", "######", "~~~~~~", "~~~~~~"]
        );
        let west: Vec<Point> = east.into_iter().rev().collect();
        assert_eq!(
            picture(&sea(&[west], 6, 4), 6),
            ["~~~~~~", "~~~~~~", "######", "######"]
        );
    }

    #[test]
    fn a_coast_can_cut_off_any_corner() {
        // Heading north then east on the map: the sea is to the south-east
        let corner = vec![(3.4, 7.0), (3.4, 3.6), (7.0, 3.6)];
        assert_eq!(
            picture(&sea(std::slice::from_ref(&corner), 6, 6), 6),
            ["######", "######", "######", "######", "###~~~", "###~~~"]
        );
        let flipped: Vec<Point> = corner.iter().map(|&(c, r)| (6.0 - c, r)).rev().collect();
        assert_eq!(
            picture(&sea(&[flipped], 6, 6), 6),
            ["######", "######", "######", "######", "~~~###", "~~~###"]
        );
        let up: Vec<Point> = corner.iter().map(|&(c, r)| (6.0 - c, 6.0 - r)).collect();
        assert_eq!(
            picture(&sea(&[up], 6, 6), 6),
            ["~~~###", "~~~###", "######", "######", "######", "######"]
        );
    }

    #[test]
    fn coasts_split_into_ways_join_up() {
        let whole = vec![(3.4, 7.0), (3.4, 3.6), (7.0, 3.6)];
        let split = [vec![(3.4, 3.6), (7.0, 3.6)], vec![(3.4, 7.0), (3.4, 3.6)]];
        assert_eq!(join(&split), std::slice::from_ref(&whole));
        assert_eq!(sea(&split, 6, 6), sea(&[whole], 6, 6));
    }

    #[test]
    fn an_island_is_land_in_the_sea() {
        // Anticlockwise on the map is clockwise in rows running south
        let island = vec![(2.0, 2.0), (2.0, 5.0), (6.0, 5.0), (6.0, 2.0), (2.0, 2.0)];
        assert_eq!(
            picture(&sea(std::slice::from_ref(&island), 8, 7), 8),
            [
                "~~~~~~~~", "~~~~~~~~", "~~####~~", "~~####~~", "~~####~~", "~~~~~~~~", "~~~~~~~~"
            ]
        );
        let lagoon: Vec<Point> = island.into_iter().rev().collect();
        assert_eq!(
            picture(&sea(&[lagoon], 8, 7), 8),
            [
                "########", "########", "##~~~~##", "##~~~~##", "##~~~~##", "########", "########"
            ]
        );
    }

    #[test]
    fn a_narrow_headland_stays_land_at_its_tip() {
        // Out along row 2.2 and back along row 2.8 on the map's left
        let headland = vec![(-1.0, 2.8), (4.6, 2.8), (4.6, 2.2), (-1.0, 2.2)];
        let cells = sea(std::slice::from_ref(&headland), 7, 5);
        assert_eq!(
            picture(&cells, 7),
            ["~~~~~~~", "~~~~~~~", "#####~~", "~~~~~~~", "~~~~~~~"]
        );
        assert!(is_land((4.5, 2.5), &headland, 1));
        assert!(!is_land((5.0, 2.5), &headland, 1));
        assert!(!is_land((5.0, 1.9), &headland, 2));
    }

    #[test]
    fn a_coast_beyond_the_slab_decides_the_whole_slab() {
        let offshore = vec![(-10.0, -2.0), (20.0, -2.0)];
        assert!(
            sea(std::slice::from_ref(&offshore), 4, 3)
                .iter()
                .all(|&s| s)
        );
        let inland: Vec<Point> = offshore.into_iter().rev().collect();
        assert!(sea(&[inland], 4, 3).iter().all(|&s| !s));
    }

    #[test]
    fn without_a_coast_only_bare_ground_below_sea_level_is_sea() {
        let deep = [-3400.0, -3500.0, -3429.0, 0.0];
        assert_eq!(sea_cells(&[], 2, 2, false, &deep), [true; 4]);
        let polder = [-4.0, -5.0, -3.0, -4.5];
        assert_eq!(sea_cells(&[], 2, 2, true, &polder), [false; 4]);
        let shore = [-40.0, -30.0, 2.0, -10.0];
        assert_eq!(sea_cells(&[], 2, 2, false, &shore), [false; 4]);
    }

    #[test]
    fn land_beside_the_sea_is_raised_to_sea_level() {
        let mut heightmap = Heightmap {
            cols: 3,
            rows: 2,
            heights_m: vec![0.0, -6.0, -8.0, 0.0, 4.0, -9.0],
        };
        raise_shore(&[true, false, false, true, false, false], &mut heightmap);
        assert_eq!(heightmap.heights_m, [0.0, 0.0, -8.0, 0.0, 4.0, -9.0]);
    }
}
