use super::{Field, Theme, Visitor};
use crate::labels::DATE_PLACEHOLDERS;
use crate::{LightKey, ROUTE_WIDTH_RANGE};

const LANDCOVER: [&str; 13] = [
    "water",
    "snow",
    "rock",
    "scree",
    "grassland",
    "heath",
    "scrub",
    "coniferous",
    "broadleaf",
    "mixed-forest",
    "wetland",
    "farmland",
    "built-up",
];

/// The ramp's keys below the day's, from the highest sun down, with the
/// index of each in the ramp
const DUSK: [(&str, usize, &str); 4] = [
    ("golden", 3, "The sun 6° up"),
    ("sunrise", 2, "The sun on the horizon"),
    ("twilight", 1, "The sun 6° down"),
    ("night", 0, "The sun 18° down and lower"),
];

const SHARE: std::ops::RangeInclusive<f32> = 0.0..=1.0;
const PER_MILLE: std::ops::RangeInclusive<u32> = 0..=1000;

/// Every table and key, in the order a printed theme has them
pub(super) fn visit(theme: &mut Theme, v: &mut dyn Visitor) {
    v.table(
        "landcover",
        "Ground colours, one per kind of cover",
        &mut |v| {
            for (&key, colour) in LANDCOVER.iter().zip(&mut theme.slab.landcover) {
                v.field(key, Field::Rgba(colour));
            }
        },
    );
    v.table(
        "terrain",
        "How bright the walls of steps and slopes are, as a share of the ground on top",
        &mut |v| {
            v.field(
                "side-shade",
                Field::Number(&mut theme.slab.side_shade, SHARE),
            );
        },
    );
    v.table(
        "strata",
        "The layers on the slab's cut faces, the water at its edges, and how many blocks\n\
         per thousand show a pebble or ore",
        &mut |v| {
            let s = &mut theme.slab.strata;
            v.field("dirt", Field::Rgba(&mut s.dirt));
            v.field("pebble", Field::Rgba(&mut s.pebble));
            v.field("stone", Field::Rgba(&mut s.stone));
            v.field("deep-stone", Field::Rgba(&mut s.deep_stone));
            v.field("copper", Field::Rgba(&mut s.copper));
            v.field("gold", Field::Rgba(&mut s.gold));
            v.field("crystal", Field::Rgba(&mut s.crystal));
            v.field("water", Field::Rgba(&mut s.water));
            v.field("sea-water", Field::Rgba(&mut s.sea_water));
            let [dirt, stone, deep] = &mut s.specks_per_mille;
            v.field("dirt-specks-per-mille", Field::Whole(dirt, PER_MILLE));
            v.field("stone-specks-per-mille", Field::Whole(stone, PER_MILLE));
            v.field("deep-stone-specks-per-mille", Field::Whole(deep, PER_MILLE));
        },
    );
    v.table(
        "props",
        "Trees, bushes and boulders; spacing is in pixels between spots where one may\n\
         stand, and jitter how far it strays from its spot, as a share of the spacing",
        &mut |v| {
            let p = &mut theme.slab.props;
            v.field("needles", Field::Rgba(&mut p.needles));
            v.field("leaves", Field::Rgba(&mut p.leaves));
            v.field("bush", Field::Rgba(&mut p.bush));
            v.field("trunk", Field::Rgba(&mut p.trunk));
            v.field("boulder", Field::Rgba(&mut p.boulder));
            v.field("twigs", Field::Rgba(&mut p.twigs));
            v.field("bare-bush", Field::Rgba(&mut p.bare_bush));
            v.field("spacing", Field::Number(&mut p.spacing_px, 1.0..=32.0));
            v.field("jitter", Field::Number(&mut p.jitter, 0.0..=0.5));
        },
    );
    v.table(
        "water",
        "Lake shores, the sea and its surf, glints on open water and frozen lakes; the\n\
         foam shares are for the sea's cells next to land and the ring beyond them",
        &mut |v| {
            let w = &mut theme.slab.water;
            v.field("shore", Field::Rgba(&mut w.shore));
            v.field("sea", Field::Rgba(&mut w.sea));
            v.field("foam", Field::Rgba(&mut w.foam));
            v.field("glint", Field::Rgba(&mut w.glint));
            v.field("ice", Field::Rgba(&mut w.ice));
            v.field("ice-crack", Field::Rgba(&mut w.ice_crack));
            v.field(
                "glints-per-mille",
                Field::Whole(&mut w.glint_per_mille, PER_MILLE),
            );
            v.field(
                "cracks-per-mille",
                Field::Whole(&mut w.crack_per_mille, PER_MILLE),
            );
            v.field("foam-percent", Field::Wholes(&mut w.foam_percent, 0..=100));
        },
    );
    v.table(
        "paths",
        "Paths and roads from the map, and how far they turn the ground towards their colour",
        &mut |v| {
            let p = &mut theme.slab.paths;
            v.field("trodden", Field::Rgba(&mut p.trodden));
            v.field("paved", Field::Rgba(&mut p.paved));
            v.field("path-blend", Field::Number(&mut p.path_blend, SHARE));
            v.field("road-blend", Field::Number(&mut p.road_blend, SHARE));
        },
    );
    v.table(
        "season",
        "Colours of the seasons: snow, spring leaves and blossom, autumn leaves and fallen\n\
         litter, meadow flowers, young and ripe crops, ploughed fields, bracken, and the\n\
         dull grass and dark moorland of winter",
        &mut |v| {
            let s = &mut theme.slab.season;
            v.field("snow", Field::Rgba(&mut s.snow));
            v.field("fresh", Field::Rgba(&mut s.fresh));
            v.field("blossom", Field::Rgbas(&mut s.blossom));
            v.field("autumn", Field::Rgbas(&mut s.autumn));
            v.field("litter", Field::Rgba(&mut s.litter));
            v.field("flowers", Field::Rgbas(&mut s.flowers));
            v.field(
                "flowers-per-mille",
                Field::Whole(&mut s.flowers_per_mille, PER_MILLE),
            );
            v.field("young-crop", Field::Rgba(&mut s.young_crop));
            v.field("golden", Field::Rgba(&mut s.golden));
            v.field("ploughed", Field::Rgba(&mut s.ploughed));
            v.field("bracken", Field::Rgba(&mut s.bracken));
            v.field("dull", Field::Rgba(&mut s.dull));
            v.field("dark-heath", Field::Rgba(&mut s.dark_heath));
        },
    );
    v.table(
        "sky",
        "The daytime sky at the top and bottom of the image, and how much it is dithered",
        &mut |v| {
            let [top, bottom] = &mut theme.light.ramp[4].sky;
            v.field("top", Field::Colour(top));
            v.field("bottom", Field::Colour(bottom));
            v.field("dither", Field::Number(&mut theme.sky_dither, 0.0..=0.5));
        },
    );
    v.table(
        "light",
        "Daylight: the sun's colour and strength, the ambient light and its tint, extra\n\
         ambient on faces turned to the camera, the steps the light is cut into, the\n\
         moon's light, and the sun of tracks without times as [bearing, elevation];\n\
         light colours are [red, green, blue] strengths, or #rrggbb",
        &mut |v| {
            let light = &mut theme.light;
            key(v, &mut light.ramp[4], false);
            v.field("fill", Field::Number(&mut light.fill, 0.0..=2.0));
            v.field("bands", Field::Whole(&mut light.bands, 2..=16));
            v.field("moonlight", Field::Number(&mut light.moonlight, 0.0..=2.0));
            v.field("moon-colour", Field::Light(&mut light.moon_colour));
            v.field("untimed-sun", Field::Sun(&mut light.untimed_sun));
            for (name, index, comment) in DUSK {
                v.table(name, comment, &mut |v| key(v, &mut light.ramp[index], true));
            }
        },
    );
    v.table(
        "route",
        "The route's colour and width in pixels, the dark border round it, and how the\n\
         parts hidden behind ridges fade and how long their dashes are",
        &mut |v| {
            let r = &mut theme.route;
            v.field("colour", Field::Colour(&mut r.colour));
            v.field("width", Field::Whole(&mut r.width_px, ROUTE_WIDTH_RANGE));
            v.field("outline", Field::Number(&mut r.outline, SHARE));
            v.field(
                "hidden-opacity",
                Field::Number(&mut r.hidden_opacity, SHARE),
            );
            v.field(
                "hidden-dash",
                Field::Number(&mut r.hidden_dash_px, 0.0..=16.0),
            );
        },
    );
    v.table(
        "overlay",
        "Text and panels: the frame under a panel is blurred and mixed with its tint;\n\
         profile-ahead is the part of the profile not yet walked; a title too long for\n\
         its lines splits at the first separator found; column-share is the widest the\n\
         text column of a landscape image gets",
        &mut |v| {
            let o = &mut theme.overlay;
            v.field("text", Field::Colour(&mut o.text));
            v.field("subtext", Field::Colour(&mut o.subtext));
            v.field("shadow", Field::Colour(&mut o.shadow));
            v.field("panel-tint", Field::Colour(&mut o.panel_tint));
            v.field(
                "panel-tint-share",
                Field::Number(&mut o.panel_tint_share, SHARE),
            );
            v.field("panel-blur", Field::Whole(&mut o.panel_blur_px, 0..=8));
            v.field("panel-edge", Field::Colour(&mut o.panel_edge));
            v.field("profile-ahead", Field::Colour(&mut o.profile_ahead));
            v.field("profile-rows", Field::Whole(&mut o.profile_rows, 2..=32));
            v.field(
                "column-share",
                Field::Number(&mut o.column_share, 0.1..=0.9),
            );
            v.field("separators", Field::TextList(&mut o.separators));
        },
    );
    v.table(
        "labels",
        "Every word the overlays print. Patterns fill in {names}: pace-value {pace} {unit};\n\
         minutes {m}; hours {h} {m} {mm}; days {d} {h}; dates {weekday} {weekday-short}\n\
         {day} {dd} {month} {month-short} {mm} {year} {yyyy}. Weekdays start on Monday",
        &mut |v| {
            let l = &mut theme.labels;
            v.field("distance", Field::Text(&mut l.distance, &[]));
            v.field("gain", Field::Text(&mut l.gain, &[]));
            v.field("loss", Field::Text(&mut l.loss, &[]));
            v.field("time", Field::Text(&mut l.time, &[]));
            v.field("elapsed", Field::Text(&mut l.elapsed, &[]));
            v.field("high-point", Field::Text(&mut l.high_point, &[]));
            v.field("low-point", Field::Text(&mut l.low_point, &[]));
            v.field("speed", Field::Text(&mut l.speed, &[]));
            v.field("pace", Field::Text(&mut l.pace, &[]));
            v.field("km", Field::Text(&mut l.km, &[]));
            v.field("mi", Field::Text(&mut l.mi, &[]));
            v.field("m", Field::Text(&mut l.m, &[]));
            v.field("ft", Field::Text(&mut l.ft, &[]));
            v.field("kmh", Field::Text(&mut l.kmh, &[]));
            v.field("mph", Field::Text(&mut l.mph, &[]));
            v.field("decimal", Field::Text(&mut l.decimal, &[]));
            v.field(
                "pace-value",
                Field::Text(&mut l.pace_value, &["pace", "unit"]),
            );
            v.field("minutes", Field::Text(&mut l.minutes, &["m"]));
            v.field("hours", Field::Text(&mut l.hours, &["h", "m", "mm"]));
            v.field("days", Field::Text(&mut l.days, &["d", "h"]));
            v.field(
                "date-long",
                Field::Text(&mut l.date_long, &DATE_PLACEHOLDERS),
            );
            v.field(
                "date-short",
                Field::Text(&mut l.date_short, &DATE_PLACEHOLDERS),
            );
            v.field("date-iso", Field::Text(&mut l.date_iso, &DATE_PLACEHOLDERS));
            v.field("months", Field::Texts(&mut l.months));
            v.field("months-short", Field::Texts(&mut l.months_short));
            v.field("weekdays", Field::Texts(&mut l.weekdays));
            v.field("weekdays-short", Field::Texts(&mut l.weekdays_short));
        },
    );
    v.table(
        "animation",
        "Clips: the share of the drawing each night of a multi-day hike takes, and all of\n\
         them at most; and the most of a stop that time pacing keeps, in seconds",
        &mut |v| {
            let p = &mut theme.pacing;
            v.field("night-share", Field::Number(&mut p.night_share, 0.0..=0.5));
            v.field(
                "max-nights-share",
                Field::Number(&mut p.max_nights_share, 0.0..=0.9),
            );
            v.field("pause-cut", Field::Seconds(&mut p.pause_s, 1.0..=3600.0));
        },
    );
}

/// One key of the light's ramp, with its sky if `sky`
fn key(v: &mut dyn Visitor, key: &mut LightKey, sky: bool) {
    v.field("colour", Field::Light(&mut key.colour));
    v.field("intensity", Field::Number(&mut key.intensity, 0.0..=4.0));
    v.field("ambient", Field::Number(&mut key.ambient, 0.0..=2.0));
    v.field("ambient-colour", Field::Light(&mut key.ambient_colour));
    if sky {
        let [top, bottom] = &mut key.sky;
        v.field("sky-top", Field::Colour(top));
        v.field("sky-bottom", Field::Colour(bottom));
    }
}
