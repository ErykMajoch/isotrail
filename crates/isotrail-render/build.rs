// For the web build, parses each WGSL shader into naga IR, so the runtime
// carries no WGSL parser

use std::path::PathBuf;
use std::{env, fs};

const SHADERS: [&str; 5] = ["post", "present", "route", "terrain", "test_scene"];

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    for name in SHADERS {
        println!("cargo::rerun-if-changed=src/{name}.wgsl");
    }
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("wasm32") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    for name in SHADERS {
        let path = format!("src/{name}.wgsl");
        let source = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string_with_path(&source, &path)));
        let bytes = postcard::to_allocvec(&module).expect("naga IR serialises");
        fs::write(out.join(format!("{name}.naga")), bytes)
            .unwrap_or_else(|e| panic!("{name}.naga: {e}"));
    }
}
