//! Writes the synthetic hike bundle the web demo loads: `demo_bundle <out.itb>`

fn main() -> std::io::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "web/demo/hike.itb".into());
    std::fs::write(&path, isotrail_testkit::hike_bundle().to_bytes())?;
    eprintln!("wrote {path}");
    Ok(())
}
