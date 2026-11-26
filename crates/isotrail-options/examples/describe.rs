//! Writes the description of every render option as JSON, for the web app's
//! controls: `cargo run -p isotrail-options --example describe -- FILE`

use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        anyhow::bail!("give the file to write the description to");
    };
    let json = serde_json::to_string_pretty(&isotrail_options::describe())?;
    std::fs::write(&path, json + "\n")?;
    Ok(())
}
