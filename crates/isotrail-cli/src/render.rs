use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Args;
use isotrail_bundle::SceneBundle;
use isotrail_export::{
    DEFAULT_SCALE, ExportError, Ffmpeg, GifWriter, Mp4Writer, check_gif_look, encode_png,
    render_clip, render_still, upscale,
};
use isotrail_render::{
    Aim, Aspect, Background, Fitted, Frame, Gpu, Look, Mesher, Motion, Overlays, Pace, SunState,
    Walk,
};
use isotrail_track::Track;
use tracing::{info, warn};

use crate::animation::AnimationArgs;
use crate::bundle::DataArgs;
use crate::config::Typed;
use crate::frame::{AspectArg, FrameArgs, Shape, parse_scale};
use crate::look::LookArgs;
use crate::output::{self, Pending};
use crate::overlay::OverlayArgs;
use crate::scene::SceneArgs;
use crate::usage;

#[derive(Debug, Args)]
pub struct RenderArgs {
    /// GPX file or scene bundle (.itb) to render
    file: PathBuf,

    /// Where to write the image (.png) or animation (.gif or .mp4); defaults
    /// to a PNG named after the input in the current directory. With several
    /// aspects, each name gets the aspect added: hike-story.png, hike-feed.png
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Image shapes to render: story or video (1080 × 1920), portrait
    /// (1080 × 1440), feed (1080 × 1350), square (1080 × 1080) or wide
    /// (1920 × 1080)
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        value_name = "ASPECTS",
        default_values_t = [AspectArg::Story]
    )]
    aspect: Vec<AspectArg>,

    /// Output pixels per drawn pixel, from 1 to 16; it must divide both
    /// sides of the image
    #[arg(long, value_name = "N", default_value_t = DEFAULT_SCALE, value_parser = parse_scale)]
    scale: u32,

    /// Read settings from the [render] table of this TOML file; flags given
    /// here take precedence over it, and it over the user config file
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// How many times taller than life to draw the terrain; chosen from the
    /// slab's size and relief by default
    #[arg(long, value_name = "X", value_parser = crate::bundle::parse_exaggeration)]
    exaggeration: Option<f64>,

    #[command(flatten)]
    frame: FrameArgs,

    #[command(flatten)]
    look: LookArgs,

    #[command(flatten)]
    scene: SceneArgs,

    #[command(flatten)]
    overlay: OverlayArgs,

    #[command(flatten)]
    animation: AnimationArgs,

    #[command(flatten)]
    data: DataArgs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Png,
    Gif,
    Mp4,
}

impl RenderArgs {
    fn shapes(&self) -> anyhow::Result<Vec<Shape>> {
        let aspects: Vec<Aspect> = self.aspect.iter().map(|&a| a.into()).collect();
        for (i, aspect) in aspects.iter().enumerate() {
            if aspects[..i].contains(aspect) {
                return Err(usage(format!("--aspect lists {} twice", aspect.name())));
            }
        }
        Ok(self.frame.shapes(&aspects))
    }
}

pub fn run(args: &RenderArgs, typed: &Typed) -> anyhow::Result<()> {
    let shapes = args.shapes()?;
    let framings = args.frame.framings(&shapes, args.scale)?;
    let (outputs, format) = output_paths(&args.file, args.output.as_deref(), &shapes)?;
    let inputs: Vec<&Path> = [Some(args.file.as_path()), args.config.as_deref()]
        .into_iter()
        .flatten()
        .chain(args.look.palette_file().filter(|path| path.is_file()))
        .collect();
    output::plan(outputs.iter().map(PathBuf::as_path), &inputs)?;
    let mut motion = match format {
        Format::Png => None,
        Format::Gif | Format::Mp4 => Some(args.animation.motion()?),
    };
    args.overlay.check()?;
    let mesher = args.look.mesher(typed, args.scene.terrain());
    let look = args.scene.apply(args.look.look()?);
    if format == Format::Gif {
        check_gif_look(&look).map_err(|error| usage(error.to_string()))?;
    }
    let ffmpeg = if format == Format::Mp4 {
        if look.background == Background::Transparent {
            return Err(usage(ExportError::Mp4Transparent.to_string()));
        }
        for framing in &framings {
            let (width, height) = (framing.width * args.scale, framing.height * args.scale);
            if width % 2 != 0 || height % 2 != 0 {
                return Err(usage(ExportError::Mp4Size { width, height }.to_string()));
            }
        }
        Some(Ffmpeg::find()?)
    } else {
        None
    };
    let source = Source::open(&args.file)?;
    if source.is_bundle() {
        args.data.warn_unused_with_bundle(typed);
    }
    if args.look.time.is_some() && !source.has_times() {
        return Err(usage(
            "--time needs a track with times, to know which day to light",
        ));
    }
    let mut bundle = source.into_bundle(&args.file, &args.data)?;
    if let Some(exaggeration) = args.exaggeration {
        bundle.grid.exaggeration = exaggeration;
    }
    info!(exaggeration = bundle.grid.exaggeration, "terrain");
    let conditions = args.scene.conditions();
    let sun = crate::sun::hike_sun(&bundle, args.look.time, &conditions)?;
    let aim = args.frame.aim();
    if let Some(motion) = &mut motion {
        let held = args.look.time.is_some() || args.scene.holds_sun();
        motion.sun = held.then_some(sun);
        motion.conditions = conditions;
        motion.aim = aim;
    }
    let text = args.overlay.text(&bundle)?;
    let fitted = shapes
        .iter()
        .zip(&framings)
        .map(|(shape, framing)| {
            let route_colour = args.scene.route_colour();
            args.overlay
                .fit(&text, &shape.label(), framing, args.scale, route_colour)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    if motion.is_some_and(|motion| motion.pace == Pace::Time) && !Walk::new(&bundle).is_timed() {
        warn!("the track has no times to pace the route by, so it draws at an even speed");
    }

    let gpu = Gpu::headless()?;
    for ((framing, fit), output) in framings.iter().zip(&fitted).zip(&outputs) {
        let overlays = &fit.overlays;
        let (width, height) = (framing.width * args.scale, framing.height * args.scale);
        let size = format!(
            "{width} × {height} from {} × {} pixels",
            framing.width, framing.height
        );
        if let Some(motion) = &motion {
            let clip = Clip {
                gpu: &gpu,
                bundle: &bundle,
                mesher: mesher.as_ref(),
                overlays,
                look: &look,
                motion,
            };
            let frames = write_clip(args, &clip, output, ffmpeg.as_ref())?;
            println!("Rendered {frames} frames of {size}");
        } else {
            let frame = still(&gpu, &bundle, mesher.as_ref(), overlays, &aim, &sun, &look)?;
            let image = upscale(&frame, args.scale)?;
            output::write(output, |file| Ok(encode_png(&image, file)?))?;
            println!("Rendered {size}");
        }
        println!("Wrote {}", output.display());
    }
    let dropped = fitted.iter().any(Fitted::credit_dropped);
    if let Some(credit) = args.overlay.caption_credit(&bundle, dropped) {
        println!("Credit for the caption: {credit}");
    }
    Ok(())
}

/// Renders the still with its overlays at internal resolution
pub(crate) fn still(
    gpu: &Gpu,
    bundle: &SceneBundle,
    mesher: &dyn Mesher,
    overlays: &Overlays,
    aim: &Aim,
    sun: &SunState,
    look: &Look,
) -> anyhow::Result<Frame> {
    let mut frame = render_still(
        gpu,
        bundle,
        mesher,
        &overlays.diorama_framing(),
        aim,
        sun,
        look,
    )?;
    overlays.draw(&mut frame, look.palette.as_ref(), 1.0);
    Ok(frame)
}

struct Clip<'a> {
    gpu: &'a Gpu,
    bundle: &'a SceneBundle,
    mesher: &'a dyn Mesher,
    overlays: &'a Overlays,
    look: &'a Look,
    motion: &'a Motion,
}

enum ClipWriter {
    Gif(GifWriter<BufWriter<File>>),
    Mp4(Mp4Writer),
}

impl ClipWriter {
    /// Writes into `pending`'s temporary file, as an MP4 when given ffmpeg
    /// and a GIF otherwise
    fn open(
        pending: &Pending,
        clip: &Clip,
        scale: u32,
        ffmpeg: Option<&Ffmpeg>,
    ) -> anyhow::Result<ClipWriter> {
        let framing = clip.overlays.diorama_framing();
        let (width, height) = (framing.width, framing.height);
        let fps = clip.motion.timeline.fps;
        Ok(match ffmpeg {
            Some(ffmpeg) => ClipWriter::Mp4(Mp4Writer::create(
                ffmpeg,
                pending.temp(),
                width,
                height,
                scale,
                fps,
            )?),
            None => ClipWriter::Gif(GifWriter::new(
                pending.open()?,
                width,
                height,
                scale,
                clip.look,
                fps,
            )?),
        })
    }

    fn push(&mut self, frame: &Frame) -> Result<(), ExportError> {
        match self {
            ClipWriter::Gif(gif) => gif.push(frame),
            ClipWriter::Mp4(mp4) => mp4.push(frame),
        }
    }

    fn finish(self) -> anyhow::Result<()> {
        match self {
            ClipWriter::Gif(gif) => gif.finish()?.flush()?,
            ClipWriter::Mp4(mp4) => mp4.finish()?,
        }
        Ok(())
    }
}

/// Renders the animation into `output`, which is only replaced once the
/// whole clip is written, and returns how many frames it has
fn write_clip(
    args: &RenderArgs,
    clip: &Clip,
    output: &Path,
    ffmpeg: Option<&Ffmpeg>,
) -> anyhow::Result<u32> {
    let write = || -> anyhow::Result<u32> {
        let pending = Pending::new(output);
        let mut writer = ClipWriter::open(&pending, clip, args.scale, ffmpeg)?;
        let frames = draw_clip(args, clip, |frame| Ok(writer.push(frame)?))?;
        writer.finish()?;
        pending.commit()?;
        Ok(frames)
    };
    write().with_context(|| format!("could not write {}", output.display()))
}

/// Draws each frame of the animation with its stats counting up, hands it
/// to `push`, and returns how many there were
fn draw_clip(
    args: &RenderArgs,
    clip: &Clip,
    mut push: impl FnMut(&Frame) -> anyhow::Result<()>,
) -> anyhow::Result<u32> {
    let framing = clip.overlays.diorama_framing();
    let walk = Walk::new(clip.bundle);
    let palette = clip.look.palette.as_ref();
    let mut frames = 0;
    render_clip(
        clip.gpu,
        clip.bundle,
        clip.mesher,
        &framing,
        clip.look,
        clip.motion,
        |moment, frame| -> anyhow::Result<()> {
            output::check_interrupted()?;
            let mut frame = frame.clone();
            let walked = walk.stats_at(moment);
            let stats = args.overlay.walked_stats(clip.bundle, &walked);
            clip.overlays
                .with_stats(&stats)
                .draw(&mut frame, palette, moment.progress);
            push(&frame)?;
            frames += 1;
            Ok(())
        },
    )?;
    Ok(frames)
}

/// A scene bundle as it is, or a track still to be made into one
pub(crate) enum Source {
    Bundle(Box<SceneBundle>),
    Track(Track),
}

impl Source {
    /// Reads the file without fetching anything
    pub fn open(path: &Path) -> anyhow::Result<Source> {
        let bytes = crate::read_file(path)?;
        if crate::is_bundle_file(path, &bytes) {
            let bundle = SceneBundle::from_bytes(&bytes)
                .with_context(|| format!("could not read {}", path.display()))?;
            Ok(Source::Bundle(Box::new(bundle)))
        } else {
            Ok(Source::Track(crate::parse_track(&bytes, path)?))
        }
    }

    pub fn is_bundle(&self) -> bool {
        matches!(self, Source::Bundle(_))
    }

    pub fn has_times(&self) -> bool {
        match self {
            Source::Bundle(bundle) => bundle.meta.local_date.is_some(),
            Source::Track(track) => track.points().any(|point| point.time.is_some()),
        }
    }

    /// The bundle with the data flags applied, fetching what a track needs
    pub fn into_bundle(self, path: &Path, data: &DataArgs) -> anyhow::Result<SceneBundle> {
        match self {
            Source::Bundle(bundle) => {
                let mut bundle = *bundle;
                if data.no_weather {
                    bundle.weather = None;
                    bundle.meta.provenance.weather = None;
                    crate::season::without_weather(&mut bundle.season);
                }
                crate::season::apply(&mut bundle, data.season);
                Ok(bundle)
            }
            Source::Track(track) => Ok(crate::bundle::prepare(&track, path, data)?.bundle),
        }
    }
}

/// One file per shape, named after its aspect when there are several, and
/// the format their extension asks for
fn output_paths(
    input: &Path,
    output: Option<&Path>,
    shapes: &[Shape],
) -> anyhow::Result<(Vec<PathBuf>, Format)> {
    let output = match output {
        Some(path) => path.to_owned(),
        None => {
            let mut name = input
                .file_stem()
                .with_context(|| format!("{} has no file name", input.display()))?
                .to_owned();
            name.push(".png");
            PathBuf::from(name)
        }
    };
    let extension = output.extension().unwrap_or_default().to_owned();
    let format = if extension.eq_ignore_ascii_case("png") {
        Format::Png
    } else if extension.eq_ignore_ascii_case("gif") {
        Format::Gif
    } else if extension.eq_ignore_ascii_case("mp4") {
        Format::Mp4
    } else {
        return Err(usage(format!(
            "cannot write {}: only .png, .gif and .mp4 output are supported",
            output.display()
        )));
    };
    if shapes.len() == 1 {
        return Ok((vec![output], format));
    }
    let stem = output
        .file_stem()
        .with_context(|| format!("{} has no file name", output.display()))?;
    let paths = shapes
        .iter()
        .map(|shape| {
            let mut name = stem.to_owned();
            name.push(format!("-{}.", shape.label()));
            name.push(&extension);
            output.with_file_name(name)
        })
        .collect();
    Ok((paths, format))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use anyhow::bail;
    use clap::Parser;
    use isotrail_testkit::hike_bundle;

    use super::*;

    #[derive(Debug, Parser)]
    struct Cli {
        #[command(flatten)]
        render: RenderArgs,
    }

    #[test]
    fn a_clip_that_fails_part_way_leaves_the_old_file() {
        let args = Cli::try_parse_from(["isotrail", "hike.itb", "--duration", "1", "--fps", "5"])
            .unwrap()
            .render;
        let bundle = hike_bundle();
        let framing = Aspect::Square.framing(10).unwrap();
        let text = args.overlay.text(&bundle).unwrap();
        let overlays = Overlays::layout(
            &text,
            &args.overlay.style(args.scene.route_colour()),
            &framing,
        );
        let motion = args.animation.motion().unwrap();
        let gpu = isotrail_testkit::gpu();
        let clip = Clip {
            gpu: &gpu,
            bundle: &bundle,
            mesher: &isotrail_render::VoxelMesher::default(),
            overlays: &overlays,
            look: &Look::default(),
            motion: &motion,
        };
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("clip.gif");
        fs::write(&target, "good").unwrap();

        let pending = Pending::new(&target);
        let mut writer = ClipWriter::open(&pending, &clip, 10, None).unwrap();
        let mut pushed = 0;
        let drawn = draw_clip(&args, &clip, |frame| {
            pushed += 1;
            if pushed == 3 {
                bail!("the disk is full");
            }
            Ok(writer.push(frame)?)
        });
        assert!(drawn.is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        drop((writer, pending));

        assert_eq!(fs::read(&target).unwrap(), b"good");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    fn output_path(input: &Path, output: Option<&Path>) -> anyhow::Result<PathBuf> {
        Ok(
            output_paths(input, output, &[Shape::Aspect(Aspect::Story)])?
                .0
                .remove(0),
        )
    }

    #[test]
    fn output_defaults_to_the_input_name_here() {
        let path = output_path(Path::new("hikes/Derwent Edge.gpx"), None).unwrap();
        assert_eq!(path, Path::new("Derwent Edge.png"));
        let path = output_path(Path::new("ridge.v2.itb"), None).unwrap();
        assert_eq!(path, Path::new("ridge.v2.png"));
    }

    #[test]
    fn several_aspects_name_their_images() {
        let aspects = [Shape::Aspect(Aspect::Story), Shape::Aspect(Aspect::Wide)];
        let output = Some(Path::new("out/a.v2.PNG"));
        let (paths, format) = output_paths(Path::new("hike.gpx"), output, &aspects).unwrap();
        assert_eq!(format, Format::Png);
        assert_eq!(
            paths,
            [
                Path::new("out/a.v2-story.PNG"),
                Path::new("out/a.v2-wide.PNG")
            ]
        );
        let (paths, _) = output_paths(Path::new("hikes/ridge.itb"), None, &aspects).unwrap();
        assert_eq!(
            paths,
            [Path::new("ridge-story.png"), Path::new("ridge-wide.png")]
        );
    }

    #[test]
    fn png_gif_and_mp4_output_are_accepted() {
        let input = Path::new("hike.gpx");
        let feed = [Shape::Aspect(Aspect::Feed)];
        let format = |output: &str| output_paths(input, Some(Path::new(output)), &feed);
        assert_eq!(format("out/a.PNG").unwrap().1, Format::Png);
        assert_eq!(format("a.gif").unwrap().1, Format::Gif);
        assert_eq!(format("a.MP4").unwrap().1, Format::Mp4);
        let aspects = [Shape::Aspect(Aspect::Story), Shape::Aspect(Aspect::Wide)];
        let (paths, _) = output_paths(input, Some(Path::new("a.Gif")), &aspects).unwrap();
        assert_eq!(paths, [Path::new("a-story.Gif"), Path::new("a-wide.Gif")]);
        for bad in ["a.webm", "a"] {
            let message = format(bad).unwrap_err().to_string();
            assert!(message.contains("only .png, .gif and .mp4"), "{message}");
        }
    }
}
