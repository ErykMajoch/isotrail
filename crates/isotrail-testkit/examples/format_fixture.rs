//! Writes `fixtures/bundles/format-N.itb` for the current format from the
//! file of the format before, read and converted as isotrail reads it

use std::path::PathBuf;

use isotrail_bundle::{FORMAT_VERSION, OLDEST_FORMAT_VERSION, SceneBundle};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/bundles");
    let from = dir.join(format!("format-{OLDEST_FORMAT_VERSION}.itb"));
    let to = dir.join(format!("format-{FORMAT_VERSION}.itb"));
    let bundle = SceneBundle::from_bytes(&std::fs::read(&from)?)?;
    std::fs::write(&to, bundle.to_bytes()?)?;
    eprintln!("wrote {} from {}", to.display(), from.display());
    Ok(())
}
