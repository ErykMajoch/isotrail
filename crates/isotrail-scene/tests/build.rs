use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use isotrail_bundle::SceneBundle;
use isotrail_geo::{Answer, Fetcher, OVERPASS_URL, RecordedFetcher, Request, Retry};
use isotrail_scene::{Builder, Failure, Kind, Outcome, VK_MAPS_URL, Wait, waits};

/// 2026-10-01, long after the recorded hike
const LATER: i64 = 1_790_812_800;
const OPTIONS: &str = r#"{"detail": 32}"#;
const EUROPE: &str = "https://ovp.dresel.systems/api/interpreter";
/// One Overpass server, named in the options
const ONE_SERVER: &str =
    r#"{"detail": 32, "overpass-url": "https://overpass.example.org/api/interpreter"}"#;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn snowdon() -> Vec<u8> {
    fs::read(fixtures().join("gpx/snowdon.gpx")).unwrap()
}

/// The recorded answer, as a page would fetch it. Every Overpass server
/// answers as the recorded one did
fn recorded(request: &Request) -> Answer {
    let fetched = match &request.body {
        Some(body) => RecordedFetcher::new(fixtures().join("osm")).post(OVERPASS_URL, body),
        None => RecordedFetcher::new(fixtures().join("dem"))
            .get(&request.url)
            .or_else(|_| RecordedFetcher::new(fixtures().join("weather")).get(&request.url)),
    };
    match fetched {
        Ok(body) => Answer::Body(body),
        Err(_) => Answer::Status {
            status: 404,
            body: Vec::new(),
        },
    }
}

fn busy() -> Answer {
    Answer::Status {
        status: 502,
        body: Vec::new(),
    }
}

fn builder(options: &str) -> Builder {
    Builder::new(&snowdon(), "snowdon.gpx", options, LATER).unwrap()
}

/// Holds each request's answer, recorded unless `answer` gives another
fn fetch(builder: &mut Builder, wanted: Vec<Request>, answer: impl Fn(&Request) -> Option<Answer>) {
    for request in wanted {
        let held = answer(&request).unwrap_or_else(|| recorded(&request));
        builder.hold(request.key(), held);
    }
}

/// Builds until the scene is made or the build fails, answering as `answer`
/// does, and gives back every pause asked for
fn run(
    builder: &mut Builder,
    answer: impl Fn(&Request) -> Option<Answer>,
) -> (Result<Vec<u8>, Failure>, Vec<Wait>) {
    let wanted = builder.requests();
    fetch(builder, wanted, &answer);
    let mut paused = Vec::new();
    for _ in 0..12 {
        match builder.build() {
            Ok(Outcome::Scene(bytes)) => return (Ok(bytes), paused),
            Ok(Outcome::Missing) => {}
            Ok(Outcome::Wait(wait)) => paused.push(wait),
            Err(failure) => return (Err(failure), paused),
        }
        let wanted = builder.missing().to_vec();
        fetch(builder, wanted, &answer);
    }
    panic!("the build went on asking");
}

fn is_map_data(request: &Request) -> bool {
    request.body.is_some()
}

/// Where the scene says its map data came from
fn map_data_from(scene: &[u8]) -> String {
    SceneBundle::from_bytes(scene)
        .unwrap()
        .meta
        .provenance
        .map_data
}

/// The scene with the server it names left out, for scenes built from the
/// same answers given by different servers
fn without_server(scene: &[u8]) -> SceneBundle {
    let mut bundle = SceneBundle::from_bytes(scene).unwrap();
    bundle.meta.provenance.map_data.clear();
    bundle
}

#[test]
fn recorded_answers_build_the_scene_in_one_pass() {
    let mut builder = builder(OPTIONS);
    let kinds: Vec<Kind> = builder
        .requests()
        .iter()
        .map(|request| builder.kind(request))
        .collect();
    assert!(kinds.contains(&Kind::Elevation));
    assert!(kinds.contains(&Kind::MapData));
    assert!(kinds.contains(&Kind::Weather));
    assert_eq!(builder.servers(), [EUROPE, VK_MAPS_URL, OVERPASS_URL]);
    let (built, paused) = run(&mut builder, |_| None);
    let built = built.unwrap();
    assert!(isotrail_bundle::is_bundle(&built));
    assert_eq!(
        map_data_from(&built),
        "OpenStreetMap via https://ovp.dresel.systems"
    );
    assert!(paused.is_empty());
    assert!(builder.requests().is_empty());
}

#[test]
fn the_page_waits_twice_for_busy_overpass_servers_then_gives_up() {
    assert_eq!(
        waits(Kind::MapData, Retry::Busy),
        [Duration::from_secs(5), Duration::from_secs(15)]
    );
    for (options, servers, said) in [(OPTIONS, 3, "servers are"), (ONE_SERVER, 1, "server is")] {
        let mut builder = builder(options);
        let asked = std::cell::RefCell::new(Vec::new());
        let (built, paused) = run(&mut builder, |request| {
            is_map_data(request).then(|| {
                asked.borrow_mut().push(request.url.clone());
                busy()
            })
        });
        let steps: Vec<(Kind, usize, usize, u64)> = paused
            .iter()
            .map(|wait| (wait.kind, wait.attempt, wait.attempts, wait.delay.as_secs()))
            .collect();
        assert_eq!(steps, [(Kind::MapData, 2, 3, 5), (Kind::MapData, 3, 3, 15)]);
        // Each attempt asks every server, the first again after each pause
        let asked = asked.into_inner();
        assert_eq!(asked.len(), 3 * servers);
        assert_eq!(asked[..servers], asked[servers..2 * servers]);
        let failure = built.unwrap_err();
        assert_eq!(failure.reason, "busy");
        assert_eq!(
            failure.message,
            format!(
                "OpenStreetMap's Overpass {said} busy or failing (HTTP 502); \
                 try again later, or build the diorama without lakes, woods and paths"
            )
        );
    }
}

#[test]
fn a_busy_server_hands_over_to_the_next_without_a_pause() {
    let (expected, _) = run(&mut builder(OPTIONS), |_| None);
    let mut builder = builder(OPTIONS);
    let (built, paused) = run(&mut builder, |request| {
        (is_map_data(request) && request.url == EUROPE).then(busy)
    });
    assert!(paused.is_empty());
    let built = built.unwrap();
    assert_eq!(
        map_data_from(&built),
        "OpenStreetMap via https://maps.mail.ru"
    );
    assert_eq!(without_server(&built), without_server(&expected.unwrap()));
    assert_eq!(builder.server(), VK_MAPS_URL);
}

#[test]
fn any_failure_hands_over_and_an_empty_answer_does_too_but_from_the_last() {
    let empty = || Answer::Body(br#"{"elements":[]}"#.to_vec());
    for (answer, from) in [
        (
            Answer::Status {
                status: 403,
                body: Vec::new(),
            },
            "https://maps.mail.ru",
        ),
        (empty(), "https://maps.mail.ru"),
    ] {
        let mut builder = builder(OPTIONS);
        let (built, _) = run(&mut builder, |request| {
            (is_map_data(request) && request.url == EUROPE).then(|| answer.clone())
        });
        assert_eq!(
            map_data_from(&built.unwrap()),
            format!("OpenStreetMap via {from}")
        );
    }
    // A slab with nothing mapped is a slab with nothing mapped
    let mut builder = builder(OPTIONS);
    let (built, paused) = run(&mut builder, |request| is_map_data(request).then(empty));
    assert!(paused.is_empty());
    assert_eq!(
        map_data_from(&built.unwrap()),
        "OpenStreetMap via https://overpass-api.de"
    );
}

#[test]
fn an_answer_kept_from_another_server_serves_a_second_build() {
    let mut builder = builder(OPTIONS);
    let wanted = builder.requests();
    let map = wanted.iter().find(|r| is_map_data(r)).unwrap().clone();
    assert_eq!(map.url, EUROPE);
    let aliases = builder.aliases(&map);
    let vk = Request::post(VK_MAPS_URL, map.body.clone().unwrap());
    assert_eq!(
        aliases,
        [
            vk.key(),
            Request::post(OVERPASS_URL, map.body.clone().unwrap()).key()
        ]
    );
    // The page found VK's answer in its cache, under VK's key
    for request in wanted {
        let key = if is_map_data(&request) {
            vk.key()
        } else {
            request.key()
        };
        builder.hold(key, recorded(&request));
    }
    assert_eq!(builder.server(), VK_MAPS_URL);
    let Ok(Outcome::Scene(scene)) = builder.build() else {
        panic!("not built in one pass");
    };
    assert_eq!(
        map_data_from(&scene),
        "OpenStreetMap via https://maps.mail.ru"
    );
}

#[test]
fn a_page_may_name_its_own_servers_but_the_options_come_first() {
    let mut builder = builder(OPTIONS);
    builder.use_servers(vec![
        "https://a.example/api".into(),
        "https://b.example/api".into(),
    ]);
    assert_eq!(
        builder.servers(),
        ["https://a.example/api", "https://b.example/api"]
    );
    assert!(
        builder
            .requests()
            .iter()
            .any(|r| r.url == "https://a.example/api")
    );
    let mut typed = self::builder(ONE_SERVER);
    typed.use_servers(vec!["https://a.example/api".into()]);
    assert_eq!(
        typed.servers(),
        ["https://overpass.example.org/api/interpreter"]
    );
}

fn skip(builder: &mut Builder, names: &[&str]) -> Result<(), Failure> {
    let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
    builder.skip_servers(&names)
}

#[test]
fn a_page_may_leave_servers_out_by_name() {
    let mut builder = builder(OPTIONS);
    skip(&mut builder, &["vk-maps"]).unwrap();
    assert_eq!(builder.servers(), [EUROPE, OVERPASS_URL]);
    let mut builder = self::builder(OPTIONS);
    skip(&mut builder, &["dresel", "overpass-api"]).unwrap();
    assert_eq!(builder.servers(), [VK_MAPS_URL]);
    assert!(builder.requests().iter().any(|r| r.url == VK_MAPS_URL));
    let (built, _) = run(&mut builder, |_| None);
    assert_eq!(
        map_data_from(&built.unwrap()),
        "OpenStreetMap via https://maps.mail.ru"
    );
    // Options that name their own server keep it
    let mut typed = self::builder(ONE_SERVER);
    skip(&mut typed, &["dresel", "vk-maps", "overpass-api"]).unwrap();
    assert_eq!(
        typed.servers(),
        ["https://overpass.example.org/api/interpreter"]
    );
}

#[test]
fn leaving_out_every_server_that_can_answer_is_refused_before_any_request() {
    let mut builder = builder(OPTIONS);
    let failure = skip(&mut builder, &["dresel", "vk-maps", "overpass-api"]).unwrap_err();
    assert_eq!(failure.reason, "no-server");
    assert_eq!(
        failure.message,
        "none of the Overpass servers left can fetch the map data here; allow another, \
         or build the diorama without lakes, woods and paths"
    );
    // Outside dresel's regions, it alone can't serve
    let tangier = r#"<gpx version="1.1"><trk><trkseg>
        <trkpt lat="35.77" lon="-5.80"></trkpt><trkpt lat="35.76" lon="-5.82"></trkpt>
        </trkseg></trk></gpx>"#;
    let mut builder = Builder::new(tangier.as_bytes(), "t.gpx", OPTIONS, LATER).unwrap();
    assert_eq!(builder.servers(), [VK_MAPS_URL, OVERPASS_URL]);
    let failure = skip(&mut builder, &["vk-maps", "overpass-api"]).unwrap_err();
    assert_eq!(failure.reason, "no-server");
    // Without map data, no server is needed
    let mut builder = self::builder(r#"{"detail": 32, "no-map-data": true}"#);
    skip(&mut builder, &["dresel", "vk-maps", "overpass-api"]).unwrap();
    let (built, _) = run(&mut builder, |_| None);
    assert!(built.is_ok());
}

#[test]
fn an_unknown_server_name_is_a_usage_mistake() {
    let failure = skip(&mut builder(OPTIONS), &["vk"]).unwrap_err();
    assert_eq!(failure.reason, "usage");
    assert_eq!(
        failure.message,
        "there's no Overpass server called vk to leave out; use dresel, vk-maps or overpass-api"
    );
}

#[test]
fn every_failed_answer_is_named_for_the_page_to_drop() {
    let mut builder = builder(OPTIONS);
    let wanted = builder.requests();
    let europe = wanted.iter().find(|r| is_map_data(r)).unwrap().key();
    fetch(&mut builder, wanted, |request| {
        is_map_data(request).then(busy)
    });
    assert!(matches!(builder.build(), Ok(Outcome::Missing)));
    assert_eq!(builder.dropped(), [europe]);
    assert_eq!(builder.missing()[0].url, VK_MAPS_URL);
}

#[test]
fn a_busy_overpass_that_recovers_gives_the_same_scene() {
    let (expected, _) = run(&mut builder(ONE_SERVER), |_| None);
    let mut builder = builder(ONE_SERVER);
    let wanted = builder.requests();
    fetch(&mut builder, wanted, |request| {
        is_map_data(request).then(busy)
    });
    let Ok(Outcome::Wait(wait)) = builder.build() else {
        panic!("no pause");
    };
    assert_eq!((wait.kind, wait.retry), (Kind::MapData, Retry::Busy));
    assert_eq!(builder.missing().len(), 1);
    let (built, _) = run(&mut builder, |_| None);
    assert_eq!(built.unwrap(), expected.unwrap());
}

#[test]
fn an_overpass_remark_about_overload_is_a_busy_server_too() {
    let remark = br#"{"elements":[],"remark":"runtime error: Query timed out"}"#;
    let mut builder = builder(OPTIONS);
    let (built, paused) = run(&mut builder, |request| {
        is_map_data(request).then(|| Answer::Body(remark.to_vec()))
    });
    assert_eq!(paused.len(), 2);
    let failure = built.unwrap_err();
    assert_eq!(failure.reason, "busy");
    assert!(failure.message.contains("(runtime error: Query timed out)"));
}

#[test]
fn a_busy_elevation_server_is_asked_as_the_command_line_asks_it() {
    assert_eq!(
        waits(Kind::Elevation, Retry::Busy),
        [1, 2, 4].map(Duration::from_secs)
    );
    assert_eq!(
        waits(Kind::MapData, Retry::Unreachable),
        [2, 4].map(Duration::from_secs)
    );
    let mut builder = builder(OPTIONS);
    let (built, paused) = run(&mut builder, |request| {
        (builder_kind(request) == Kind::Elevation).then(|| Answer::Status {
            status: 503,
            body: Vec::new(),
        })
    });
    assert_eq!(paused.len(), 3);
    assert!(paused.iter().all(|wait| wait.kind == Kind::Elevation));
    let failure = built.unwrap_err();
    assert_eq!(failure.reason, "busy");
    assert_eq!(
        failure.message,
        "the elevation server https://s3.amazonaws.com is busy or failing (HTTP 503); try again later"
    );
}

fn builder_kind(request: &Request) -> Kind {
    if request.body.is_some() {
        Kind::MapData
    } else if request.url.contains("terrarium") {
        Kind::Elevation
    } else {
        Kind::Weather
    }
}

#[test]
fn weather_that_stays_busy_is_left_out_after_its_tries() {
    let mut builder = builder(OPTIONS);
    let (built, paused) = run(&mut builder, |request| {
        (builder_kind(request) == Kind::Weather).then(|| Answer::Status {
            status: 503,
            body: Vec::new(),
        })
    });
    assert!(built.is_ok());
    assert_eq!(paused.len(), 3);
    assert!(paused.iter().all(|wait| wait.kind == Kind::Weather));
    assert!(builder.warnings().contains(&"weather".to_owned()));
}

/// A straight walk north, `km` long
fn walk(km: f64) -> Vec<u8> {
    let points: String = (0..=10)
        .map(|i| {
            let lat = 52.0 + km / 111.2 * f64::from(i) / 10.0;
            format!(
                r#"<trkpt lat="{lat:.6}" lon="-1.5"><ele>200</ele><time>2026-06-01T{:02}:00:00Z</time></trkpt>"#,
                8 + i
            )
        })
        .collect();
    format!(
        r#"<?xml version="1.0"?><gpx version="1.1" creator="test"><trk><name>Long walk</name><trkseg>{points}</trkseg></trk></gpx>"#
    )
    .into_bytes()
}

#[test]
fn a_slab_too_large_for_map_data_is_refused_before_any_request() {
    let failure = Builder::new(&walk(150.0), "long.gpx", "{}", LATER)
        .err()
        .expect("refused");
    assert_eq!(failure.reason, "too-large");
    assert!(
        failure
            .message
            .starts_with("the land around this route is ")
            && failure
                .message
                .contains("km, too large to fetch map data for in a browser; "),
        "{}",
        failure.message
    );
    let without =
        Builder::new(&walk(150.0), "long.gpx", r#"{"no-map-data": true}"#, LATER).unwrap();
    assert!(
        without
            .requests()
            .iter()
            .all(|request| request.body.is_none())
    );
    assert!(Builder::new(&walk(20.0), "short.gpx", "{}", LATER).is_ok());
}

#[test]
fn files_that_aren_t_gpx_are_named() {
    let failure = |bytes: &[u8], name: &str| {
        Builder::new(bytes, name, "{}", LATER)
            .err()
            .expect("refused")
    };
    let png = failure(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR", "photo.png");
    assert_eq!(
        (png.reason, png.message.as_str()),
        ("input", "photo.png is an image, not a GPX file")
    );
    let video = failure(b"\0\0\0\x18ftypisom\0\0", "");
    assert_eq!(video.message, "the file is a video, not a GPX file");
    let scene = failure(b"ITB1\0\x06rest", "hike.gpx");
    assert_eq!(
        scene.message,
        "hike.gpx is already a scene file, not a GPX file"
    );
    let text = failure(b"just some notes", "notes.gpx");
    assert_eq!(text.reason, "input");
    assert!(
        text.message.starts_with("can't read notes.gpx: "),
        "{}",
        text.message
    );
}

#[test]
fn a_route_on_one_spot_or_trimmed_away_says_what_to_change() {
    let spot = r#"<gpx version="1.1"><trk><trkseg>
        <trkpt lat="52.0" lon="-1.5"></trkpt><trkpt lat="52.0" lon="-1.5"></trkpt>
        </trkseg></trk></gpx>"#;
    let failure = Builder::new(spot.as_bytes(), "spot.gpx", r#"{"padding": "0,0"}"#, LATER)
        .err()
        .expect("refused");
    assert_eq!(failure.reason, "no-ground");
    assert_eq!(
        failure.message,
        "the route stays on one spot, so a padding of 0,0 leaves no ground to draw; \
         give it some metres, such as 15,500"
    );
    let failure = Builder::new(
        &snowdon(),
        "snowdon.gpx",
        r#"{"trim-start": "50km"}"#,
        LATER,
    )
    .err()
    .expect("refused");
    assert_eq!(failure.reason, "usage");
    assert!(
        failure
            .message
            .starts_with("trimming 50km from the start leaves none of the route, which is "),
        "{}",
        failure.message
    );
    assert!(failure.message.ends_with(" long; trim less"));
}

#[test]
fn options_the_tool_would_refuse_are_refused_the_same_way() {
    let failure = Builder::new(&snowdon(), "snowdon.gpx", r#"{"detail": 2}"#, LATER)
        .err()
        .expect("refused");
    assert_eq!(failure.reason, "usage");
    assert!(failure.message.contains("detail"), "{}", failure.message);
}

/// The Snowdon fixture without the lines that hold `tag`
fn snowdon_without(tag: &str) -> Vec<u8> {
    String::from_utf8(snowdon())
        .unwrap()
        .lines()
        .filter(|line| !line.contains(tag))
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes()
}

fn notes(gpx: &[u8], options: &str) -> Vec<String> {
    let mut builder = Builder::new(gpx, "walk.gpx", options, LATER).unwrap();
    let (built, _) = run(&mut builder, |_| None);
    built.unwrap();
    builder.notes().to_vec()
}

#[test]
fn the_track_s_own_warnings_are_kept_for_the_page() {
    assert!(notes(&snowdon(), OPTIONS).is_empty());
    assert_eq!(
        notes(&snowdon_without("<time>"), OPTIONS),
        [
            "the track has no timestamps, so time-based features are off",
            "the track has no times, so the weather is left out",
        ]
    );
    // Without elevations the weather is asked at another height, which has no recording
    let untracked = notes(&snowdon_without("<ele>"), OPTIONS);
    assert_eq!(untracked[0], "the track has no elevation data");
    assert!(
        untracked[1].starts_with("can't get the weather, so it is left out: "),
        "{untracked:?}"
    );
    assert_eq!(
        notes(&snowdon(), r#"{"detail": 32, "no-map-data": true}"#),
        ["the diorama is built from the elevation alone, without lakes, woods or paths"]
    );
}

fn snowdon_scene() -> Vec<u8> {
    let (built, _) = run(&mut builder(OPTIONS), |_| None);
    built.unwrap()
}

#[test]
fn a_finished_scene_takes_the_season_weather_and_exaggeration_as_the_tool_does() {
    let scene = snowdon_scene();
    assert_eq!(isotrail_scene::adjust(&scene, "{}").unwrap(), scene);
    let options = r#"{"season": "winter", "no-weather": true, "exaggeration": 3}"#;
    let adjusted =
        SceneBundle::from_bytes(&isotrail_scene::adjust(&scene, options).unwrap()).unwrap();
    let mut expected = SceneBundle::from_bytes(&scene).unwrap();
    let render = isotrail_options::Settings::from_json(options, "the test")
        .and_then(|settings| settings.options())
        .unwrap();
    render.data.apply_to(&mut expected);
    expected.grid.exaggeration = 3.0;
    assert_eq!(adjusted, expected);
    assert!(adjusted.weather.is_none());
    assert_eq!(adjusted.season.leaf_day, Some(15));
    let autumn = isotrail_scene::adjust(&scene, r#"{"season": "autumn"}"#).unwrap();
    assert_eq!(
        SceneBundle::from_bytes(&autumn).unwrap().season.leaf_day,
        Some(290)
    );
}

#[test]
fn a_finished_scene_refuses_what_needs_the_gpx_file() {
    let scene = snowdon_scene();
    let failure = isotrail_scene::adjust(&scene, r#"{"trim-start": "1km"}"#).unwrap_err();
    assert_eq!(failure.reason, "usage");
    assert_eq!(
        failure.message,
        "--trim-start can't cut the route a scene bundle holds; give the GPX file instead"
    );
    let failure = isotrail_scene::adjust(b"not a scene", r#"{"season": "winter"}"#).unwrap_err();
    assert_eq!(failure.reason, "input");
    assert!(failure.message.starts_with("can't read the scene file: "));
}

#[test]
fn palette_and_theme_files_get_the_tool_s_messages() {
    assert_eq!(
        isotrail_scene::check_palette("warm.hex", "ff0000\n00ff00\n"),
        Ok(())
    );
    let failure = isotrail_scene::check_palette("warm.hex", "ff0000\nzzzzzz\n").unwrap_err();
    assert_eq!(failure.reason, "usage");
    assert_eq!(
        failure.message,
        r#"can't read the palette warm.hex: line 2: expected a colour as RRGGBB, found "zzzzzz""#
    );
    let failure = isotrail_scene::check_palette("empty.hex", "\n").unwrap_err();
    assert_eq!(
        failure.message,
        "can't read the palette empty.hex: the palette has no colours"
    );
    assert_eq!(
        isotrail_scene::check_theme("mine.toml", "[route]\n"),
        Ok(())
    );
    let failure = isotrail_scene::check_theme("mine.toml", "[route]\nshade = 1\n").unwrap_err();
    assert_eq!(failure.reason, "usage");
    assert_eq!(
        failure.message,
        r#"the theme mine.toml: unknown key "shade" in the [route] table"#
    );
}

#[test]
fn options_are_checked_with_the_tool_s_messages() {
    assert!(isotrail_scene::read_options(r#"{"palette": "aap-64", "route-width": 3}"#).is_ok());
    let failure = isotrail_scene::read_options(r#"{"route-width": 9}"#).expect_err("refused");
    assert_eq!(failure.reason, "usage");
    assert!(
        failure.message.starts_with("route-width in the options: "),
        "{}",
        failure.message
    );
}
