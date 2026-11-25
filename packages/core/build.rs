use std::env;
use std::fs;
use std::path::PathBuf;

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

fn generated(properties: &[(String, u16)]) -> String {
    let fields: String = properties.iter().map(|(name, _)| format!("    pub {name}: u16,\n")).collect();
    let fallbacks: String = properties
        .iter()
        .map(|(name, fallback)| format!("    {name}: {fallback},\n"))
        .collect();
    let names: String = properties.iter().map(|(name, _)| format!("    \"{name}\",\n")).collect();
    let setters: String = properties
        .iter()
        .enumerate()
        .map(|(index, (name, _))| format!("        {index} => self.{name} = value,\n"))
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
}
