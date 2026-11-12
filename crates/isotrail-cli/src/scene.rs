use clap::{ArgAction, Args};
use isotrail_render::{
    BASE_BLOCKS, BASE_BLOCKS_RANGE, Conditions, Edges, ForcedWeather, HiddenRoute, Look, Markers,
    OUTLINES_RANGE, PROP_DENSITY_RANGE, ROUTE_WIDTH_RANGE, RouteStyle, Terrain, Theme,
    WeatherEffects, colour_name, parse_colour, parse_count, parse_sun, parse_switch, parse_within,
    switch_name,
};

/// Which parts of the scene show, how it is lit and how the route is drawn,
/// for `render` and `embed`
#[derive(Debug, Args)]
pub struct SceneArgs {
    /// How many trees, bushes and boulders stand on the terrain, as a share
    /// of the usual number from 0 (none) to 2
    #[arg(long, value_name = "X", default_value_t = 1.0, value_parser = parse_density)]
    prop_density: f32,

    /// Paths and roads from the map, drawn as lighter ground
    #[arg(long, value_name = "on|off", default_value = "on", value_parser = parse_switch,
          action = ArgAction::Set)]
    paths: bool,

    /// Streams from the map
    #[arg(long, value_name = "on|off", default_value = "on", value_parser = parse_switch,
          action = ArgAction::Set)]
    streams: bool,

    /// Soil and rock layers on the slab's cut faces; off draws plain stone
    #[arg(long, value_name = "on|off", default_value = "on", value_parser = parse_switch,
          action = ArgAction::Set)]
    strata: bool,

    /// How far the slab's base sits below the lowest ground, in blocks, from
    /// 1 to 24
    #[arg(long, value_name = "N", default_value_t = BASE_BLOCKS, value_parser = parse_base_blocks)]
    base_blocks: u32,

    /// How strongly edges are outlined, from 0 (none) to 2
    #[arg(long, value_name = "X", default_value_t = 1.0, value_parser = parse_outlines)]
    outlines: f32,

    /// Shadows cast by the terrain and props
    #[arg(long, value_name = "on|off", default_value = "on", value_parser = parse_switch,
          action = ArgAction::Set)]
    shadows: bool,

    /// Stars and the moon after dusk
    #[arg(long, value_name = "on|off", default_value = "on", value_parser = parse_switch,
          action = ArgAction::Set)]
    night_sky: bool,

    /// Which of the weather's effects to draw: clouds (with the dimming under
    /// them), shade (cloud shadows on the ground), rain, snow and haze, or none
    #[arg(long, value_name = "LIST|none", default_value = "clouds,shade,rain,snow,haze",
          value_parser = parse_effects)]
    weather_effects: WeatherEffects,

    /// Draw this sky instead of the recorded weather
    #[arg(long, value_name = "clear|cloudy|overcast|rain|snow|fog", value_parser = parse_weather)]
    weather: Option<ForcedWeather>,

    /// Set the sun by hand, as its compass bearing and its elevation in
    /// degrees, such as 200,25
    #[arg(long, value_name = "AZIMUTH,ELEVATION", value_parser = parse_sun,
          allow_hyphen_values = true, conflicts_with = "time")]
    sun: Option<[f64; 2]>,

    /// The route's colour (default #d64834, or the theme's)
    #[arg(long, visible_alias = "route-color", value_name = "#RRGGBB",
          value_parser = parse_colour)]
    route_colour: Option<[u8; 3]>,

    /// The route's width in pixels of the image before it is enlarged, from 1
    /// to 4 (default 2, or the theme's)
    #[arg(long, value_name = "PX", value_parser = parse_route_width)]
    route_width: Option<u32>,

    /// How the parts of the route behind ridges show: faded and dashed,
    /// faded, like the rest, or not at all
    #[arg(long, value_name = "dashed|dim|solid|off", default_value = "dashed",
          value_parser = parse_hidden)]
    route_hidden: HiddenRoute,

    /// Which markers to draw: the start and finish flags and, in clips, the
    /// route's head, or none
    #[arg(long, value_name = "LIST|none", default_value = "start,finish,head",
          value_parser = parse_markers)]
    markers: Markers,
}

impl SceneArgs {
    pub fn terrain(&self, theme: &Theme) -> Terrain {
        Terrain {
            paths: self.paths,
            streams: self.streams,
            strata: self.strata,
            base_blocks: self.base_blocks,
            prop_density: self.prop_density,
            ..theme.terrain()
        }
    }

    pub fn conditions(&self, theme: &Theme) -> Conditions {
        Conditions {
            sun: self.sun,
            weather: self.weather,
            effects: self.weather_effects,
            ..theme.conditions()
        }
    }

    /// Whether the sun is set by hand, so a clip holds it
    pub fn holds_sun(&self) -> bool {
        self.sun.is_some()
    }

    /// `look` with these lights, outlines and route
    pub fn apply(&self, look: Look) -> Look {
        Look {
            edges: Edges::default().scaled(self.outlines),
            shadows: self.shadows,
            night_sky: self.night_sky,
            route: RouteStyle {
                colour: self.route_colour.unwrap_or(look.route.colour),
                width_px: self.route_width.unwrap_or(look.route.width_px),
                markers: self.markers,
                ..look.route
            }
            .hiding(self.route_hidden),
            ..look
        }
    }

    /// The choices that differ from the default, as the embed's attribute
    /// names and values; a route colour or width given counts when it differs
    /// from `theme`'s
    pub fn attributes(&self, theme: &RouteStyle) -> Vec<(&'static str, String)> {
        let usual = Terrain::default();
        let mut attributes = Vec::new();
        let mut switch = |name, on: bool, usual: bool| {
            if on != usual {
                attributes.push((name, switch_name(on).to_owned()));
            }
        };
        switch("paths", self.paths, usual.paths);
        switch("streams", self.streams, usual.streams);
        switch("strata", self.strata, usual.strata);
        switch("shadows", self.shadows, true);
        switch("night-sky", self.night_sky, true);
        let mut add = |name, differs: bool, value: String| {
            if differs {
                attributes.push((name, value));
            }
        };
        add(
            "prop-density",
            self.prop_density != usual.prop_density,
            self.prop_density.to_string(),
        );
        add(
            "base-blocks",
            self.base_blocks != usual.base_blocks,
            self.base_blocks.to_string(),
        );
        add("outlines", self.outlines != 1.0, self.outlines.to_string());
        add(
            "weather-effects",
            self.weather_effects != WeatherEffects::ALL,
            self.weather_effects.to_string(),
        );
        if let Some(weather) = self.weather {
            attributes.push(("weather", weather.to_string()));
        }
        if let Some([azimuth, elevation]) = self.sun {
            attributes.push(("sun", format!("{azimuth},{elevation}")));
        }
        if let Some(colour) = self.route_colour.filter(|&colour| colour != theme.colour) {
            attributes.push(("route-colour", colour_name(colour)));
        }
        if let Some(width) = self.route_width.filter(|&width| width != theme.width_px) {
            attributes.push(("route-width", width.to_string()));
        }
        let mut add = |name, differs: bool, value: String| {
            if differs {
                attributes.push((name, value));
            }
        };
        add(
            "route-hidden",
            self.route_hidden != HiddenRoute::default(),
            self.route_hidden.to_string(),
        );
        add(
            "markers",
            self.markers != Markers::ALL,
            self.markers.to_string(),
        );
        attributes
    }
}

fn parse_density(value: &str) -> Result<f32, String> {
    parse_within(value, &PROP_DENSITY_RANGE)
}

fn parse_base_blocks(value: &str) -> Result<u32, String> {
    parse_count(value, &BASE_BLOCKS_RANGE)
}

fn parse_outlines(value: &str) -> Result<f32, String> {
    parse_within(value, &OUTLINES_RANGE)
}

fn parse_route_width(value: &str) -> Result<u32, String> {
    parse_count(value, &ROUTE_WIDTH_RANGE)
}

fn parse_effects(value: &str) -> Result<WeatherEffects, String> {
    value.parse()
}

fn parse_weather(value: &str) -> Result<ForcedWeather, String> {
    value.parse()
}

fn parse_hidden(value: &str) -> Result<HiddenRoute, String> {
    value.parse()
}

fn parse_markers(value: &str) -> Result<Markers, String> {
    value.parse()
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    #[command(allow_negative_numbers = true)]
    struct Cli {
        #[command(flatten)]
        scene: SceneArgs,
        #[arg(long)]
        time: Option<String>,
    }

    fn parse(line: &[&str]) -> Result<SceneArgs, clap::Error> {
        Cli::try_parse_from(["isotrail"].iter().chain(line)).map(|cli| cli.scene)
    }

    #[test]
    fn defaults_change_nothing() {
        let args = parse(&[]).unwrap();
        let (theme, route) = (Theme::default(), RouteStyle::default());
        assert_eq!(args.terrain(&theme), Terrain::default());
        assert_eq!(args.conditions(&theme), Conditions::default());
        assert_eq!(args.apply(Look::default()), Look::default());
        assert!(args.attributes(&route).is_empty());
        let spelled_out = parse(&[
            "--prop-density",
            "1",
            "--paths",
            "on",
            "--base-blocks",
            "6",
            "--outlines",
            "1",
            "--weather-effects",
            "haze,snow,rain,shade,clouds",
            "--route-colour",
            "#D64834",
            "--route-width",
            "2",
            "--route-hidden",
            "dashed",
            "--markers",
            "head,finish,start",
        ])
        .unwrap();
        assert!(spelled_out.attributes(&route).is_empty());
        assert_eq!(spelled_out.apply(Look::default()), Look::default());
    }

    #[test]
    fn choices_become_attributes_in_a_fixed_order() {
        let args = parse(&[
            "--markers",
            "none",
            "--route-color",
            "#112233",
            "--route-width",
            "4",
            "--route-hidden",
            "off",
            "--sun",
            "200,-5",
            "--weather",
            "fog",
            "--weather-effects",
            "clouds,haze",
            "--outlines",
            "0",
            "--base-blocks",
            "12",
            "--prop-density",
            "0.5",
            "--night-sky",
            "off",
            "--shadows",
            "off",
            "--strata",
            "off",
            "--streams",
            "off",
            "--paths",
            "off",
        ])
        .unwrap();
        let names: Vec<String> = args
            .attributes(&RouteStyle::default())
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        assert_eq!(
            names,
            [
                "paths=off",
                "streams=off",
                "strata=off",
                "shadows=off",
                "night-sky=off",
                "prop-density=0.5",
                "base-blocks=12",
                "outlines=0",
                "weather-effects=clouds,haze",
                "weather=fog",
                "sun=200,-5",
                "route-colour=#112233",
                "route-width=4",
                "route-hidden=off",
                "markers=none",
            ]
        );
        let look = args.apply(Look::default());
        assert_eq!(look.edges.silhouette, 0.0);
        assert_eq!((look.route.hidden_opacity, look.route.width_px), (0.0, 4));
        assert!(!look.shadows && !look.night_sky);
        assert_eq!(args.terrain(&Theme::default()).base_blocks, 12);
    }

    #[test]
    fn route_flags_win_over_the_theme_and_the_theme_over_the_built_in_route() {
        let theme = Theme::parse("[route]\ncolour = \"#00ff00\"\nwidth = 3\n").unwrap();
        let themed = |line: &[&str]| {
            let args = parse(line).unwrap();
            let route = args.apply(theme.look()).route;
            let attributes = args.attributes(&theme.route);
            ((route.colour, route.width_px), attributes)
        };
        assert_eq!(themed(&[]), (([0, 255, 0], 3), Vec::new()));
        let (route, attributes) = themed(&["--route-colour", "#d64834", "--route-width", "3"]);
        assert_eq!(route, ([214, 72, 52], 3));
        assert_eq!(attributes, [("route-colour", "#d64834".to_owned())]);
    }

    #[test]
    fn bad_values_name_what_is_expected() {
        let message = |line: &[&str]| parse(line).unwrap_err().to_string();
        assert!(message(&["--paths", "maybe"]).contains("expected on or off"));
        assert!(message(&["--prop-density", "3"]).contains("from 0 to 2"));
        assert!(message(&["--base-blocks", "0"]).contains("from 1 to 24"));
        assert!(message(&["--route-width", "5"]).contains("from 1 to 4"));
        assert!(message(&["--route-colour", "red"]).contains("#d64834"));
        assert!(message(&["--weather", "sunny"]).contains("clear, cloudy"));
        assert!(message(&["--markers", "start,start"]).contains("twice"));
        assert!(message(&["--sun", "200"]).contains("AZIMUTH,ELEVATION"));
        let both = message(&["--sun", "200,20", "--time", "12:00"]);
        assert!(both.contains("cannot be used with"), "{both}");
    }
}
