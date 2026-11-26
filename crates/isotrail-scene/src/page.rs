use isotrail_bundle::SceneBundle;
use isotrail_options::theme::theme_from_text;
use isotrail_options::{RenderOptions, Settings};
use isotrail_options::{data::SeasonArg, look::palette_from_text};

use crate::Failure;

/// The options read and checked as a build reads them, with the tool's
/// messages and the page's own refusals
pub fn read_options(options: &str) -> Result<RenderOptions, Failure> {
    let render = Settings::from_json(options, "the options")
        .and_then(|settings| settings.options())
        .map_err(|error| Failure::new("usage", format!("{error:#}")))?;
    if render.data.offline {
        return Err(Failure::new(
            "usage",
            "offline in the options: a page has no cache folder to build from",
        ));
    }
    if render.data.cache_dir.get().is_some() {
        return Err(Failure::new(
            "usage",
            "cache-dir in the options: a page has no cache folder",
        ));
    }
    Ok(render)
}

/// Checks a palette file's contents, named `name` in the message
pub fn check_palette(name: &str, text: &str) -> Result<(), Failure> {
    palette_from_text(name, text)
        .map(drop)
        .map_err(|error| Failure::new("usage", error.to_string()))
}

/// Checks a theme file's contents, named `name` in the message
pub fn check_theme(name: &str, text: &str) -> Result<(), Failure> {
    theme_from_text(name, text)
        .map(drop)
        .map_err(|error| Failure::new("usage", error.to_string()))
}

/// The scene file with the options that also change a finished scene applied,
/// as `isotrail render` applies them to one: `exaggeration`, `season` and
/// `no-weather`. Options that need the GPX file are refused
pub fn adjust(scene: &[u8], options: &str) -> Result<Vec<u8>, Failure> {
    let render = read_options(options)?;
    render
        .data
        .refuse_with_bundle()
        .map_err(|error| Failure::new("usage", error.to_string()))?;
    let unchanged = render.exaggeration.is_none()
        && !render.data.no_weather
        && matches!(render.data.season, SeasonArg::Auto);
    if unchanged {
        return Ok(scene.to_vec());
    }
    let damaged = |error: isotrail_bundle::BundleError| {
        Failure::new("input", format!("can't read the scene file: {error}"))
    };
    let mut bundle = SceneBundle::from_bytes(scene).map_err(damaged)?;
    render.data.apply_to(&mut bundle);
    if let Some(exaggeration) = render.exaggeration {
        bundle.grid.exaggeration = exaggeration;
    }
    bundle.to_bytes().map_err(damaged)
}
