use std::env;
use std::path::PathBuf;

const HEADER: &str = "../.theos/generated/Astral.h";

fn main() {
    println!("cargo:rerun-if-changed=lib.rs");
    println!("cargo:rerun-if-changed=bridge.rs");
    println!("cargo:rerun-if-changed=text.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed={HEADER}");
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let config =
        cbindgen::Config::from_file(manifest.join("cbindgen.toml")).expect("cbindgen.toml");
    let header = manifest.join(HEADER);
    if let Some(directory) = header.parent() {
        std::fs::create_dir_all(directory).expect("generated header directory");
    }
    cbindgen::Builder::new()
        .with_crate(&manifest)
        .with_config(config)
        .generate()
        .expect("Astral.h")
        .write_to_file(header);
}
