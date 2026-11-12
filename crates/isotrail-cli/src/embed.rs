use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::Args;
use isotrail_bundle::SceneBundle;
use isotrail_export::{DEFAULT_SCALE, encode_png, upscale};
use isotrail_render::{Aim, Aspect, Gpu, PITCH_DEG};
use tracing::warn;

use crate::bundle::DataArgs;
use crate::config::Typed;
use crate::frame::{AspectArg, FrameArgs, Shape, parse_scale};
use crate::look::LookArgs;
use crate::output;
use crate::overlay::OverlayArgs;
use crate::playback::PlaybackArgs;
use crate::render::Source;
use crate::report::{Report, ReportArgs, Written, say, shown};
use crate::scene::SceneArgs;
use crate::theme::ThemeFileArg;
use crate::usage;

/// The element's script and the WebAssembly renderer it loads, at the paths
/// the script expects them relative to itself
const RUNTIME_FILES: [&str; 3] = [
    "isotrail-embed.js",
    "pkg/isotrail_web.js",
    "pkg/isotrail_web_bg.wasm",
];

#[derive(Debug, Args)]
pub struct EmbedArgs {
    /// GPX file or scene bundle (.itb) to put on a web page
    file: PathBuf,

    /// Folder to write the scene bundle, poster image and snippet.html to
    #[arg(short, long, value_name = "DIR", help_heading = crate::OUTPUT)]
    output: PathBuf,

    /// Also copy the web runtime (isotrail-embed.js and pkg/) into the folder
    #[arg(long, help_heading = crate::OUTPUT)]
    with_runtime: bool,

    /// Where to copy the web runtime from [default: the web folder next to isotrail]
    #[arg(long, value_name = "DIR", requires = "with_runtime", help_heading = crate::OUTPUT)]
    runtime_dir: Option<PathBuf>,

    /// The address the folder is served at, such as /hikes/derwent/ [default: bare names]
    #[arg(long, value_name = "URL", help_heading = crate::OUTPUT)]
    base_url: Option<String>,

    /// The address of isotrail-embed.js, when one copy serves every hike on the site
    #[arg(long, value_name = "URL", help_heading = crate::OUTPUT)]
    runtime_url: Option<String>,

    #[command(flatten)]
    report: ReportArgs,

    /// Shape of the embed and its poster: story, video, portrait, feed, square or wide
    #[arg(long, value_enum, value_name = "SHAPE", default_value_t = AspectArg::Wide,
          hide_possible_values = true, help_heading = crate::IMAGE)]
    aspect: AspectArg,

    /// Poster pixels per drawn pixel, from 1 to 16, dividing both sides of the poster
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
    playback: PlaybackArgs,

    #[command(flatten)]
    data: DataArgs,

    /// Read settings from the [embed] and [render] tables of this TOML file
    #[arg(long, value_name = "FILE", help_heading = crate::GENERAL)]
    config: Option<PathBuf>,
}

pub fn run(args: &EmbedArgs, typed: &Typed) -> anyhow::Result<()> {
    crate::check_input(&args.file)?;
    let mut report = Report::new(&args.report);
    let shape = args.frame.shapes(&[Aspect::from(args.aspect)])[0];
    let framing = args.frame.framings(&[shape], args.scale)?[0];
    let dir = &args.output;
    let runtime = if args.with_runtime {
        let runtime = runtime_dir(args.runtime_dir.as_deref())?;
        if output::same_file(&runtime, dir) {
            return Err(usage(format!(
                "--runtime-dir {} is the folder being written to; copy the runtime from another folder",
                runtime.display()
            )));
        }
        Some(runtime)
    } else {
        None
    };
    for (flag, url) in [
        ("--base-url", &args.base_url),
        ("--runtime-url", &args.runtime_url),
    ] {
        if let Some(url) = url.as_deref().filter(|url| looks_like_a_drive_path(url)) {
            warn!(
                "{flag} {url} looks like a file path, not a web address; Git Bash turns \
                 arguments starting with / into Windows paths unless MSYS_NO_PATHCONV=1 is set"
            );
        }
    }
    let name = args
        .file
        .file_stem()
        .with_context(|| format!("{} has no file name", args.file.display()))?
        .to_string_lossy()
        .into_owned();
    let bundle_path = dir.join(format!("{name}.itb"));
    let poster_path = dir.join(format!("{name}.png"));
    let snippet_path = dir.join("snippet.html");
    let mut copies: Vec<(PathBuf, PathBuf)> = runtime
        .iter()
        .flat_map(|runtime| {
            RUNTIME_FILES.map(|file| (runtime_file(runtime, file), runtime_file(dir, file)))
        })
        .collect();
    // Palette and theme files go beside the bundle, for the live embed to fetch
    let mut beside = |file: Option<&Path>, extension: &str| -> anyhow::Result<Option<String>> {
        let Some(file) = file else {
            return Ok(None);
        };
        let mut copy = file
            .file_stem()
            .with_context(|| format!("{} has no file name", file.display()))?
            .to_owned();
        copy.push(extension);
        let url = format!(
            "{}{}",
            base_path(args.base_url.as_deref()),
            url_segment(&copy.to_string_lossy())
        );
        let copy = dir.join(copy);
        if !output::same_file(file, &copy) {
            copies.push((file.to_owned(), copy));
        }
        Ok(Some(url))
    };
    let palette_url = beside(args.look.palette_file(), ".hex")?;
    let theme_url = beside(args.theme.path(), ".toml")?;
    // The bundle being embedded may already sit where it would be written
    let in_place = output::same_file(&args.file, &bundle_path);
    output::folder(dir)?;
    let outputs = [&poster_path, &snippet_path]
        .into_iter()
        .chain((!in_place).then_some(&bundle_path))
        .chain(copies.iter().map(|(_, to)| to));
    let inputs: Vec<&Path> = [Some(args.file.as_path()), args.config.as_deref()]
        .into_iter()
        .flatten()
        .chain(args.look.palette_file().filter(|path| path.is_file()))
        .chain(args.theme.path().filter(|path| path.is_file()))
        .chain(args.playback.strings_file().filter(|path| path.is_file()))
        .collect();
    output::plan(outputs.map(PathBuf::as_path), &inputs)?;
    args.overlay.check()?;
    args.playback.check()?;
    let theme = args.theme.read()?;
    let mesher = args.look.mesher(typed, args.scene.terrain(&theme));
    let look = args.scene.apply(args.look.look(&theme)?);

    let source = Source::open(&args.file)?;
    if source.is_bundle() {
        args.data.warn_unused_with_bundle(typed);
    }
    if args.look.time.is_some() && !source.has_times() {
        return Err(usage(
            "--time needs a track with times, to know which day to light",
        ));
    }
    let original = match (&source, in_place) {
        (Source::Bundle(bundle), true) => Some(bundle.clone()),
        _ => None,
    };
    let mut bundle = source.into_bundle(&args.file, &args.data)?;
    if let Some(exaggeration) = args.exaggeration {
        bundle.grid.exaggeration = exaggeration;
    }
    // Saved in the bundle, so the live embed is labelled the same
    if let Some(title) = args.overlay.given_title() {
        title.clone_into(&mut bundle.meta.title);
    }
    if original.is_some_and(|original| *original != bundle) {
        return Err(usage(format!(
            "{} is the bundle being embedded, and the flags would change it; write to another folder",
            bundle_path.display()
        )));
    }
    let text = args.overlay.text(&bundle, &theme.labels)?;
    let built_in = args.overlay.built_in_text(&bundle, &theme.labels);
    let themed = built_in.as_ref().zip(args.theme.path());
    let style = args.overlay.style(&theme, look.route.colour);
    let fitted = args
        .overlay
        .fit(&text, themed, &shape.label(), &framing, args.scale, &style)?;
    let aim = args.frame.aim();
    let conditions = args.scene.conditions(&theme);
    let sun = crate::sun::hike_sun(&bundle, args.look.time, &conditions)?;

    if in_place {
        report.kept(Written::file(&bundle_path));
    } else {
        let bytes = bundle.to_bytes()?;
        output::write(&bundle_path, |file| Ok(file.write_all(&bytes)?))?;
        say!(
            "Wrote {} ({} KB)",
            shown(&bundle_path),
            bytes.len().div_ceil(1024)
        );
        report.add(Written::file(&bundle_path));
    }

    let poster = crate::render::still(
        &Gpu::headless()?,
        &bundle,
        mesher.as_ref(),
        &fitted.overlays,
        &aim,
        &sun,
        &look,
    )?;
    let poster = upscale(&poster, args.scale)?;
    output::write(&poster_path, |file| Ok(encode_png(&poster, file)?))?;
    report.wrote(Written::image(
        &poster_path,
        (poster.width(), poster.height()),
    ));

    for (from, to) in &copies {
        output::write(to, |file| {
            let mut source =
                File::open(from).with_context(|| format!("can't copy {}", from.display()))?;
            io::copy(&mut source, file)?;
            Ok(())
        })?;
        report.wrote(Written::file(to));
    }

    let mut attributes = args.look.attributes(palette_url.as_deref());
    attributes.extend(theme_url.map(|url| ("theme", url)));
    attributes.extend(args.scene.attributes(&theme.route));
    attributes.extend(view_attributes(&aim, &bundle));
    attributes.extend(args.overlay.attributes());
    attributes.extend(args.playback.attributes()?);
    let html = snippet(&Snippet {
        name: &name,
        title: args.overlay.title(&bundle),
        shape,
        base_url: args.base_url.as_deref(),
        runtime_url: args.runtime_url.as_deref(),
        attributes: &attributes,
    });
    output::write(&snippet_path, |file| Ok(file.write_all(html.as_bytes())?))?;
    say!("Wrote {}\n\n{html}", shown(&snippet_path));
    report.add(Written::file(&snippet_path));
    report.finish(
        args.overlay
            .caption_credit(&bundle, fitted.credit_dropped())
            .as_deref(),
    )
}

/// The element's starting view, for each part that isn't the default; a
/// compass heading becomes the turn from the default view that matches it
fn view_attributes(aim: &Aim, bundle: &SceneBundle) -> Vec<(&'static str, String)> {
    let mut attributes = Vec::new();
    let turn = aim.as_turn(bundle);
    if turn != 0.0 {
        attributes.push(("turn", turn.to_string()));
    }
    if aim.pitch_deg != PITCH_DEG {
        attributes.push(("pitch", aim.pitch_deg.to_string()));
    }
    if aim.zoom != 1.0 {
        attributes.push(("zoom", aim.zoom.to_string()));
    }
    attributes
}

/// The runtime folder, checked for every file before any work starts
fn runtime_dir(given: Option<&Path>) -> anyhow::Result<PathBuf> {
    let dir = match given {
        Some(dir) => dir.to_owned(),
        None => std::env::current_exe()
            .context("can't find the isotrail program")?
            .parent()
            .context("can't find the isotrail program's folder")?
            .join("web"),
    };
    if RUNTIME_FILES
        .iter()
        .any(|file| !runtime_file(&dir, file).is_file())
    {
        let hint = if given.is_some() {
            "it needs isotrail-embed.js and the pkg folder"
        } else {
            "keep the web folder next to isotrail, or name it with --runtime-dir"
        };
        bail!("can't find the web runtime in {}; {hint}", dir.display());
    }
    Ok(dir)
}

fn looks_like_a_drive_path(url: &str) -> bool {
    let bytes = url.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

/// One of [`RUNTIME_FILES`] under `dir`, with the system's separators
fn runtime_file(dir: &Path, file: &str) -> PathBuf {
    file.split('/')
        .fold(dir.to_owned(), |path, part| path.join(part))
}

/// What goes into snippet.html
struct Snippet<'a> {
    name: &'a str,
    title: &'a str,
    shape: Shape,
    base_url: Option<&'a str>,
    runtime_url: Option<&'a str>,
    /// The options that aren't the default; an empty value is written as
    /// the attribute's name alone
    attributes: &'a [(&'static str, String)],
}

/// `--base-url` ending in a slash, or nothing
fn base_path(base_url: Option<&str>) -> String {
    let mut base = base_url.unwrap_or_default().to_owned();
    if !base.is_empty() && !base.ends_with('/') {
        base.push('/');
    }
    base
}

/// The script, and the element with its poster inside, so the poster shows
/// at its size even when the script can't run. The script's address carries
/// the version, so browsers fetch a new runtime after an update
fn snippet(snippet: &Snippet) -> String {
    let base = base_path(snippet.base_url);
    let name = snippet.name;
    let file = |extension: &str| format!("{base}{}", url_segment(&format!("{name}.{extension}")));
    let version = concat!("v=", env!("CARGO_PKG_VERSION"));
    let script = match snippet.runtime_url {
        Some(url) if url.contains('?') => url.to_owned(),
        Some(url) => format!("{url}?{version}"),
        None => format!("{base}{}?{version}", RUNTIME_FILES[0]),
    };
    let (width, height) = snippet.shape.size();
    let common = gcd(width, height);
    let aspect = match snippet.shape {
        Shape::Aspect(aspect) => aspect.name().to_owned(),
        Shape::Size(..) => format!("{}:{}", width / common, height / common),
    };
    let alt = match snippet.title.trim() {
        "" => "Diorama of a hike".to_owned(),
        title => format!("Diorama of {title}"),
    };
    let poster = html_attr(&file("png"));
    let look: String = snippet
        .attributes
        .iter()
        .map(|(name, value)| match value.as_str() {
            "" => format!(" {name}"),
            value => format!(r#" {name}="{}""#, html_attr(value)),
        })
        .collect();
    let mut html = String::new();
    let _ = writeln!(
        html,
        r#"<script type="module" src="{}"></script>"#,
        html_attr(&script)
    );
    let _ = writeln!(
        html,
        r#"<isotrail-hike src="{}" poster="{poster}" aspect="{}"{look} style="display:block;aspect-ratio:{}/{}"><img src="{poster}" alt="{}" style="display:block;width:100%"></isotrail-hike>"#,
        html_attr(&file("itb")),
        aspect,
        width / common,
        height / common,
        html_attr(&alt),
    );
    html
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Percent-encodes everything but the characters a URL path segment can hold
/// as they are
fn url_segment(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        // A `:` would turn the start of a relative address into a scheme
        if byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=@".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn html_attr(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_encoded_for_urls_and_html() {
        assert_eq!(url_segment("ridge_walk-2.itb"), "ridge_walk-2.itb");
        assert_eq!(
            url_segment("Łąka 50%#1?.png"),
            "%C5%81%C4%85ka%2050%25%231%3F.png"
        );
        assert_eq!(url_segment("a/b"), "a%2Fb");
        assert_eq!(url_segment("Rysy:2025.itb"), "Rysy%3A2025.itb");
        assert!(looks_like_a_drive_path("C:/Program Files/Git/hikes/"));
        assert!(looks_like_a_drive_path(r"d:\hikes"));
        for url in ["/hikes/", "https://a.b/", "hikes/", "C:"] {
            assert!(!looks_like_a_drive_path(url), "{url}");
        }
        assert_eq!(
            html_attr(r#"a&b "c" <d>"#),
            "a&amp;b &quot;c&quot; &lt;d&gt;"
        );
    }

    fn plain(
        name: &str,
        title: &str,
        aspect: Aspect,
        base_url: Option<&str>,
        runtime_url: Option<&str>,
    ) -> String {
        snippet(&Snippet {
            name,
            title,
            shape: Shape::Aspect(aspect),
            base_url,
            runtime_url,
            attributes: &[],
        })
    }

    #[test]
    fn snippet_paths_follow_the_base_url() {
        let html = plain("ridge", "Ridge <walk>", Aspect::Wide, None, None);
        assert_eq!(
            html,
            format!(
                "<script type=\"module\" src=\"isotrail-embed.js?v={}\"></script>\n\
                 <isotrail-hike src=\"ridge.itb\" poster=\"ridge.png\" aspect=\"wide\" \
                 style=\"display:block;aspect-ratio:16/9\"><img src=\"ridge.png\" \
                 alt=\"Diorama of Ridge &lt;walk&gt;\" style=\"display:block;width:100%\">\
                 </isotrail-hike>\n",
                env!("CARGO_PKG_VERSION")
            )
        );
        let html = plain("Tom & Jo", " ", Aspect::Story, Some("/hikes/x"), None);
        assert!(
            html.contains(r#"src="/hikes/x/isotrail-embed.js?v="#),
            "{html}"
        );
        assert!(html.contains("aspect-ratio:9/16"), "{html}");
        assert!(html.contains(r#"alt="Diorama of a hike""#), "{html}");
        assert!(
            html.contains(r#"src="/hikes/x/Tom%20&amp;%20Jo.itb""#),
            "{html}"
        );
        assert!(
            html.contains(r#"poster="/hikes/x/Tom%20&amp;%20Jo.png""#),
            "{html}"
        );
        assert!(html.contains(r#"aspect="story""#), "{html}");
        let html = plain(
            "r",
            "R",
            Aspect::Square,
            Some("https://a.b/h/"),
            Some("/js/e.js"),
        );
        assert!(
            html.contains(&format!(
                r#"<script type="module" src="/js/e.js?v={}">"#,
                env!("CARGO_PKG_VERSION")
            )),
            "{html}"
        );
        assert!(html.contains(r#"src="https://a.b/h/r.itb""#), "{html}");
        assert!(html.contains("aspect-ratio:1/1"), "{html}");
        let html = plain("r", "R", Aspect::Feed, None, Some("/e.js?v=2"));
        assert!(html.contains(r#"src="/e.js?v=2""#), "{html}");
        assert!(html.contains("aspect-ratio:4/5"), "{html}");
        let html = plain("r", "R", Aspect::Portrait, None, None);
        assert!(html.contains(r#"aspect="portrait""#), "{html}");
        assert!(html.contains("aspect-ratio:3/4"), "{html}");
        let html = plain("r", "R", Aspect::Video, None, None);
        assert!(html.contains(r#"aspect="video""#), "{html}");
        assert!(html.contains("aspect-ratio:9/16"), "{html}");
    }

    #[test]
    fn a_given_size_becomes_its_ratio() {
        let html = snippet(&Snippet {
            name: "ridge",
            title: "Ridge",
            shape: Shape::Size(1200, 800),
            base_url: None,
            runtime_url: None,
            attributes: &[],
        });
        assert!(
            html.contains(r#"aspect="3:2" style="display:block;aspect-ratio:3/2""#),
            "{html}"
        );
    }

    #[test]
    fn the_starting_view_is_written_when_it_isnt_the_default() {
        let bundle = isotrail_testkit::hike_bundle();
        assert_eq!(view_attributes(&Aim::default(), &bundle), []);
        let aim = Aim {
            turn_deg: -30.0,
            pitch_deg: 45.0,
            zoom: 1.5,
            ..Aim::default()
        };
        assert_eq!(
            view_attributes(&aim, &bundle),
            [
                ("turn", "-30".to_owned()),
                ("pitch", "45".to_owned()),
                ("zoom", "1.5".to_owned())
            ]
        );
        let heading = Aim {
            heading_deg: Some(90.0),
            ..Aim::default()
        };
        let [("turn", turn)] = &view_attributes(&heading, &bundle)[..] else {
            panic!("a turn");
        };
        let turn: f32 = turn.parse().unwrap();
        let back = Aim {
            turn_deg: turn,
            ..Aim::default()
        };
        let diff = (back.heading(&bundle) - heading.heading(&bundle)).to_degrees();
        assert!((diff + 180.0).rem_euclid(360.0) - 180.0 < 0.01, "{diff}");
    }

    #[test]
    fn look_options_become_attributes_after_the_aspect() {
        let html = snippet(&Snippet {
            name: "ridge",
            title: "Ridge",
            shape: Shape::Aspect(Aspect::Wide),
            base_url: None,
            runtime_url: None,
            attributes: &[
                ("palette", "a&b.hex".to_owned()),
                ("mesher", "smooth".to_owned()),
            ],
        });
        assert!(
            html.contains(
                r#"aspect="wide" palette="a&amp;b.hex" mesher="smooth" style="display:block;"#
            ),
            "{html}"
        );
        assert_eq!(base_path(Some("/h")), "/h/");
        assert_eq!(base_path(Some("/h/")), "/h/");
        assert_eq!(base_path(None), "");
    }
}
