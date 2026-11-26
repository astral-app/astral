use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn properties(header: &str) -> Vec<(String, u16)> {
    header
        .lines()
        .filter_map(|line| {
            let entry = line.trim().strip_prefix("X(")?;
            let (name, rest) = entry.split_once(',')?;
            let fallback = rest.trim().split(')').next()?.trim().parse().ok()?;
            Some((name.trim().to_string(), fallback))
        })
        .collect()
}

const KEYWORDS: [&str; 4] = ["loop", "type", "move", "match"];

fn snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for character in name.chars() {
        if character.is_ascii_uppercase() {
            out.push('_');
            out.push(character.to_ascii_lowercase());
        } else {
            out.push(character);
        }
    }
    if KEYWORDS.contains(&out.as_str()) {
        out.insert_str(0, "r#");
    }
    out
}

fn generated(properties: &[(String, u16)]) -> String {
    let fields: String = properties
        .iter()
        .map(|(name, _)| format!("    pub {}: u16,\n", snake_case(name)))
        .collect();
    let fallbacks: String = properties
        .iter()
        .map(|(name, fallback)| format!("    {}: {fallback},\n", snake_case(name)))
        .collect();
    let names: String = properties.iter().map(|(name, _)| format!("    \"{name}\",\n")).collect();
    let setters: String = properties
        .iter()
        .enumerate()
        .map(|(index, (name, _))| format!("        {index} => self.{} = value,\n", snake_case(name)))
        .collect();
    format!(
        "#[repr(C)]\n#[derive(Clone, Copy)]\npub struct AstralWireProperties {{\n{fields}}}\n\n\
         pub const FALLBACK: AstralWireProperties = AstralWireProperties {{\n{fallbacks}}};\n\n\
         pub const NAMES: [&str; {count}] = [\n{names}];\n\n\
         impl AstralWireProperties {{\n    pub fn set(&mut self, index: usize, value: u16) {{\n        match index {{\n{setters}        _ => {{}}\n        }}\n    }}\n}}\n",
        count = properties.len()
    )
}

fn main() {
    let header = PathBuf::from("wire/PhoenixProperties.h");
    println!("cargo:rerun-if-changed={}", header.display());
    let text = fs::read_to_string(&header).expect("PhoenixProperties.h");
    let properties = properties(&text);
    assert!(!properties.is_empty(), "no ViewWire properties in PhoenixProperties.h");
    let output = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("properties.rs");
    fs::write(output, generated(&properties)).expect("write properties.rs");
    link_lua_for_host();
}

fn link_lua_for_host() {
    if env::var("TARGET").ok() != env::var("HOST").ok() {
        return;
    }
    let source = PathBuf::from("../../vendor/lua/onelua.c");
    println!("cargo:rerun-if-changed={}", source.display());
    let object = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("onelua.o");
    let status = Command::new("cc")
        .args(["-c", "-O1", "-w", "-DMAKE_LIB", "-DLUA_USE_POSIX"])
        .arg(&source)
        .arg("-o")
        .arg(&object)
        .status()
        .expect("cc for onelua.c");
    assert!(status.success(), "could not compile onelua.c for host tests");
    let directory = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let archive = directory.join("liblua_host.a");
    let _ = fs::remove_file(&archive);
    let status = Command::new("ar")
        .arg("rcs")
        .arg(&archive)
        .arg(&object)
        .status()
        .expect("ar for liblua_host.a");
    assert!(status.success(), "could not archive onelua.o");
    println!("cargo:rustc-link-search=native={}", directory.display());
    println!("cargo:rustc-link-lib=static=lua_host");
}
