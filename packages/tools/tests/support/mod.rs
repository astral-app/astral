#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};

use astral_tools::macho;
use plist::{Dictionary, Value};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

pub struct Temporary(pub PathBuf);

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn temporary(label: &str) -> Temporary {
    let directory = std::env::temp_dir().join(format!(
        "astral-tools-{label}-{}-{}",
        std::process::id(),
        std::time::UNIX_EPOCH
            .elapsed()
            .map_or(0, |elapsed| elapsed.as_nanos())
    ));
    std::fs::create_dir_all(&directory).expect("temporary directory");
    Temporary(directory)
}

pub fn write_zip(path: &Path, entries: &[(&str, Vec<u8>)]) {
    let mut writer = ZipWriter::new(std::fs::File::create(path).expect("zip file"));
    for (name, data) in entries {
        writer
            .start_file(*name, SimpleFileOptions::default().unix_permissions(0o755))
            .expect("zip entry");
        writer.write_all(data).expect("zip data");
    }
    writer.finish().expect("zip finish");
}

pub fn read_zip(path: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(path).expect("zip")).expect("archive");
    let mut entries = std::collections::BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("entry");
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut data).expect("read");
        entries.insert(entry.name().expect("name").into_owned(), data);
    }
    entries
}

pub fn machine_header(file_type: u32, size: usize) -> Vec<u8> {
    let mut data = vec![0u8; size];
    data[0..4].copy_from_slice(&0xfeed_facfu32.to_le_bytes());
    data[4..8].copy_from_slice(&macho::CPU_ARM64.to_le_bytes());
    data[12..16].copy_from_slice(&file_type.to_le_bytes());
    data
}

pub fn bundle_binary() -> Vec<u8> {
    machine_header(8, 512)
}

pub fn group_dylib(install: &str) -> Vec<u8> {
    let padded = (24 + install.len() + 1 + 7) & !7;
    let mut data = macho::add_weak_dylib(&machine_header(macho::TYPE_DYLIB, 32 + padded), install)
        .expect("dylib");
    data[32..36].copy_from_slice(&macho::ID_DYLIB.to_le_bytes());
    data
}

pub fn plist(pairs: &[(&str, Value)]) -> Vec<u8> {
    let mut dictionary = Dictionary::new();
    for (key, value) in pairs {
        dictionary.insert((*key).to_owned(), value.clone());
    }
    let mut out = Vec::new();
    Value::Dictionary(dictionary)
        .to_writer_xml(&mut out)
        .expect("plist");
    out
}

pub fn extension_info(identifier: &str, executable: &str) -> Vec<u8> {
    plist(&[
        ("CFBundleIdentifier", Value::String(identifier.into())),
        ("CFBundleExecutable", Value::String(executable.into())),
    ])
}
