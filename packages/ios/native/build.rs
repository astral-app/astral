use std::env;
use std::path::PathBuf;

const HEADER: &str = "../.theos/generated/Astral.h";
const KINDS_HEADER: &str = "../.theos/generated/AstralRequestKinds.h";
const KINDS_ENUM: &str = "pub enum AstralRequestKind {";

fn request_kinds(bridge: &str) -> Vec<String> {
    let start = bridge.find(KINDS_ENUM).expect("AstralRequestKind enum") + KINDS_ENUM.len();
    let end = start + bridge[start..].find('}').expect("AstralRequestKind end");
    bridge[start..end]
        .lines()
        .map(|line| line.trim().trim_end_matches(','))
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

fn write_kinds(manifest: &std::path::Path) {
    let bridge = std::fs::read_to_string(manifest.join("bridge.rs")).expect("bridge.rs");
    let entries: Vec<String> = request_kinds(&bridge)
        .iter()
        .map(|kind| format!("    X({kind})"))
        .collect();
    let header = format!(
        "#ifndef ASTRAL_REQUEST_KINDS_H\n#define ASTRAL_REQUEST_KINDS_H\n\n#define ASTRAL_REQUEST_KINDS(X) \\\n{}\n\n#endif\n",
        entries.join(" \\\n")
    );
    let path = manifest.join(KINDS_HEADER);
    if std::fs::read_to_string(&path).ok().as_deref() != Some(header.as_str()) {
        std::fs::write(path, header).expect("AstralRequestKinds.h");
    }
}

fn main() {
    println!("cargo:rerun-if-changed=lib.rs");
    println!("cargo:rerun-if-changed=bridge.rs");
    println!("cargo:rerun-if-changed=text.rs");
    println!("cargo:rerun-if-changed=network.rs");
    println!("cargo:rerun-if-changed=shapes.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed={HEADER}");
    println!("cargo:rerun-if-changed={KINDS_HEADER}");
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let config =
        cbindgen::Config::from_file(manifest.join("cbindgen.toml")).expect("cbindgen.toml");
    let header = manifest.join(HEADER);
    if let Some(directory) = header.parent() {
        std::fs::create_dir_all(directory).expect("generated header directory");
    }
    write_kinds(&manifest);
    cbindgen::Builder::new()
        .with_crate(&manifest)
        .with_config(config)
        .generate()
        .expect("Astral.h")
        .write_to_file(header);
}
