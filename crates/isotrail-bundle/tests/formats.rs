use std::path::PathBuf;

use isotrail_bundle::{
    BundleError, FORMAT_VERSION, OLDEST_FORMAT_VERSION, SceneBundle, format_version,
};
use miniz_oxide::inflate::decompress_to_vec;

const CHANGING_THE_FORMAT: &str = "a new format needs a reader for the one before it, \
     and a file of each in fixtures/bundles: see \"Changing the scene file format\" in CONTRIBUTING.md";

fn fixture(format: u16) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/bundles")
        .join(format!("format-{format}.itb"));
    std::fs::read(&path)
        .unwrap_or_else(|err| panic!("{}: {err}; {CHANGING_THE_FORMAT}", path.display()))
}

#[test]
fn the_format_before_the_current_one_is_still_read() {
    assert_eq!(
        OLDEST_FORMAT_VERSION + 1,
        FORMAT_VERSION,
        "{CHANGING_THE_FORMAT}"
    );
    for format in OLDEST_FORMAT_VERSION..=FORMAT_VERSION {
        let bytes = fixture(format);
        assert_eq!(format_version(&bytes), Some(format));
        if let Err(err) = SceneBundle::from_bytes(&bytes) {
            panic!("format {format}: {err}; {CHANGING_THE_FORMAT}");
        }
    }
}

#[test]
fn the_older_file_reads_as_its_current_twin() {
    let old = SceneBundle::from_bytes(&fixture(OLDEST_FORMAT_VERSION)).unwrap();
    let current = SceneBundle::from_bytes(&fixture(FORMAT_VERSION)).unwrap();
    assert_eq!(old, current);
    assert!(!current.water.is_empty() && current.water.iter().all(|body| !body.sea));
    assert!(!current.route.points.is_empty() && current.weather.is_some());
}

#[test]
fn the_current_file_holds_today_s_layout() {
    let bytes = fixture(FORMAT_VERSION);
    let bundle = SceneBundle::from_bytes(&bytes).unwrap();
    // Compared before compression, so a newer deflate can't break it
    assert_eq!(
        decompress_to_vec(&bytes[6..]).unwrap(),
        postcard::to_allocvec(&bundle).unwrap(),
        "the layout changed: {CHANGING_THE_FORMAT}"
    );
}

#[test]
fn a_damaged_older_file_is_refused_in_plain_words() {
    let bytes = fixture(OLDEST_FORMAT_VERSION);
    let mut garbled = bytes.clone();
    let middle = garbled.len() / 2;
    garbled[middle..middle + 8].fill(0xa5);
    for damaged in [bytes[..bytes.len() - 20].to_vec(), garbled] {
        let err = SceneBundle::from_bytes(&damaged).unwrap_err();
        assert!(matches!(err, BundleError::Corrupt(_)), "{err:?}");
        let message = err.to_string();
        assert!(message.starts_with("the bundle is damaged"), "{message}");
    }
}
