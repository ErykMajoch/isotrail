mod print;
mod read;
mod tables;

use std::ops::RangeInclusive;

use glam::Vec3;

use crate::{
    Conditions, Labels, Light, Look, OverlayStyle, OverlayTheme, Pacing, RouteStyle, SKY_DITHER,
    SlabStyle, Terrain,
};

/// Colours, words and fine-tuning that a theme file can change, each at its
/// built-in value unless the file sets it
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub slab: SlabStyle,
    /// The day's sky is the last key of the light's ramp
    pub light: Light,
    pub sky_dither: f32,
    pub route: RouteStyle,
    pub overlay: OverlayTheme,
    pub labels: Labels,
    pub pacing: Pacing,
}

impl Default for Theme {
    fn default() -> Theme {
        Theme {
            slab: SlabStyle::default(),
            light: Light::default(),
            sky_dither: SKY_DITHER,
            route: RouteStyle::default(),
            overlay: OverlayTheme::default(),
            labels: Labels::default(),
            pacing: Pacing::default(),
        }
    }
}

/// What is wrong with a theme file
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ThemeError {
    #[error("{0}")]
    Toml(String),
    #[error("unknown table [{0}]")]
    UnknownTable(String),
    #[error("unknown key {key:?} in the [{table}] table")]
    UnknownKey { table: String, key: String },
    #[error("{key:?} is outside a table; keys go in tables such as [route]")]
    OutsideTable { key: String },
    #[error("{key}: {message}")]
    Value { key: String, message: String },
}

impl Theme {
    /// Reads a theme file's text; keys it leaves out keep their built-in
    /// values
    pub fn parse(text: &str) -> Result<Theme, ThemeError> {
        read::read(text)
    }

    /// The theme as a file with every key, which [`Theme::parse`] reads back
    /// as it is
    pub fn to_toml(&self) -> String {
        print::print(self)
    }

    /// The built-in look with the theme's sky and route
    pub fn look(&self) -> Look {
        Look {
            sky_dither: self.sky_dither,
            route: self.route,
            ..Look::default()
        }
    }

    pub fn terrain(&self) -> Terrain {
        Terrain {
            style: self.slab,
            ..Terrain::default()
        }
    }

    pub fn conditions(&self) -> Conditions {
        Conditions {
            light: self.light,
            ..Conditions::default()
        }
    }

    pub fn overlay_style(&self) -> OverlayStyle {
        OverlayStyle {
            route_colour: self.route.colour,
            theme: self.overlay.clone(),
            ..OverlayStyle::default()
        }
    }
}

/// One value of a theme, and what it may hold
pub(crate) enum Field<'a> {
    /// `#rrggbb`
    Colour(&'a mut [u8; 3]),
    /// `#rrggbb`, keeping the alpha
    Rgba(&'a mut [u8; 4]),
    /// A list of exactly this many `#rrggbb`
    Rgbas(&'a mut [[u8; 4]]),
    /// Red, green and blue strengths, or `#rrggbb` as shares of 255
    Light(&'a mut Vec3),
    Number(&'a mut f32, RangeInclusive<f32>),
    Seconds(&'a mut f64, RangeInclusive<f64>),
    Whole(&'a mut u32, RangeInclusive<u32>),
    /// A list of exactly this many whole numbers
    Wholes(&'a mut [u32], RangeInclusive<u32>),
    /// Compass bearing and elevation in degrees
    Sun(&'a mut [f64; 2]),
    /// Text with only these `{placeholders}`
    Text(&'a mut String, &'static [&'static str]),
    /// A list of exactly this many texts
    Texts(&'a mut [String]),
    /// A list of any length
    TextList(&'a mut Vec<String>),
}

/// Walks a theme's tables and keys in the order of the file
pub(crate) trait Visitor {
    fn field(&mut self, key: &'static str, field: Field);

    /// A comment line above the next key
    fn note(&mut self, _line: &'static str) {}

    fn table(
        &mut self,
        name: &'static str,
        comment: &'static str,
        body: &mut dyn FnMut(&mut dyn Visitor),
    );
}

#[cfg(test)]
mod tests;
