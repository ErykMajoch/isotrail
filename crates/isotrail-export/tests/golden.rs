use isotrail_bundle::{
    MoonSample, SceneBundle, Season, SunSample, Units, Weather, WeatherDataset, WeatherSample,
};
use isotrail_export::{
    ExportError, GifOptions, GifWriter, Plays, render_clip, render_still, upscale,
};
use isotrail_render::{
    Aim, Aspect, Background, Conditions, DEFAULT_PALETTE, DEFAULT_STATS, DateFormat, Diorama,
    Edges, Frame, GodRays, Gpu, HiddenRoute, Look, Markers, Mesher, Motion, OffscreenTarget, Orbit,
    OverlayStyle, OverlayText, Overlays, Pace, Palette, RouteStyle, Shaped, SmoothMesher, StatKind,
    SunState, Terrain, TerrainRenderer, Theme, Timeline, View, Viewpoints, VoxelMesher, Walk,
    WeatherEffects, When, draw_test_scene,
};
use isotrail_testkit::{
    Tolerance, assert_golden, decode_gif, differing_share, diorama_bundle, hike_bundle,
};

#[test]
fn test_scene_matches_golden() {
    let gpu = isotrail_testkit::gpu();
    let target = OffscreenTarget::new(&gpu, 90, 60);
    draw_test_scene(&gpu, &target);
    let frame = upscale(&target.read_back(&gpu).unwrap(), 3).unwrap();
    assert_golden("test-scene", frame.width(), frame.height(), frame.rgba());
}

fn assert_terrain_golden(name: &str, mesher: &dyn Mesher, look: &Look) {
    let gpu = isotrail_testkit::gpu();
    let framing = Aspect::Story.framing(6).unwrap();
    let bundle = diorama_bundle();
    let frame = render_still(
        &gpu,
        &bundle,
        mesher,
        &framing,
        &Aim::default(),
        &SunState::for_hike(&bundle),
        look,
    )
    .unwrap();
    assert_eq!((frame.width(), frame.height()), (180, 320));
    // The safe area's corners stay sky and its centre is terrain. The sky's
    // dither repeats every 4 pixels along a row, and the left edge is all sky
    let sky = |x: u32, y| frame.pixel(x, y) == frame.pixel(x % 4, y);
    assert!(sky(179, 0) && sky(179, 319));
    assert!(!sky(90, 166));
    assert_golden(name, frame.width(), frame.height(), frame.rgba());
}

/// Props, paths, streams, strata, shadows, outlines, the night sky, the
/// weather, the hidden route and the markers all switched off
#[test]
fn the_bare_scene_matches_golden() {
    let gpu = isotrail_testkit::gpu();
    let framing = Aspect::Story.framing(6).unwrap();
    let bundle = hike_bundle();
    let bare = Terrain {
        paths: false,
        streams: false,
        strata: false,
        base_blocks: 2,
        prop_density: 0.0,
        ..Terrain::default()
    };
    let look = Look {
        edges: Edges::default().scaled(0.0),
        shadows: false,
        night_sky: false,
        route: RouteStyle {
            markers: Markers::NONE,
            ..RouteStyle::default()
        }
        .hiding(HiddenRoute::Off),
        ..Look::default()
    };
    let conditions = Conditions {
        effects: WeatherEffects::NONE,
        ..Conditions::default()
    };
    let render = |terrain: Terrain, look: &Look, conditions: &Conditions| {
        render_still(
            &gpu,
            &bundle,
            &Shaped {
                mesher: VoxelMesher::default(),
                terrain,
            },
            &framing,
            &Aim::default(),
            &SunState::seen(&bundle, When::Start, conditions),
            look,
        )
        .unwrap()
    };
    let frame = render(bare, &look, &conditions);
    let usual = render(Terrain::default(), &Look::default(), &Conditions::default());
    assert!(differing_share(frame.rgba(), usual.rgba(), Tolerance::default()) > 0.05);
    assert_golden("scene-bare", frame.width(), frame.height(), frame.rgba());
}

#[test]
fn smooth_terrain_matches_golden() {
    assert_terrain_golden("terrain-smooth", &SmoothMesher, &Look::default());
}

#[test]
fn voxel_terrain_matches_golden() {
    assert_terrain_golden("terrain-voxel", &VoxelMesher::default(), &Look::default());
}

#[test]
fn other_palettes_match_goldens() {
    for name in Palette::builtin_names().filter(|&name| name != DEFAULT_PALETTE) {
        let look = Look {
            palette: Palette::builtin(name),
            ..Look::default()
        };
        assert_terrain_golden(&format!("palette-{name}"), &VoxelMesher::default(), &look);
    }
}

#[test]
fn low_suns_and_night_match_goldens() {
    let gpu = isotrail_testkit::gpu();
    let framing = Aspect::Story.framing(6).unwrap();
    let mut mean = Vec::new();
    // The full moon stands ahead and to the right of the view
    for (name, azimuth_deg, elevation_deg, moon_elevation_deg) in [
        ("terrain-voxel", 150.0, 40.0, -30.0),
        ("light-golden-hour", 290.0, 3.0, -30.0),
        ("light-night", 330.0, -20.0, -30.0),
        ("light-moonlit", 330.0, -20.0, 35.0),
    ] {
        let mut bundle = diorama_bundle();
        bundle.sun = vec![SunSample {
            time: 1_750_000_000,
            azimuth_deg,
            elevation_deg,
        }];
        bundle.moon = vec![MoonSample {
            time: 1_750_000_000,
            azimuth_deg: 50.0,
            elevation_deg: moon_elevation_deg,
            illuminated: 1.0,
        }];
        let frame = render_still(
            &gpu,
            &bundle,
            &VoxelMesher::default(),
            &framing,
            &Aim::default(),
            &SunState::for_hike(&bundle),
            &Look::default(),
        )
        .unwrap();
        mean.push(frame.rgba().iter().map(|&c| u64::from(c)).sum::<u64>());
        assert_golden(name, frame.width(), frame.height(), frame.rgba());
    }
    assert!(mean[0] > mean[1] && mean[1] * 2 > mean[2] * 3, "{mean:?}");
    assert!(mean[3] > mean[2], "{mean:?}");
}

const NOON: i64 = 1_750_000_000;

/// The same weather for two hours around a day-time sun
fn under(weather: WeatherSample) -> SceneBundle {
    let mut bundle = diorama_bundle();
    bundle.sun = vec![SunSample {
        time: NOON,
        azimuth_deg: 150.0,
        elevation_deg: 40.0,
    }];
    bundle.weather = Some(Weather {
        dataset: WeatherDataset::Archive,
        samples: [NOON - 1_800, NOON + 1_800]
            .map(|time| WeatherSample { time, ..weather })
            .to_vec(),
    });
    bundle
}

fn sky(code: u8, cover_pct: f32, low_pct: f32) -> WeatherSample {
    WeatherSample {
        time: 0,
        temperature_c: 12.0,
        weather_code: code,
        cloud_cover_pct: cover_pct,
        cloud_low_pct: low_pct,
        cloud_mid_pct: cover_pct / 2.0,
        cloud_high_pct: 0.0,
        precipitation_mm: 0.0,
        rain_mm: 0.0,
        snowfall_cm: 0.0,
        snow_depth_m: 0.0,
        wind_speed_m_s: 8.0,
        wind_direction_deg: 250.0,
    }
}

fn rain() -> WeatherSample {
    WeatherSample {
        precipitation_mm: 3.0,
        rain_mm: 3.0,
        ..sky(63, 100.0, 90.0)
    }
}

#[test]
fn weather_matches_goldens() {
    let gpu = isotrail_testkit::gpu();
    let framing = Aspect::Story.framing(6).unwrap();
    let snow = WeatherSample {
        temperature_c: -3.0,
        precipitation_mm: 2.0 / 0.7,
        snowfall_cm: 2.0,
        ..sky(73, 100.0, 90.0)
    };
    let mut means = Vec::new();
    for (name, weather) in [
        ("weather-clear", sky(1, 15.0, 5.0)),
        ("weather-partly-cloudy", sky(2, 45.0, 30.0)),
        ("weather-overcast", sky(3, 100.0, 100.0)),
        ("weather-rain", rain()),
        ("weather-snow", snow),
        ("weather-fog", sky(45, 100.0, 100.0)),
    ] {
        let bundle = under(weather);
        let frame = render_still(
            &gpu,
            &bundle,
            &VoxelMesher::default(),
            &framing,
            &Aim::default(),
            &SunState::for_hike(&bundle),
            &Look::default(),
        )
        .unwrap();
        means.push(mean(&frame));
        assert_golden(name, frame.width(), frame.height(), frame.rgba());
    }
    // White clouds brighten a blue sky, and grey ones darken it
    let [clear, partly, overcast, rain, snow, fog] = means[..] else {
        unreachable!()
    };
    assert!(
        partly > clear && overcast < clear && fog < clear,
        "{means:?}"
    );
    assert!(rain != overcast && snow > rain, "{means:?}");
}

#[test]
fn seasons_match_goldens() {
    let gpu = isotrail_testkit::gpu();
    let framing = Aspect::Story.framing(6).unwrap();
    let summer = diorama_bundle();
    let terrain = &summer.terrain;
    let low = (0..terrain.heights.len())
        .map(|cell| terrain.height_m(cell))
        .fold(f32::MAX, f32::min);
    let day = |leaf_day| Season {
        leaf_day: Some(leaf_day),
        ..Season::default()
    };
    let winter = Season {
        snow_line_m: Some(low - 1.0),
        ice_line_m: Some(low - 1.0),
        ..day(15)
    };
    let mut means = Vec::new();
    for (name, season) in [
        ("season-summer", Season::default()),
        ("season-spring", day(125)),
        ("season-autumn", day(290)),
        ("season-winter", winter),
    ] {
        let bundle = SceneBundle {
            season,
            ..summer.clone()
        };
        let frame = render_still(
            &gpu,
            &bundle,
            &VoxelMesher::default(),
            &framing,
            &Aim::default(),
            &SunState::for_hike(&bundle),
            &Look::default(),
        )
        .unwrap();
        means.push(mean(&frame));
        if name != "season-summer" {
            assert_golden(name, frame.width(), frame.height(), frame.rgba());
        }
    }
    let [summer, spring, autumn, winter] = means[..] else {
        unreachable!()
    };
    assert!(spring != summer && autumn != summer, "{means:?}");
    assert!(winter > summer, "{means:?}");
}

#[test]
fn the_sea_stays_open_and_dark_in_winter() {
    let gpu = isotrail_testkit::gpu();
    let framing = Aspect::Story.framing(6).unwrap();
    let mut bundle = diorama_bundle();
    let terrain = &bundle.terrain;
    let low = (0..terrain.heights.len())
        .map(|cell| terrain.height_m(cell))
        .fold(f32::MAX, f32::min);
    bundle.season = Season {
        leaf_day: Some(15),
        snow_line_m: Some(low - 1.0),
        ice_line_m: Some(low - 1.0),
    };
    let render = |bundle: &SceneBundle| {
        render_still(
            &gpu,
            bundle,
            &VoxelMesher::default(),
            &framing,
            &Aim::default(),
            &SunState::for_hike(bundle),
            &Look::default(),
        )
        .unwrap()
    };
    let frozen_lake = mean(&render(&bundle));
    bundle.water[0].sea = true;
    let sea = render(&bundle);
    assert!(mean(&sea) < frozen_lake, "{} {frozen_lake}", mean(&sea));
    assert_golden("sea-winter", sea.width(), sea.height(), sea.rgba());
}

/// Sunlight breaking through a partly cloudy sky falls in beams on the
/// ground, and the weather decides whether they show
#[test]
fn god_rays_light_the_ground_through_gaps_in_the_cloud() {
    let gpu = isotrail_testkit::gpu();
    let framing = Aspect::Story.framing(6).unwrap();
    let render = |weather: WeatherSample, god_rays: GodRays| {
        let mut bundle = under(weather);
        bundle.sun[0].elevation_deg = 15.0;
        let look = Look {
            god_rays,
            ..Look::default()
        };
        render_still(
            &gpu,
            &bundle,
            &VoxelMesher::default(),
            &framing,
            &Aim::default(),
            &SunState::for_hike(&bundle),
            &look,
        )
        .unwrap()
    };
    let partly = sky(2, 45.0, 30.0);
    let beams = render(partly, GodRays::Weather);
    assert_golden("god-rays", beams.width(), beams.height(), beams.rgba());
    assert_ne!(beams.rgba(), render(partly, GodRays::Off).rgba());
    let overcast = sky(3, 100.0, 100.0);
    assert_eq!(
        render(overcast, GodRays::Weather).rgba(),
        render(overcast, GodRays::Off).rgba(),
        "no sun breaks through an overcast sky"
    );
    assert_ne!(
        render(overcast, GodRays::On).rgba(),
        render(overcast, GodRays::Off).rgba()
    );
}

/// Hollows and the feet of slopes lose some of the sky's light
#[test]
fn occlusion_darkens_hollows() {
    let gpu = isotrail_testkit::gpu();
    let framing = Aspect::Story.framing(6).unwrap();
    let bundle = diorama_bundle();
    let render = |occlusion: f32| {
        let look = Look {
            occlusion,
            ..Look::default()
        };
        render_still(
            &gpu,
            &bundle,
            &VoxelMesher::default(),
            &framing,
            &Aim::default(),
            &SunState::for_hike(&bundle),
            &look,
        )
        .unwrap()
    };
    let (open, occluded) = (render(0.0), render(1.0));
    assert!(
        mean(&occluded) < mean(&open),
        "{} {}",
        mean(&occluded),
        mean(&open)
    );
}

/// Rain falls on as the clock runs, the same way every time
#[test]
fn rain_moves_with_the_clock_and_repeats() {
    let gpu = isotrail_testkit::gpu();
    let bundle = under(rain());
    let framing = Aspect::Square.framing(6).unwrap();
    let renderer = TerrainRenderer::new(&gpu);
    let sun = SunState::for_hike(&bundle);
    assert!(sun.weather.is_some_and(|weather| weather.moves()));
    let mut diorama = Diorama::new(
        &gpu,
        &renderer,
        &bundle,
        VoxelMesher::default().mesh(&bundle),
        &framing,
        &Viewpoints {
            aim: Aim::default(),
            yaw_offsets_deg: &[0.0],
        },
        sun,
    )
    .unwrap();
    let target = OffscreenTarget::new(&gpu, framing.width, framing.height);
    let mut at = |time_s| {
        diorama.set_time(time_s);
        diorama.draw(
            &gpu,
            &renderer,
            &target,
            &View::default(),
            1.0,
            &Look::default(),
        );
        target.read_back(&gpu).unwrap()
    };
    let (first, later, again) = (at(0.0), at(0.25), at(0.0));
    assert_eq!(first.rgba(), again.rgba());
    let moved = first
        .rgba()
        .chunks(4)
        .zip(later.rgba().chunks(4))
        .filter(|(a, b)| a != b)
        .count();
    let pixels = (framing.width * framing.height) as usize;
    assert!(moved > pixels / 100 && moved < pixels / 3, "{moved}");
}

fn mean(frame: &Frame) -> f64 {
    frame.rgba().iter().map(|&c| f64::from(c)).sum::<f64>() / frame.rgba().len() as f64
}

/// A hike from an afternoon into the night: the light follows the clock at
/// the route's head through the clip, unless the clip holds one sun
#[test]
fn clips_follow_the_sun_unless_one_is_held() {
    let gpu = isotrail_testkit::gpu();
    let mut bundle = hike_bundle();
    let start = 1_750_000_000;
    for (i, point) in bundle.route.points.iter_mut().enumerate() {
        point.time = Some(start + i as i64 * 600);
    }
    let end = bundle.route.points.last().unwrap().time.unwrap();
    bundle.sun = [(start, 250.0, 35.0), (end, 320.0, -25.0)]
        .map(|(time, azimuth_deg, elevation_deg)| SunSample {
            time,
            azimuth_deg,
            elevation_deg,
        })
        .to_vec();
    bundle.moon = [start, end]
        .map(|time| MoonSample {
            time,
            azimuth_deg: 90.0,
            elevation_deg: -30.0,
            illuminated: 0.2,
        })
        .to_vec();
    let framing = Aspect::Square.framing(6).unwrap();
    let motion = Motion {
        timeline: Timeline {
            duration_s: 1.0,
            fps: 4,
            step_fps: None,
            ..Timeline::default()
        },
        ..Motion::default()
    };
    let clip = |motion: &Motion| {
        let mut means = Vec::new();
        render_clip(
            &gpu,
            &bundle,
            &VoxelMesher::default(),
            &framing,
            &Look::default(),
            motion,
            |_, frame| -> Result<(), ExportError> {
                means.push(mean(frame));
                Ok(())
            },
        )
        .unwrap();
        means
    };
    let following = clip(&motion);
    assert!(following.windows(2).all(|w| w[0] > w[1]), "{following:?}");
    assert!(following[0] > following[3] * 1.5, "{following:?}");
    let held = clip(&Motion {
        sun: Some(SunState::for_hike(&bundle)),
        ..motion
    });
    assert_eq!(held[0], following[0]);
    assert!(held[3] > following[3] * 1.5, "{held:?}");
}

#[test]
fn overlays_on_the_diorama_match_golden() {
    let gpu = isotrail_testkit::gpu();
    let mut bundle = diorama_bundle();
    bundle.meta.title = "Zażółć gęślą jaźń | Łańcuch".into();
    bundle.meta.local_date = Some("2025-06-14".into());
    let stats = &mut bundle.meta.stats;
    (stats.distance_m, stats.gain_m) = (7_420.0, Some(612.0));
    (stats.moving_s, stats.max_ele_m) = (Some(9_000), Some(842.0));
    let framing = Aspect::Story.framing(6).unwrap();
    let text = OverlayText::from_bundle(&bundle, Units::Metric, &DEFAULT_STATS, DateFormat::Long);
    assert_eq!(text.date.as_deref(), Some("Saturday 14 June 2025"));
    let overlays = Overlays::fit(&text, &OverlayStyle::default(), &framing)
        .unwrap()
        .overlays;
    let look = Look::default();
    let mut frame = render_still(
        &gpu,
        &bundle,
        &VoxelMesher::default(),
        &overlays.diorama_framing(),
        &Aim::default(),
        &SunState::for_hike(&bundle),
        &look,
    )
    .unwrap();
    overlays.draw(&mut frame, look.palette.as_ref(), 1.0);
    assert_golden(
        "overlay-diorama",
        frame.width(),
        frame.height(),
        frame.rgba(),
    );
}

/// Every colour group, the light, the route and the overlays themed loudly,
/// with the overlays in Polish
#[test]
fn a_loud_theme_matches_golden() {
    let gpu = isotrail_testkit::gpu();
    let theme = Theme::parse(include_str!("../../../fixtures/themes/loud.toml")).unwrap();
    let mut bundle = diorama_bundle();
    bundle.meta.title = "Wąwóz Ślężny | Ścieżka nad Łąkami".into();
    bundle.meta.local_date = Some("2025-06-14".into());
    let stats = &mut bundle.meta.stats;
    (stats.distance_m, stats.gain_m) = (7_420.0, Some(612.0));
    (stats.moving_s, stats.max_ele_m) = (Some(9_000), Some(842.0));
    let framing = Aspect::Story.framing(6).unwrap();
    let kinds = [
        StatKind::Distance,
        StatKind::Gain,
        StatKind::Time,
        StatKind::Speed,
    ];
    let draw = |theme: &Theme| {
        let text = OverlayText::from_bundle_with(
            &bundle,
            Units::Metric,
            &kinds,
            DateFormat::Long,
            &theme.labels,
        );
        let overlays = Overlays::fit(&text, &theme.overlay_style(), &framing)
            .unwrap()
            .overlays;
        let look = theme.look();
        let mut frame = render_still(
            &gpu,
            &bundle,
            &Shaped {
                mesher: VoxelMesher::default(),
                terrain: theme.terrain(),
            },
            &overlays.diorama_framing(),
            &Aim::default(),
            &SunState::seen(&bundle, When::Start, &theme.conditions()),
            &look,
        )
        .unwrap();
        overlays.draw(&mut frame, look.palette.as_ref(), 1.0);
        (text, frame)
    };
    let (text, frame) = draw(&theme);
    assert_eq!(text.date.as_deref(), Some("sobota, 14 czerwca 2025"));
    let words: Vec<(&str, &str)> = text
        .stats
        .iter()
        .map(|stat| (stat.value.as_str(), stat.label.as_str()))
        .collect();
    assert_eq!(
        words,
        [
            ("7,4 km", "dystans"),
            ("612 m", "podejścia"),
            ("2 godz. 30 min", "czas"),
            ("3,0 km/h", "prędkość"),
        ]
    );
    let (_, plain) = draw(&Theme::default());
    let changed = differing_share(frame.rgba(), plain.rgba(), Tolerance::default());
    assert!(changed > 0.3, "{changed}");
    assert_golden("theme-loud", frame.width(), frame.height(), frame.rgba());
}

fn render_hike(gpu: &Gpu, aspect: Aspect, look: &Look) -> Frame {
    render_hike_at(gpu, aspect, 6, look)
}

fn render_hike_at(gpu: &Gpu, aspect: Aspect, scale: u32, look: &Look) -> Frame {
    let bundle = hike_bundle();
    let framing = aspect.framing(scale).unwrap();
    let text = OverlayText::from_bundle(&bundle, Units::Metric, &DEFAULT_STATS, DateFormat::Long);
    let overlays = Overlays::fit(&text, &OverlayStyle::default(), &framing)
        .unwrap()
        .overlays;
    let mut frame = render_still(
        gpu,
        &bundle,
        &VoxelMesher::default(),
        &overlays.diorama_framing(),
        &Aim::default(),
        &SunState::for_hike(&bundle),
        look,
    )
    .unwrap();
    overlays.draw(&mut frame, look.palette.as_ref(), 1.0);
    frame
}

#[test]
fn every_preset_matches_its_golden() {
    let gpu = isotrail_testkit::gpu();
    for aspect in Aspect::ALL {
        let frame = render_hike(&gpu, aspect, &Look::default());
        let (width, height) = aspect.output_size();
        assert_eq!((frame.width(), frame.height()), (width / 6, height / 6));
        assert_golden(
            &format!("preset-{}", aspect.name()),
            frame.width(),
            frame.height(),
            frame.rgba(),
        );
    }
}

/// The coarsest scale, where the overlays give up parts to fit
#[test]
fn every_preset_at_scale_10_matches_its_golden() {
    let gpu = isotrail_testkit::gpu();
    for aspect in Aspect::ALL {
        let frame = render_hike_at(&gpu, aspect, 10, &Look::default());
        let (width, height) = aspect.output_size();
        assert_eq!((frame.width(), frame.height()), (width / 10, height / 10));
        assert_golden(
            &format!("preset-{}-10", aspect.name()),
            frame.width(),
            frame.height(),
            frame.rgba(),
        );
    }
}

#[test]
fn backgrounds_replace_only_the_sky() {
    let gpu = isotrail_testkit::gpu();
    let sky = render_hike(&gpu, Aspect::Square, &Look::default());
    let render = |background| {
        let look = Look {
            background,
            ..Look::default()
        };
        render_hike(&gpu, Aspect::Square, &look)
    };
    let transparent = render(Background::Transparent);
    let colour = [12, 200, 99];
    let flat = render(Background::Colour(colour));
    // The corners are sky, and the diorama sits above the stats panel
    for (x, y) in [(0, 0), (179, 0), (0, 179), (179, 179)] {
        assert_eq!(transparent.pixel(x, y), [0, 0, 0, 0], "({x}, {y})");
        assert_eq!(flat.pixel(x, y), [12, 200, 99, 255], "({x}, {y})");
    }
    assert_ne!(sky.pixel(90, 52), sky.pixel(90 % 4, 52));
    assert_eq!(transparent.pixel(90, 52), sky.pixel(90, 52));
    assert_eq!(flat.pixel(90, 52), sky.pixel(90, 52));
}

/// A one-second, seven-frame square clip of the synthetic hike, paced by its
/// times, with the stats counting up, decoded back from the GIF
#[test]
fn animation_frames_match_goldens() {
    let gpu = isotrail_testkit::gpu();
    let mut bundle = hike_bundle();
    // Ten minutes between points, with an hour's stop at the fourth
    for (i, point) in bundle.route.points.iter_mut().enumerate() {
        let stop = if i >= 4 { 3_600 } else { 0 };
        point.time = Some(1_750_000_000 + i as i64 * 600 + stop);
    }
    let framing = Aspect::Square.framing(6).unwrap();
    let units = Units::Metric;
    let text = OverlayText::from_bundle(&bundle, units, &DEFAULT_STATS, DateFormat::Long);
    let overlays = Overlays::fit(&text, &OverlayStyle::default(), &framing)
        .unwrap()
        .overlays;
    let look = Look::default();
    let motion = Motion {
        timeline: Timeline {
            duration_s: 1.0,
            fps: 7,
            step_fps: None,
            ..Timeline::default()
        },
        pace: Pace::Time,
        ..Motion::default()
    };
    let walk = Walk::new(&bundle);
    assert!(walk.is_timed());
    let diorama = overlays.diorama_framing();
    let options = GifOptions {
        scale: 1,
        fps: 7,
        plays: Plays::Forever,
    };
    let mut gif =
        GifWriter::new(Vec::new(), diorama.width, diorama.height, &look, options).unwrap();
    render_clip(
        &gpu,
        &bundle,
        &VoxelMesher::default(),
        &diorama,
        &look,
        &motion,
        |moment, frame| -> Result<(), ExportError> {
            let walked = walk.stats(moment.progress);
            let stats: Vec<_> = DEFAULT_STATS
                .iter()
                .filter_map(|kind| kind.stat_walked(&bundle.meta.stats, &walked, units))
                .collect();
            let mut frame = frame.clone();
            overlays
                .with_stats(&stats)
                .draw(&mut frame, look.palette.as_ref(), moment.progress);
            gif.push(&frame)
        },
    )
    .unwrap();
    let frames = decode_gif(&gif.finish().unwrap());
    assert_eq!(frames.len(), 7);
    assert_eq!(
        frames.iter().map(|f| u32::from(f.delay_cs)).sum::<u32>(),
        100
    );
    // The last frame is in the hold
    for (name, i) in [("first", 0), ("middle", 3), ("last", 6)] {
        let frame = &frames[i];
        assert_eq!((frame.width, frame.height), (180, 180));
        assert_golden(
            &format!("animation-{name}"),
            frame.width,
            frame.height,
            &frame.rgba,
        );
    }
}

/// However short or choppy, a clip ends on the still: the route finished,
/// the camera at rest and, with the sun held as `--time` holds it, the
/// same light
#[test]
fn every_clip_ends_on_the_still() {
    let gpu = isotrail_testkit::gpu();
    let mut bundle = hike_bundle();
    for (i, point) in bundle.route.points.iter_mut().enumerate() {
        point.time = Some(1_750_000_000 + i as i64 * 600);
    }
    let framing = Aspect::Square.framing(6).unwrap();
    let look = Look::default();
    let sun = SunState::for_hike(&bundle);
    let still = render_still(
        &gpu,
        &bundle,
        &VoxelMesher::default(),
        &framing,
        &Aim::default(),
        &sun,
        &look,
    )
    .unwrap();
    for (duration_s, fps, step_fps) in [(5.0, 10, Some(1)), (1.0, 1, None), (2.0, 3, Some(2))] {
        let motion = Motion {
            timeline: Timeline {
                duration_s,
                fps,
                step_fps,
                ..Timeline::default()
            },
            orbit: Orbit { sweep_deg: 0.0 },
            sun: Some(sun),
            ..Motion::default()
        };
        let mut last = None;
        render_clip(
            &gpu,
            &bundle,
            &VoxelMesher::default(),
            &framing,
            &look,
            &motion,
            |moment, frame| -> Result<(), ExportError> {
                last = Some((moment.progress, frame.clone()));
                Ok(())
            },
        )
        .unwrap();
        let (progress, last) = last.unwrap();
        assert_eq!(progress, 1.0);
        let share = differing_share(last.rgba(), still.rgba(), Tolerance::default());
        assert!(
            share <= Tolerance::default().max_diff_fraction,
            "{duration_s}s at {fps} fps, {step_fps:?}: {:.2}% differs",
            share * 100.0
        );
    }
}
