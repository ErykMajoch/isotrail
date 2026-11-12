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
        "The layers on the slab's cut faces, and the water at its edges",
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
            v.note("Blocks per thousand in each layer that show a pebble or ore");
            v.field("dirt-specks-per-mille", Field::Whole(dirt, PER_MILLE));
            v.field("stone-specks-per-mille", Field::Whole(stone, PER_MILLE));
            v.field("deep-stone-specks-per-mille", Field::Whole(deep, PER_MILLE));
        },
    );
    v.table("props", "Trees, bushes and boulders", &mut |v| {
        let p = &mut theme.slab.props;
        v.field("needles", Field::Rgba(&mut p.needles));
        v.field("leaves", Field::Rgba(&mut p.leaves));
        v.field("bush", Field::Rgba(&mut p.bush));
        v.field("trunk", Field::Rgba(&mut p.trunk));
        v.field("boulder", Field::Rgba(&mut p.boulder));
        v.field("twigs", Field::Rgba(&mut p.twigs));
        v.field("bare-bush", Field::Rgba(&mut p.bare_bush));
        v.note("Pixels between the spots where a tree, bush or boulder may stand");
        v.field("spacing", Field::Number(&mut p.spacing_px, 1.0..=32.0));
        v.note("How far one strays from its spot, as a share of the spacing");
        v.field("jitter", Field::Number(&mut p.jitter, 0.0..=0.5));
    });
    v.table(
        "water",
        "Lake shores, the sea and its surf, glints on open water, and frozen lakes",
        &mut |v| {
            let w = &mut theme.slab.water;
            v.field("shore", Field::Rgba(&mut w.shore));
            v.field("sea", Field::Rgba(&mut w.sea));
            v.field("foam", Field::Rgba(&mut w.foam));
            v.field("glint", Field::Rgba(&mut w.glint));
            v.field("ice", Field::Rgba(&mut w.ice));
            v.field("ice-crack", Field::Rgba(&mut w.ice_crack));
            v.note("Cells of open water per thousand that glint");
            v.field(
                "glints-per-mille",
                Field::Whole(&mut w.glint_per_mille, PER_MILLE),
            );
            v.note("Cells of ice per thousand that show a crack");
            v.field(
                "cracks-per-mille",
                Field::Whole(&mut w.crack_per_mille, PER_MILLE),
            );
            v.note("Percent of foam on sea cells next to land, then on the ring beyond");
            v.field("foam-percent", Field::Wholes(&mut w.foam_percent, 0..=100));
        },
    );
    v.table("paths", "Paths and roads from the map", &mut |v| {
        let p = &mut theme.slab.paths;
        v.field("trodden", Field::Rgba(&mut p.trodden));
        v.field("paved", Field::Rgba(&mut p.paved));
        v.note("How far paths and roads turn the ground towards their colour");
        v.field("path-blend", Field::Number(&mut p.path_blend, SHARE));
        v.field("road-blend", Field::Number(&mut p.road_blend, SHARE));
    });
    v.table("season", "Colours of the seasons", &mut |v| {
        let s = &mut theme.slab.season;
        v.field("snow", Field::Rgba(&mut s.snow));
        v.note("Spring leaves and grass");
        v.field("fresh", Field::Rgba(&mut s.fresh));
        v.field("blossom", Field::Rgbas(&mut s.blossom));
        v.field("autumn", Field::Rgbas(&mut s.autumn));
        v.note("Fallen leaves on the ground in autumn");
        v.field("litter", Field::Rgba(&mut s.litter));
        v.note("Meadow flowers in spring, and how many cells per thousand have one");
        v.field("flowers", Field::Rgbas(&mut s.flowers));
        v.field(
            "flowers-per-mille",
            Field::Whole(&mut s.flowers_per_mille, PER_MILLE),
        );
        v.field("young-crop", Field::Rgba(&mut s.young_crop));
        v.note("Autumn grass and wetland");
        v.field("golden", Field::Rgba(&mut s.golden));
        v.note("Autumn fields");
        v.field("ploughed", Field::Rgba(&mut s.ploughed));
        v.note("Autumn moorland");
        v.field("bracken", Field::Rgba(&mut s.bracken));
        v.note("Winter grass, fields and wetland");
        v.field("dull", Field::Rgba(&mut s.dull));
        v.note("Winter moorland");
        v.field("dark-heath", Field::Rgba(&mut s.dark_heath));
    });
    v.table(
        "sky",
        "The daytime sky at the top and bottom of the image",
        &mut |v| {
            let [top, bottom] = &mut theme.light.ramp[4].sky;
            v.field("top", Field::Colour(top));
            v.field("bottom", Field::Colour(bottom));
            v.note("How much the sky is dithered between its colours");
            v.field("dither", Field::Number(&mut theme.sky_dither, 0.0..=0.5));
        },
    );
    v.table(
        "light",
        "Daylight, then the light as the sun goes down\n\
         Light colours are [red, green, blue] strengths, or #rrggbb",
        &mut |v| {
            let light = &mut theme.light;
            key(v, &mut light.ramp[4], false);
            v.note("Extra ambient on faces turned to the camera, as a share of the ambient");
            v.field("fill", Field::Number(&mut light.fill, 0.0..=2.0));
            v.note("Steps the sunlight is cut into");
            v.field("bands", Field::Whole(&mut light.bands, 2..=16));
            v.note("Strength of a full moon's light");
            v.field("moonlight", Field::Number(&mut light.moonlight, 0.0..=2.0));
            v.field("moon-colour", Field::Light(&mut light.moon_colour));
            v.note("The sun of tracks without times, as [bearing, elevation] in degrees");
            v.field("untimed-sun", Field::Sun(&mut light.untimed_sun));
            for (name, index, comment) in DUSK {
                v.table(name, comment, &mut |v| key(v, &mut light.ramp[index], true));
            }
        },
    );
    v.table("route", "The route line", &mut |v| {
        let r = &mut theme.route;
        v.field("colour", Field::Colour(&mut r.colour));
        v.note("Width in pixels");
        v.field("width", Field::Whole(&mut r.width_px, ROUTE_WIDTH_RANGE));
        v.note("How much a one-pixel border darkens the ground beside the line");
        v.field("outline", Field::Number(&mut r.outline, SHARE));
        v.note("How strongly the parts hidden behind ridges show through");
        v.field(
            "hidden-opacity",
            Field::Number(&mut r.hidden_opacity, SHARE),
        );
        v.note("Length of their dashes and gaps in pixels, 0 for a solid line");
        v.field(
            "hidden-dash",
            Field::Number(&mut r.hidden_dash_px, 0.0..=16.0),
        );
    });
    v.table("overlay", "Text and panels", &mut |v| {
        let o = &mut theme.overlay;
        v.field("text", Field::Colour(&mut o.text));
        v.note("Subtitles, dates, labels and the credit");
        v.field("subtext", Field::Colour(&mut o.subtext));
        v.field("shadow", Field::Colour(&mut o.shadow));
        v.field("panel-tint", Field::Colour(&mut o.panel_tint));
        v.note("How much of the tint covers the blurred image under a panel");
        v.field(
            "panel-tint-share",
            Field::Number(&mut o.panel_tint_share, SHARE),
        );
        v.note("How far the image under a panel is blurred, in pixels");
        v.field("panel-blur", Field::Whole(&mut o.panel_blur_px, 0..=8));
        v.note("The lit top edge of a panel");
        v.field("panel-edge", Field::Colour(&mut o.panel_edge));
        v.note("The part of the elevation profile not yet walked");
        v.field("profile-ahead", Field::Colour(&mut o.profile_ahead));
        v.note("Height of the elevation profile, in pixels of the large text");
        v.field("profile-rows", Field::Whole(&mut o.profile_rows, 2..=32));
        v.note("The largest share of a landscape image's width the text column takes");
        v.field(
            "column-share",
            Field::Number(&mut o.column_share, 0.1..=0.9),
        );
        v.note("A title too long for its lines splits at the first of these found");
        v.field("separators", Field::TextList(&mut o.separators));
    });
    v.table(
        "labels",
        "Every word the overlays print\n\
         Patterns fill in the {names} listed above them",
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
            v.note("The decimal separator");
            v.field("decimal", Field::Text(&mut l.decimal, &[]));
            v.note("A pace: {pace} and {unit}");
            v.field(
                "pace-value",
                Field::Text(&mut l.pace_value, &["pace", "unit"]),
            );
            v.note("Times under an hour: {m}");
            v.field("minutes", Field::Text(&mut l.minutes, &["m"]));
            v.note("Longer times: {h}, {m}, and {mm} for minutes with two digits");
            v.field("hours", Field::Text(&mut l.hours, &["h", "m", "mm"]));
            v.note("Elapsed times of a day or more: {d} and {h}");
            v.field("days", Field::Text(&mut l.days, &["d", "h"]));
            v.note("Dates: {weekday}, {weekday-short}, {day}, {dd} with two digits,");
            v.note("{month}, {month-short}, {mm} for its number, {year} and {yyyy}");
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
            v.note("Monday first");
            v.field("weekdays", Field::Texts(&mut l.weekdays));
            v.field("weekdays-short", Field::Texts(&mut l.weekdays_short));
        },
    );
    v.table("animation", "GIF and MP4 clips", &mut |v| {
        let p = &mut theme.pacing;
        v.note("Share of the drawing each night of a multi-day hike takes");
        v.field("night-share", Field::Number(&mut p.night_share, 0.0..=0.5));
        v.note("Share all the nights take together, at most");
        v.field(
            "max-nights-share",
            Field::Number(&mut p.max_nights_share, 0.0..=0.9),
        );
        v.note("The longest a stop lasts with time pacing, in seconds");
        v.field("pause-cut", Field::Seconds(&mut p.pause_s, 1.0..=3600.0));
    });
}

/// One key of the light's ramp, the day's with notes, a dusk key with its own sky
fn key(v: &mut dyn Visitor, key: &mut LightKey, dusk: bool) {
    if !dusk {
        v.note("The sun's colour and strength");
    }
    v.field("colour", Field::Light(&mut key.colour));
    v.field("intensity", Field::Number(&mut key.intensity, 0.0..=4.0));
    if !dusk {
        v.note("Light that reaches every face, and its tint");
    }
    v.field("ambient", Field::Number(&mut key.ambient, 0.0..=2.0));
    v.field("ambient-colour", Field::Light(&mut key.ambient_colour));
    if dusk {
        let [top, bottom] = &mut key.sky;
        v.field("sky-top", Field::Colour(top));
        v.field("sky-bottom", Field::Colour(bottom));
    }
}
