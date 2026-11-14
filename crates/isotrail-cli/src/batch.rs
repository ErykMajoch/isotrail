use std::fmt;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Args;
use isotrail_bundle::SceneBundle;
use isotrail_export::{DEFAULT_SCALE, encode_png, upscale};
use isotrail_geo::sha256_hex;
use isotrail_render::{Conditions, Fitted, Framing, Gpu, Look, Mesher, Theme};
use serde::{Deserialize, Serialize};

use crate::BadInput;
use crate::bundle::DataArgs;
use crate::config::Typed;
use crate::frame::{AspectArg, FrameArgs, Shape, parse_scale};
use crate::look::LookArgs;
use crate::output;
use crate::overlay::OverlayArgs;
use crate::render::Source;
use crate::report::{self, Reason, Written, say, shown};
use crate::scene::SceneArgs;
use crate::stats;
use crate::theme::ThemeFileArg;

const DEFAULT_PROFILE: usize = 64;

#[derive(Debug, Args)]
pub struct BatchArgs {
    /// Folder of GPX files; the folders inside it are left out
    #[arg(value_name = "FOLDER")]
    dir: PathBuf,

    /// The folder to write each hike's scene file, images and JSON file to
    #[arg(short, long, value_name = "DIR", help_heading = crate::OUTPUT)]
    output: PathBuf,

    /// Make every hike again, even those unchanged since the last run
    #[arg(long, help_heading = crate::OUTPUT)]
    force: bool,

    /// Points in each JSON file's elevation profile, from 2 to 256
    #[arg(long, value_name = "N", default_value_t = DEFAULT_PROFILE,
          value_parser = stats::parse_profile, help_heading = crate::OUTPUT)]
    profile: usize,

    /// Print one JSON summary of the run, for scripts
    #[arg(long, help_heading = crate::OUTPUT)]
    json: bool,

    /// Image shapes, separated by commas: story, video, portrait, feed, square or wide
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        value_name = "LIST",
        default_value = "story",
        hide_possible_values = true,
        help_heading = crate::IMAGE
    )]
    aspect: Vec<AspectArg>,

    /// Output pixels per drawn pixel, from 1 to 16, dividing both sides of the image
    #[arg(long, value_name = "N", default_value_t = DEFAULT_SCALE, value_parser = parse_scale,
          help_heading = crate::IMAGE)]
    scale: u32,

    #[command(flatten)]
    frame: FrameArgs,

    #[command(flatten)]
    look: LookArgs,

    /// How many times taller than life to draw the terrain, from 0.1 to 100 [default: automatic]
    #[arg(long, value_name = "X", value_parser = crate::bundle::parse_exaggeration,
          help_heading = crate::LOOK)]
    exaggeration: Option<f64>,

    #[command(flatten)]
    theme: ThemeFileArg,

    #[command(flatten)]
    scene: SceneArgs,

    #[command(flatten)]
    overlay: OverlayArgs,

    #[command(flatten)]
    data: DataArgs,

    /// Leave the GPX file's description out of the scene files
    #[arg(long, help_heading = crate::DATA)]
    no_desc: bool,

    /// Read settings from the [batch] and [render] tables of this TOML file
    #[arg(long, value_name = "FILE", help_heading = crate::GENERAL)]
    config: Option<PathBuf>,
}

/// Some hikes failed, each already reported
#[derive(Debug)]
pub struct HikesFailed {
    failed: usize,
    total: usize,
}

impl fmt::Display for HikesFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.failed, self.total) {
            (_, 1) => f.write_str("the hike failed; see the error above"),
            (1, total) => write!(f, "1 of {total} hikes failed; see the error above"),
            (failed, total) => write!(f, "{failed} of {total} hikes failed; see the errors above"),
        }
    }
}

impl std::error::Error for HikesFailed {}

/// What a hike's JSON file records about how its files were made
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct MadeFrom {
    gpx: String,
    gpx_sha256: String,
    isotrail: String,
    scene_options: String,
    image_options: String,
    profile: usize,
}

/// The part of an earlier JSON file that says whether a hike is up to date
#[derive(Deserialize)]
struct Record {
    made_from: MadeFrom,
    files: Vec<Listed>,
}

#[derive(Deserialize)]
struct Listed {
    path: String,
}

/// What a run did with a hike
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Outcome {
    /// Everything written
    Made,
    /// The scene file kept, the images and JSON file written
    Redrawn,
    /// Only the JSON file written
    Rewritten,
    Unchanged,
    Failed,
}

/// The work an up-to-date check leaves for a hike
fn outcome(
    record: Option<&Record>,
    now: &MadeFrom,
    scene_file: &str,
    exists: impl Fn(&str) -> bool,
) -> Outcome {
    let Some(record) = record else {
        return Outcome::Made;
    };
    let then = &record.made_from;
    if then.gpx_sha256 != now.gpx_sha256
        || then.isotrail != now.isotrail
        || then.scene_options != now.scene_options
        || !exists(scene_file)
    {
        return Outcome::Made;
    }
    let listed = |name: &str| record.files.iter().any(|file| file.path == name);
    if then.image_options != now.image_options
        || !listed(scene_file)
        || !record.files.iter().all(|file| exists(&file.path))
    {
        return Outcome::Redrawn;
    }
    if then.profile != now.profile {
        return Outcome::Rewritten;
    }
    Outcome::Unchanged
}

/// For each name, another one that differs from it only by case
fn clashes(names: &[String]) -> Vec<Option<usize>> {
    let lower: Vec<String> = names.iter().map(|name| name.to_lowercase()).collect();
    (0..names.len())
        .map(|i| (0..names.len()).find(|&j| j != i && lower[j] == lower[i]))
        .collect()
}

/// A GPX file in the folder, with its name without the extension
struct Hike {
    path: PathBuf,
    file_name: String,
    stem: String,
}

/// The GPX files directly in `dir`, by name
fn hikes(dir: &Path) -> anyhow::Result<Vec<Hike>> {
    let shown = dir.display();
    match fs::metadata(dir) {
        Ok(meta) if !meta.is_dir() => {
            return Err(BadInput(format!(
                "{shown} is a file; give the folder the GPX files are in"
            ))
            .into());
        }
        Ok(_) => {}
        Err(error) => {
            let problem = match error.kind() {
                std::io::ErrorKind::NotFound => "it doesn't exist".to_owned(),
                _ => crate::io_words(&error),
            };
            return Err(BadInput(format!("can't open the folder {shown}: {problem}")).into());
        }
    }
    let entries = fs::read_dir(dir).map_err(|error| {
        BadInput(format!(
            "can't open the folder {shown}: {}",
            crate::io_words(&error)
        ))
    })?;
    let mut hikes = Vec::new();
    for entry in entries {
        let path = entry
            .with_context(|| format!("can't read the folder {shown}"))?
            .path();
        let gpx = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("gpx"));
        if !gpx || !path.is_file() {
            continue;
        }
        let (Some(file_name), Some(stem)) = (path.file_name(), path.file_stem()) else {
            continue;
        };
        let file_name = file_name.to_string_lossy().into_owned();
        let stem = stem.to_string_lossy().into_owned();
        // Messages name the file with the separator the folder was typed with
        let path = match dir.to_str() {
            Some(typed) if cfg!(windows) && typed.contains('/') => {
                PathBuf::from(format!("{}/{file_name}", typed.trim_end_matches('/')))
            }
            _ => path,
        };
        hikes.push(Hike {
            path,
            file_name,
            stem,
        });
    }
    if hikes.is_empty() {
        return Err(BadInput(format!(
            "{shown} has no GPX files in it; give the folder they are in"
        ))
        .into());
    }
    hikes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(hikes)
}

/// What every hike of the run shares
struct Run<'a> {
    args: &'a BatchArgs,
    theme: Theme,
    mesher: Box<dyn Mesher>,
    look: Look,
    conditions: Conditions,
    shapes: Vec<Shape>,
    framings: Vec<Framing>,
    scene_options: String,
    image_options: String,
    inputs: Vec<&'a Path>,
    gpu: Option<Gpu>,
}

#[derive(Serialize)]
struct HikeJson<'a> {
    #[serde(flatten)]
    stats: stats::Report<'a>,
    files: Vec<Written>,
    made_from: &'a MadeFrom,
}

#[derive(Serialize)]
struct Summary {
    hikes: Vec<HikeSummary>,
}

#[derive(Serialize)]
struct HikeSummary {
    input: String,
    result: Outcome,
    files: Vec<Written>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Failure>,
}

#[derive(Serialize)]
struct Failure {
    reason: Reason,
    message: String,
}

pub fn run(args: &BatchArgs, typed: &Typed) -> anyhow::Result<()> {
    report::set_json(args.json);
    let shapes = args.frame.listed_shapes(&args.aspect)?;
    let framings = args.frame.framings(&shapes, args.scale)?;
    args.overlay.check()?;
    crate::sun::check_dated(args.look.time, &args.data)?;
    let theme = args.theme.read()?;
    let mesher = args.look.mesher(typed, args.scene.terrain(&theme));
    let look = args.scene.apply(args.look.look(&theme)?);
    output::folder(&args.output)?;
    let hikes = hikes(&args.dir)?;

    let scene_options = sha256_hex(&[format!(
        "{:?}",
        (
            args.data.shaping(),
            args.exaggeration,
            args.overlay.given_title(),
            args.no_desc
        )
    )
    .as_bytes()]);
    let read = |path: Option<&Path>| {
        path.and_then(|path| fs::read(path).ok())
            .unwrap_or_default()
    };
    let image_options = sha256_hex(&[
        scene_options.as_bytes(),
        format!(
            "{:?}",
            (
                &args.aspect,
                args.scale,
                &args.frame,
                &args.look,
                args.theme.path(),
                &args.scene,
                &args.overlay
            )
        )
        .as_bytes(),
        &read(args.look.palette_file()),
        &read(args.theme.path()),
    ]);
    let inputs: Vec<&Path> = args
        .config
        .as_deref()
        .into_iter()
        .chain(args.look.palette_file().filter(|path| path.is_file()))
        .chain(args.theme.path().filter(|path| path.is_file()))
        .collect();
    let mut run = Run {
        args,
        conditions: args.scene.conditions(&theme),
        theme,
        mesher,
        look,
        shapes,
        framings,
        scene_options,
        image_options,
        inputs,
        gpu: None,
    };

    let names: Vec<String> = hikes.iter().map(|hike| hike.stem.clone()).collect();
    let clashes = clashes(&names);
    let total = hikes.len();
    let mut summaries = Vec::new();
    for (i, (hike, clash)) in hikes.iter().zip(clashes).enumerate() {
        let place = format!("{} ({} of {total})", hike.file_name, i + 1);
        let result = match clash {
            Some(other) => Err(anyhow::anyhow!(
                "{} and {} would write the same files on Windows and macOS; rename one",
                hike.file_name,
                hikes[other].file_name
            )),
            None => make(&mut run, hike, &place),
        };
        let summary = match result {
            Ok((result, files)) => HikeSummary {
                input: shown(&hike.path),
                result,
                files,
                error: None,
            },
            Err(error) => {
                let reason = crate::reason(&error);
                if reason == Reason::Interrupted {
                    return Err(error);
                }
                if clash.is_some() {
                    say!("{place}");
                }
                let mut message = crate::describe(&error);
                if !message.contains(&hike.file_name) {
                    message = format!("{}: {message}", hike.file_name);
                }
                crate::print_error(&message);
                HikeSummary {
                    input: shown(&hike.path),
                    result: Outcome::Failed,
                    files: Vec::new(),
                    error: Some(Failure { reason, message }),
                }
            }
        };
        summaries.push(summary);
    }

    let count = |outcome: Outcome| summaries.iter().filter(|s| s.result == outcome).count();
    let failed = count(Outcome::Failed);
    if args.json {
        let mut out = std::io::stdout().lock();
        serde_json::to_writer_pretty(&mut out, &Summary { hikes: summaries })?;
        writeln!(out)?;
    } else if count(Outcome::Unchanged) == total {
        match total {
            1 => say!("Nothing to do: the hike is unchanged"),
            _ => say!("Nothing to do: all {total} hikes are unchanged"),
        }
    } else {
        let parts: Vec<String> = [
            (Outcome::Made, "made"),
            (Outcome::Redrawn, "redrawn"),
            (Outcome::Rewritten, "with a new JSON file"),
            (Outcome::Unchanged, "unchanged"),
            (Outcome::Failed, "failed"),
        ]
        .into_iter()
        .map(|(outcome, words)| (count(outcome), words))
        .filter(|&(n, _)| n > 0)
        .map(|(n, words)| format!("{n} {words}"))
        .collect();
        say!("Hikes: {}", parts.join(", "));
    }
    if failed > 0 {
        return Err(HikesFailed { failed, total }.into());
    }
    Ok(())
}

/// Brings one hike's files up to date, returning what was done and the
/// files written
fn make(run: &mut Run, hike: &Hike, place: &str) -> anyhow::Result<(Outcome, Vec<Written>)> {
    let args = run.args;
    let out = &args.output;
    let gpx = crate::read_file(&hike.path)?;
    let made_from = MadeFrom {
        gpx: hike.file_name.clone(),
        gpx_sha256: sha256_hex(&[&gpx]),
        isotrail: env!("CARGO_PKG_VERSION").to_owned(),
        scene_options: run.scene_options.clone(),
        image_options: run.image_options.clone(),
        profile: args.profile,
    };
    let scene_name = format!("{}.itb", hike.stem);
    let scene_path = out.join(&scene_name);
    let json_path = out.join(format!("{}.json", hike.stem));
    let (images, _) = crate::render::output_paths(
        &hike.path,
        Some(&out.join(format!("{}.png", hike.stem))),
        &run.shapes,
    )?;
    let record = fs::read(&json_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Record>(&bytes).ok());
    let outcome = if args.force {
        Outcome::Made
    } else {
        outcome(record.as_ref(), &made_from, &scene_name, |name| {
            out.join(name).is_file()
        })
    };
    match outcome {
        Outcome::Unchanged => {
            say!("{place}: unchanged");
            return Ok((outcome, Vec::new()));
        }
        Outcome::Redrawn => say!("{place}: drawing the images again"),
        Outcome::Rewritten => say!("{place}: writing the JSON file again"),
        _ => say!("{place}"),
    }
    let inputs: Vec<&Path> = [hike.path.as_path()]
        .into_iter()
        .chain(run.inputs.iter().copied())
        .collect();
    output::plan(
        [&scene_path, &json_path]
            .into_iter()
            .chain(&images)
            .map(PathBuf::as_path),
        &inputs,
    )?;

    let mut written = Vec::new();
    let (bundle, scene_bytes) = if outcome == Outcome::Made {
        report::next_area();
        let source = Source::Track(crate::parse_track(&gpx, &hike.path)?);
        crate::sun::check_time(args.look.time, &source, &args.data)?;
        let mut bundle = source.into_bundle(&hike.path, &args.data)?;
        if let Some(exaggeration) = args.exaggeration {
            bundle.grid.exaggeration = exaggeration;
        }
        if let Some(title) = args.overlay.given_title() {
            title.clone_into(&mut bundle.meta.title);
        }
        if args.no_desc {
            bundle.meta.desc = None;
        }
        let bytes = bundle.to_bytes()?;
        output::write(&scene_path, |file| Ok(file.write_all(&bytes)?))?;
        say!(
            "Wrote {} ({}KB)",
            shown(&scene_path),
            bytes.len().div_ceil(1024)
        );
        written.push(Written::file(&scene_path));
        (bundle, bytes)
    } else {
        let bytes = crate::read_file(&scene_path)?;
        let bundle = SceneBundle::from_bytes(&bytes)
            .with_context(|| format!("can't read {}", scene_path.display()))?;
        (bundle, bytes)
    };

    let mut files = vec![Written::file(&scene_path).named()];
    if outcome == Outcome::Rewritten {
        for (shape, image) in run.shapes.iter().zip(&images) {
            files.push(Written::image(image, shape.size()).named());
        }
    } else {
        for image in draw(run, &bundle, &images)? {
            files.push(image.clone().named());
            written.push(image);
        }
    }

    let summary = stats::Summary::of_bundle(bundle, &scene_bytes, true);
    let json = HikeJson {
        stats: stats::Report::new(&summary, Some(args.profile)),
        files,
        made_from: &made_from,
    };
    let mut text = serde_json::to_vec_pretty(&json)?;
    text.push(b'\n');
    output::write(&json_path, |file| Ok(file.write_all(&text)?))?;
    say!("Wrote {}", shown(&json_path));
    written.push(Written::file(&json_path));
    Ok((outcome, written))
}

/// Draws the hike's still in each shape into `images`
fn draw(run: &mut Run, bundle: &SceneBundle, images: &[PathBuf]) -> anyhow::Result<Vec<Written>> {
    let args = run.args;
    let labels = &run.theme.labels;
    let text = args.overlay.text(bundle, labels)?;
    let built_in = args.overlay.built_in_text(bundle, labels);
    let themed = built_in.as_ref().zip(args.theme.path());
    let style = args.overlay.style(&run.theme, run.look.route.colour);
    let fitted = run
        .shapes
        .iter()
        .zip(&run.framings)
        .map(|(shape, framing)| {
            args.overlay
                .fit(&text, themed, &shape.label(), framing, args.scale, &style)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let sun = crate::sun::hike_sun(bundle, args.look.time, &run.conditions)?;
    let aim = args.frame.aim();
    let gpu = match &mut run.gpu {
        Some(gpu) => gpu,
        empty => empty.insert(Gpu::headless()?),
    };
    let mut written = Vec::new();
    for (fit, image) in fitted.iter().zip(images) {
        let frame = crate::render::still(
            gpu,
            bundle,
            run.mesher.as_ref(),
            &fit.overlays,
            &aim,
            &sun,
            &run.look,
        )?;
        let frame = upscale(&frame, args.scale)?;
        output::write(image, |file| Ok(encode_png(&frame, file)?))?;
        say!("Wrote {}", shown(image));
        written.push(Written::image(image, (frame.width(), frame.height())));
    }
    let dropped = fitted.iter().any(Fitted::credit_dropped);
    if let Some(credit) = args.overlay.caption_credit(bundle, dropped) {
        say!("Credit for the caption: {credit}");
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn made_from() -> MadeFrom {
        MadeFrom {
            gpx: "walk.gpx".into(),
            gpx_sha256: "a".into(),
            isotrail: "1.0.0".into(),
            scene_options: "s".into(),
            image_options: "i".into(),
            profile: 64,
        }
    }

    fn record(made_from: MadeFrom) -> Record {
        Record {
            made_from,
            files: ["walk.itb", "walk.png"]
                .map(|path| Listed { path: path.into() })
                .into(),
        }
    }

    #[test]
    fn a_hike_is_redone_only_as_far_as_its_changes_reach() {
        let now = made_from();
        let all = |_: &str| true;
        let check = |then: MadeFrom, exists: &dyn Fn(&str) -> bool| {
            outcome(Some(&record(then)), &now, "walk.itb", exists)
        };
        assert_eq!(outcome(None, &now, "walk.itb", all), Outcome::Made);
        assert_eq!(check(made_from(), &all), Outcome::Unchanged);
        for change in [
            MadeFrom {
                gpx_sha256: "b".into(),
                ..made_from()
            },
            MadeFrom {
                isotrail: "0.9.0".into(),
                ..made_from()
            },
            MadeFrom {
                scene_options: "t".into(),
                ..made_from()
            },
        ] {
            assert_eq!(check(change, &all), Outcome::Made);
        }
        let image = MadeFrom {
            image_options: "j".into(),
            ..made_from()
        };
        assert_eq!(check(image, &all), Outcome::Redrawn);
        let profile = MadeFrom {
            profile: 32,
            ..made_from()
        };
        assert_eq!(check(profile, &all), Outcome::Rewritten);
        assert_eq!(
            check(made_from(), &|name| name != "walk.png"),
            Outcome::Redrawn
        );
        assert_eq!(
            check(made_from(), &|name| name != "walk.itb"),
            Outcome::Made
        );
    }

    #[test]
    fn names_that_differ_only_by_case_clash() {
        let names: Vec<String> = ["Ridge", "lakes", "ridge", "Moor"].map(String::from).into();
        assert_eq!(clashes(&names), [Some(2), None, Some(0), None]);
    }

    #[test]
    fn the_closing_error_counts_the_failures() {
        let text = |failed, total| HikesFailed { failed, total }.to_string();
        assert_eq!(text(1, 1), "the hike failed; see the error above");
        assert_eq!(text(1, 3), "1 of 3 hikes failed; see the error above");
        assert_eq!(text(2, 3), "2 of 3 hikes failed; see the errors above");
    }
}
