use chrono::{NaiveDate, NaiveTime};
use isotrail_bundle::SceneBundle;
use isotrail_render::{
    Aim, BASE_BLOCKS_RANGE, BLOCK_HEIGHT_RANGE, Background, Conditions, Edges, ForcedWeather,
    GodRays, HiddenRoute, Look, MAX_ZOOM, Markers, Mesher, MesherKind, OCCLUSION_RANGE,
    OUTLINES_RANGE, PITCH_RANGE, PROP_DENSITY_RANGE, Palette, ROUTE_WIDTH_RANGE, RouteStyle,
    SOFT_SHADOWS_RANGE, SunState, TURN_RANGE, Terrain, View, VoxelMesher, WeatherEffects, When,
    ZOOM_RANGE, parse_colour, parse_count, parse_sun, parse_switch, parse_within,
};
use js_sys::{Array, Function, Object, Reflect};
use wasm_bindgen::{JsCast, JsValue};

/// The look a page chose, from the object `Viewer.load` and `setOptions`
/// take; a value it can't use is left at the default with a console warning
pub struct Options {
    pub look: Look,
    mesher: MesherKind,
    block_height: f32,
    terrain: Terrain,
    /// A hand-set sun, a chosen sky and the weather's effects drawn
    pub conditions: Conditions,
    /// The sun held at `time` or set by hand, through playback and scrubbing
    pub held_sun: Option<SunState>,
    /// The starting view's turn, pitch and zoom
    aim: Aim,
}

impl Options {
    pub fn read(options: &JsValue, bundle: &SceneBundle) -> Options {
        let defaults = Look::default();
        let look = Look {
            palette: read(options, "palette", palette).unwrap_or(defaults.palette.clone()),
            background: read(options, "background", str::parse::<Background>)
                .unwrap_or(defaults.background),
            occlusion: read(options, "occlusion", |text| {
                parse_within(text, &OCCLUSION_RANGE)
            })
            .unwrap_or(defaults.occlusion),
            soft_shadows: read(options, "softShadows", |text| {
                parse_within(text, &SOFT_SHADOWS_RANGE)
            })
            .unwrap_or(defaults.soft_shadows),
            god_rays: read(options, "godRays", str::parse::<GodRays>).unwrap_or(defaults.god_rays),
            edges: read(options, "outlines", |text| {
                parse_within(text, &OUTLINES_RANGE)
            })
            .map_or(defaults.edges, |outlines| Edges::default().scaled(outlines)),
            shadows: read(options, "shadows", parse_switch).unwrap_or(defaults.shadows),
            night_sky: read(options, "nightSky", parse_switch).unwrap_or(defaults.night_sky),
            route: RouteStyle {
                colour: read(options, "routeColour", parse_colour).unwrap_or(defaults.route.colour),
                width_px: read(options, "routeWidth", |text| {
                    parse_count(text, &ROUTE_WIDTH_RANGE)
                })
                .unwrap_or(defaults.route.width_px),
                markers: read(options, "markers", str::parse::<Markers>)
                    .unwrap_or(defaults.route.markers),
                ..defaults.route
            }
            .hiding(read(options, "routeHidden", str::parse::<HiddenRoute>).unwrap_or_default()),
            ..defaults
        };
        let usual = Terrain::default();
        let terrain = Terrain {
            paths: read(options, "paths", parse_switch).unwrap_or(usual.paths),
            streams: read(options, "streams", parse_switch).unwrap_or(usual.streams),
            strata: read(options, "strata", parse_switch).unwrap_or(usual.strata),
            base_blocks: read(options, "baseBlocks", |text| {
                parse_count(text, &BASE_BLOCKS_RANGE)
            })
            .unwrap_or(usual.base_blocks),
            prop_density: read(options, "propDensity", |text| {
                parse_within(text, &PROP_DENSITY_RANGE)
            })
            .unwrap_or(usual.prop_density),
            ..usual
        };
        let conditions = Conditions {
            sun: read(options, "sun", parse_sun),
            weather: read(options, "weather", str::parse::<ForcedWeather>),
            effects: read(options, "weatherEffects", str::parse::<WeatherEffects>)
                .unwrap_or_default(),
            ..Conditions::default()
        };
        let held_sun = match conditions.sun {
            Some(_) => Some(SunState::seen(bundle, When::Start, &conditions)),
            None => read(options, "time", |text| sun_at(bundle, text, &conditions)),
        };
        Options {
            look,
            mesher: read(options, "mesher", str::parse::<MesherKind>).unwrap_or_default(),
            block_height: read(options, "blockHeight", |text| {
                parse_within(text, &BLOCK_HEIGHT_RANGE)
            })
            .unwrap_or(VoxelMesher::default().block_height),
            terrain,
            conditions,
            held_sun,
            aim: Aim {
                turn_deg: read(options, "turn", |text| parse_within(text, &TURN_RANGE))
                    .unwrap_or(0.0),
                pitch_deg: read(options, "pitch", |text| parse_within(text, &PITCH_RANGE))
                    .unwrap_or(Aim::default().pitch_deg),
                zoom: read(options, "zoom", |text| parse_within(text, &ZOOM_RANGE)).unwrap_or(1.0),
                ..Aim::default()
            },
        }
    }

    /// The view the diorama is fitted for: zoom below 1 leaves more room
    /// round the slab, while zoom above it starts the view magnified
    pub fn fitted_aim(&self) -> Aim {
        Aim {
            zoom: self.aim.zoom.min(1.0),
            ..self.aim
        }
    }

    /// Where the view starts, and returns to on reset
    pub fn start_view(&self) -> View {
        View {
            zoom: self.aim.zoom.clamp(1.0, MAX_ZOOM),
            ..View::default()
        }
    }

    /// Whether the diorama must be fitted again to go from `self` to `other`
    pub fn refits(&self, other: &Options) -> bool {
        self.aim != other.aim
    }

    pub fn mesher(&self) -> Box<dyn Mesher> {
        self.mesher.mesher(Some(self.block_height), self.terrain)
    }

    /// Whether the terrain must be built again to go from `self` to `other`
    pub fn remeshes(&self, other: &Options) -> bool {
        self.mesher != other.mesher
            || (self.mesher == MesherKind::Voxel && self.block_height != other.block_height)
            || self.terrain != other.terrain
    }
}

/// The option `key` parsed, or none when it is missing, empty or bad
fn read<T>(options: &JsValue, key: &str, parse: impl Fn(&str) -> Result<T, String>) -> Option<T> {
    if !options.is_object() {
        return None;
    }
    let value = Reflect::get(options, &key.into()).ok()?;
    let text = value
        .as_string()
        .or_else(|| value.as_f64().map(|number| number.to_string()))?;
    if text.trim().is_empty() {
        return None;
    }
    match parse(&text) {
        Ok(parsed) => Some(parsed),
        Err(error) => {
            let shown: String = text.chars().take(40).collect();
            web_sys::console::warn_1(
                &format!(
                    "<isotrail-hike> ignored {key} {shown:?}, so it uses the default: {error}"
                )
                .into(),
            );
            None
        }
    }
}

/// A built-in palette by name, none, or the text of a Lospec .hex file
fn palette(text: &str) -> Result<Option<Palette>, String> {
    if text.trim().eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    if let Some(palette) = Palette::builtin(text.trim()) {
        return Ok(Some(palette));
    }
    Palette::parse_hex(text).map(Some).map_err(|_| {
        let names: Vec<&str> = Palette::builtin_names().collect();
        format!(
            "expected {}, none or the address of a .hex file",
            names.join(", ")
        )
    })
}

/// The sun at a local time on the hike's start date, in its time zone
fn sun_at(bundle: &SceneBundle, text: &str, conditions: &Conditions) -> Result<SunState, String> {
    let time = NaiveTime::parse_from_str(text.trim(), "%H:%M")
        .map_err(|_| "expected a local time such as 20:30".to_owned())?;
    let date = bundle
        .meta
        .local_date
        .as_deref()
        .and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
        .ok_or("the hike has no times, to know which day to light")?;
    let zone = bundle
        .meta
        .timezone
        .as_deref()
        .ok_or("the hike has no time zone")?;
    let local = date.and_time(time).and_utc().timestamp();
    // The zone's offset where the time falls, twice to settle on the right
    // side of a clock change
    let mut utc = local - offset(zone, local)?;
    utc = local - offset(zone, utc)?;
    if utc + offset(zone, utc)? != local {
        return Err(format!(
            "{date} {text} does not exist in {zone} (the clocks skip it)"
        ));
    }
    Ok(SunState::seen(bundle, When::Instant(utc), conditions))
}

/// Seconds `zone` is ahead of UTC at `utc`, from the browser's time zone data
fn offset(zone: &str, utc: i64) -> Result<i64, String> {
    let error = |_: JsValue| format!("the browser doesn't know the time zone {zone}");
    let intl = Reflect::get(&js_sys::global(), &"Intl".into()).map_err(error)?;
    let format = Reflect::get(&intl, &"DateTimeFormat".into())
        .map_err(error)?
        .dyn_into::<Function>()
        .map_err(error)?;
    let settings = Object::new();
    for (key, value) in [
        ("timeZone", zone),
        ("hourCycle", "h23"),
        ("year", "numeric"),
        ("month", "numeric"),
        ("day", "numeric"),
        ("hour", "numeric"),
        ("minute", "numeric"),
        ("second", "numeric"),
    ] {
        Reflect::set(&settings, &key.into(), &value.into()).map_err(error)?;
    }
    let formatter =
        Reflect::construct(&format, &Array::of2(&"en-US".into(), &settings)).map_err(error)?;
    let parts = Reflect::get(&formatter, &"formatToParts".into())
        .map_err(error)?
        .dyn_into::<Function>()
        .map_err(error)?
        .call1(
            &formatter,
            &js_sys::Date::new(&((utc * 1000) as f64).into()),
        )
        .map_err(error)?
        .dyn_into::<Array>()
        .map_err(error)?;
    let mut fields = [0i64; 6];
    for part in parts.iter() {
        let field = |key: &str| {
            Reflect::get(&part, &key.into())
                .ok()
                .and_then(|value| value.as_string())
        };
        let index = match field("type").as_deref() {
            Some("year") => 0,
            Some("month") => 1,
            Some("day") => 2,
            Some("hour") => 3,
            Some("minute") => 4,
            Some("second") => 5,
            _ => continue,
        };
        fields[index] = field("value")
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| format!("could not read the time in {zone}"))?;
    }
    let [year, month, day, hour, minute, second] = fields;
    let local = NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)
        .and_then(|date| date.and_hms_opt((hour % 24) as u32, minute as u32, second as u32))
        .ok_or_else(|| format!("could not read the time in {zone}"))?
        .and_utc()
        .timestamp();
    Ok(local - utc)
}
