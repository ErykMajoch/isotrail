use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::Args;
use isotrail_export::{DEFAULT_SCALE, upscale, write_png};
use isotrail_render::{Aspect, Gpu, Look, Overlays, SunState, VoxelMesher};
use tracing::warn;

use crate::bundle::DataArgs;
use crate::overlay::OverlayArgs;
use crate::render::AspectArg;

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

    /// Directory to write the scene bundle, poster image and snippet.html to
    #[arg(short, long, value_name = "DIR")]
    output: PathBuf,

    /// Shape of the embed and its poster: story (9:16), feed (4:5), square or
    /// wide (16:9)
    #[arg(long, value_enum, default_value_t = AspectArg::Wide)]
    aspect: AspectArg,

    /// How many times taller than life to draw the terrain; chosen from the
    /// slab's size and relief by default
    #[arg(long, value_name = "X", value_parser = crate::bundle::parse_exaggeration)]
    exaggeration: Option<f64>,

    /// Also copy the web runtime (isotrail-embed.js and pkg/) into the
    /// directory
    #[arg(long)]
    with_runtime: bool,

    /// Where to copy the web runtime from; by default the web folder next to
    /// the isotrail executable
    #[arg(long, value_name = "DIR", requires = "with_runtime")]
    runtime_dir: Option<PathBuf>,

    /// The address the directory will be served at, such as /hikes/derwent/;
    /// by default the snippet uses bare file names
    #[arg(long, value_name = "URL")]
    base_url: Option<String>,

    /// The address of isotrail-embed.js, when the site serves one copy for
    /// all its hikes; by default it sits in the directory
    #[arg(long, value_name = "URL")]
    runtime_url: Option<String>,

    #[command(flatten)]
    overlay: OverlayArgs,

    #[command(flatten)]
    data: DataArgs,
}

pub fn run(args: &EmbedArgs) -> anyhow::Result<()> {
    let runtime = if args.with_runtime {
        Some(runtime_dir(args.runtime_dir.as_deref())?)
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
    let mut bundle = crate::render::read_scene(&args.file, &args.data)?;
    if let Some(exaggeration) = args.exaggeration {
        bundle.grid.exaggeration = exaggeration;
    }
    let text = args.overlay.text(&bundle)?;
    let style = args.overlay.style();

    let dir = &args.output;
    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    let bundle_path = dir.join(format!("{name}.itb"));
    let bytes = bundle.to_bytes();
    fs::write(&bundle_path, &bytes)
        .with_context(|| format!("could not write {}", bundle_path.display()))?;
    println!(
        "Wrote {} ({} KB)",
        bundle_path.display(),
        bytes.len().div_ceil(1024)
    );

    let aspect = Aspect::from(args.aspect);
    let framing = aspect
        .framing(DEFAULT_SCALE)
        .context("the pixel scale doesn't divide the image size")?;
    let overlays = Overlays::layout(&text, &style, &framing);
    let poster_path = dir.join(format!("{name}.png"));
    let poster = crate::render::still(
        &Gpu::headless()?,
        &bundle,
        &VoxelMesher::default(),
        &overlays,
        &SunState::for_hike(&bundle),
        &Look::default(),
    )?;
    write_png(&poster_path, &upscale(&poster, DEFAULT_SCALE)?)?;
    println!("Wrote {}", poster_path.display());

    if let Some(runtime) = runtime {
        for file in RUNTIME_FILES {
            let (from, to) = (runtime_file(&runtime, file), runtime_file(dir, file));
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("could not create {}", parent.display()))?;
            }
            fs::copy(&from, &to).with_context(|| {
                format!("could not copy {} to {}", from.display(), to.display())
            })?;
            println!("Wrote {}", to.display());
        }
    }

    let html = snippet(
        &name,
        aspect,
        args.base_url.as_deref(),
        args.runtime_url.as_deref(),
    );
    let snippet_path = dir.join("snippet.html");
    fs::write(&snippet_path, &html)
        .with_context(|| format!("could not write {}", snippet_path.display()))?;
    println!("Wrote {}\n\n{html}", snippet_path.display());
    if let Some(credit) = args.overlay.caption_credit(&bundle) {
        println!("Credit for the caption: {credit}");
    }
    Ok(())
}

/// The runtime folder, checked for every file before any work starts
fn runtime_dir(given: Option<&Path>) -> anyhow::Result<PathBuf> {
    let dir = match given {
        Some(dir) => dir.to_owned(),
        None => std::env::current_exe()
            .context("could not find the isotrail executable")?
            .parent()
            .context("could not find the isotrail executable's folder")?
            .join("web"),
    };
    for file in RUNTIME_FILES {
        let path = runtime_file(&dir, file);
        if !path.is_file() {
            let hint = if given.is_some() {
                "point --runtime-dir at a folder holding isotrail-embed.js and pkg/"
            } else {
                "the release downloads have it in a web folder next to isotrail; or point \
                 --runtime-dir at one, such as web/ in the repository after sh web/build.sh"
            };
            bail!("the web runtime has no {}: {hint}", path.display());
        }
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

fn snippet(
    name: &str,
    aspect: Aspect,
    base_url: Option<&str>,
    runtime_url: Option<&str>,
) -> String {
    let mut base = base_url.unwrap_or_default().to_owned();
    if !base.is_empty() && !base.ends_with('/') {
        base.push('/');
    }
    let file = |extension: &str| format!("{base}{}", url_segment(&format!("{name}.{extension}")));
    let script = runtime_url.map_or_else(|| format!("{base}{}", RUNTIME_FILES[0]), str::to_owned);
    let mut html = String::new();
    let _ = writeln!(
        html,
        r#"<script type="module" src="{}"></script>"#,
        html_attr(&script)
    );
    let _ = writeln!(
        html,
        r#"<isotrail-hike src="{}" poster="{}" aspect="{}"></isotrail-hike>"#,
        html_attr(&file("itb")),
        html_attr(&file("png")),
        aspect.name()
    );
    html
}

/// Percent-encodes everything but the characters a URL path segment can hold
/// as they are
fn url_segment(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@".contains(&byte) {
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

    #[test]
    fn snippet_paths_follow_the_base_url() {
        let html = snippet("ridge", Aspect::Wide, None, None);
        assert_eq!(
            html,
            "<script type=\"module\" src=\"isotrail-embed.js\"></script>\n\
             <isotrail-hike src=\"ridge.itb\" poster=\"ridge.png\" aspect=\"wide\"></isotrail-hike>\n"
        );
        let html = snippet("Tom & Jo", Aspect::Story, Some("/hikes/x"), None);
        assert!(
            html.contains(r#"src="/hikes/x/isotrail-embed.js""#),
            "{html}"
        );
        assert!(
            html.contains(r#"src="/hikes/x/Tom%20&amp;%20Jo.itb""#),
            "{html}"
        );
        assert!(
            html.contains(r#"poster="/hikes/x/Tom%20&amp;%20Jo.png""#),
            "{html}"
        );
        assert!(html.contains(r#"aspect="story""#), "{html}");
        let html = snippet(
            "r",
            Aspect::Square,
            Some("https://a.b/h/"),
            Some("/js/e.js"),
        );
        assert!(
            html.contains(r#"<script type="module" src="/js/e.js">"#),
            "{html}"
        );
        assert!(html.contains(r#"src="https://a.b/h/r.itb""#), "{html}");
    }
}
