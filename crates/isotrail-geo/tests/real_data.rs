use std::fmt::Write as _;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use isotrail_geo::{DEFAULT_DETAIL, Footprint, Grid, MAX_ASPECT};
use isotrail_track::{GpxReader, Track, TrackReader};

fn repo_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn gpx_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("gpx"))
        })
        .collect();
    files.sort();
    files
}

/// North-up sketch of the route inside its slab and grid
fn svg(track: &Track, grid: &Grid) -> String {
    let footprint = &grid.footprint;
    let corners = footprint.corners();
    let (mut min, mut max) = (corners[0], corners[0]);
    for (x, y) in corners {
        (min.0, min.1) = (min.0.min(x), min.1.min(y));
        (max.0, max.1) = (max.0.max(x), max.1.max(y));
    }
    let scale = 1000.0 / (max.0 - min.0).max(max.1 - min.1);
    let px = |(x, y): (f64, f64)| ((x - min.0) * scale, (max.1 - y) * scale);
    let (w, h) = px((max.0, min.1));

    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\">\n\
         <rect width=\"100%\" height=\"100%\" fill=\"#f4f1e8\"/>\n"
    );
    let points = |list: &mut String, pts: &[(f64, f64)]| {
        for &p in pts {
            let (x, y) = px(p);
            let _ = write!(list, "{x:.1},{y:.1} ");
        }
    };
    let mut slab = String::new();
    points(&mut slab, &corners);
    let _ = writeln!(
        out,
        "<polygon points=\"{slab}\" fill=\"#d8e4c8\" stroke=\"#556b2f\" stroke-width=\"2\"/>"
    );
    let a = grid.cell_center_local(0, 0);
    let b = grid.cell_center_local(grid.cols - 1, 0);
    let mut first_row = String::new();
    points(&mut first_row, &[a, b]);
    let _ = writeln!(
        out,
        "<polyline points=\"{first_row}\" fill=\"none\" stroke=\"#a0522d\" stroke-dasharray=\"6 4\"/>"
    );
    for segment in &track.segments {
        let local: Vec<_> = segment
            .points
            .iter()
            .map(|p| footprint.projection.project(p.lat, p.lon))
            .collect();
        let mut line = String::new();
        points(&mut line, &local);
        let _ = writeln!(
            out,
            "<polyline points=\"{line}\" fill=\"none\" stroke=\"#d7263d\" stroke-width=\"2\"/>"
        );
    }
    let _ = writeln!(
        out,
        "<text x=\"10\" y=\"24\" font-family=\"monospace\" font-size=\"16\">N ↑  \
         {} × {} cells of {:.1} m, long side {:.1}°, yaw {:.1}°, dashed = row 0</text>\n</svg>",
        grid.cols,
        grid.rows,
        grid.cell_size_m,
        footprint.long_axis_bearing_deg(),
        footprint.camera_yaw_deg()
    );
    out
}

#[test]
#[ignore = "reads local recordings from input/"]
fn real_recordings_get_a_framed_grid() {
    let input = repo_dir().join("input");
    let files = gpx_files(&input);
    if files.is_empty() {
        eprintln!("skipped: no .gpx files in {}", input.display());
        return;
    }
    let out = repo_dir().join("out");
    fs::create_dir_all(&out).unwrap();
    let mut failures = Vec::new();

    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let mut file = File::open(path).unwrap();
        let track = GpxReader
            .read(&mut file)
            .and_then(|track| track.clean())
            .unwrap_or_else(|err| panic!("{name}: {err}"));
        let footprint = Footprint::from_track(&track).unwrap();
        let grid = Grid::new(&footprint, DEFAULT_DETAIL);

        let (half_u, half_v) = (grid.footprint.length_m / 2.0, grid.footprint.width_m / 2.0);
        let margin = track
            .points()
            .map(|p| {
                let (x, y) = footprint.projection.project(p.lat, p.lon);
                let (u, v) = grid.footprint.from_local(x, y);
                (half_u - u.abs()).min(half_v - v.abs())
            })
            .fold(f64::INFINITY, f64::min);
        let aspect = footprint.length_m / footprint.width_m;
        eprintln!(
            "{name}\n  slab {:.2} × {:.2} km (aspect {aspect:.2}), long side {:.1}°, yaw {:.1}°\n  \
             grid {} × {}, cell {:.1} m, DEM zoom {}, tightest margin {margin:.0} m",
            grid.footprint.length_m / 1000.0,
            grid.footprint.width_m / 1000.0,
            footprint.long_axis_bearing_deg(),
            footprint.camera_yaw_deg(),
            grid.cols,
            grid.rows,
            grid.cell_size_m,
            grid.dem_zoom(),
        );

        if margin < 0.0 {
            failures.push(format!("{name}: route leaves the slab by {:.0} m", -margin));
        }
        if aspect > MAX_ASPECT + 1e-9 {
            failures.push(format!("{name}: aspect {aspect:.3}"));
        }
        if !(9..=14).contains(&grid.dem_zoom()) {
            failures.push(format!("{name}: DEM zoom {}", grid.dem_zoom()));
        }

        let stem = path.file_stem().unwrap().to_string_lossy();
        fs::write(
            out.join(format!("footprint-{stem}.svg")),
            svg(&track, &grid),
        )
        .unwrap();
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
