use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use plist::{Dictionary, Value};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub struct Ipa {
    archive: ZipArchive<File>,
}

pub type Patch = Box<dyn FnOnce(Vec<u8>) -> Result<Vec<u8>, String>>;

pub enum Action {
    Keep,
    Drop,
    Patch(Patch),
}

pub struct Addition {
    pub name: String,
    pub mode: u32,
    pub data: Vec<u8>,
}

fn failure(error: impl std::fmt::Display) -> String {
    error.to_string()
}

impl Ipa {
    pub fn open(path: &Path) -> Result<Ipa, String> {
        let file = File::open(path)
            .map_err(|error| format!("Failed to open {}: {error}", path.display()))?;
        Ok(Ipa {
            archive: ZipArchive::new(file).map_err(failure)?,
        })
    }

    pub fn names(&self) -> Vec<String> {
        self.archive
            .file_names()
            .filter_map(|name| name.ok().map(std::borrow::Cow::into_owned))
            .collect::<Vec<_>>()
    }

    pub fn ordered_names(&mut self) -> Result<Vec<String>, String> {
        (0..self.archive.len())
            .map(|index| {
                Ok(self
                    .archive
                    .by_index_raw(index)
                    .map_err(failure)?
                    .name()
                    .map_err(failure)?
                    .into_owned())
            })
            .collect()
    }

    pub fn has(&self, name: &str) -> bool {
        self.archive.index_for_name(name).is_some()
    }

    pub fn read(&mut self, name: &str) -> Result<Vec<u8>, String> {
        let mut entry = self
            .archive
            .by_name(name)
            .map_err(|_| format!("{name} is not in the IPA"))?;
        let mut data = Vec::new();
        entry.read_to_end(&mut data).map_err(failure)?;
        Ok(data)
    }

    pub fn app(&mut self) -> Result<String, String> {
        for name in self.ordered_names()? {
            let parts: Vec<&str> = name.split('/').collect();
            if parts.len() >= 3
                && parts[0] == "Payload"
                && parts[1].ends_with(".app")
                && parts[2] == "Info.plist"
            {
                return Ok(format!("Payload/{}/", parts[1]));
            }
        }
        Err("the IPA has no Payload/*.app/Info.plist".into())
    }

    pub fn info(&mut self, app: &str) -> Result<Dictionary, String> {
        read_plist(&self.read(&format!("{app}Info.plist"))?)
    }
}

pub fn entries(path: &Path) -> Result<Vec<String>, String> {
    Ipa::open(path)?.ordered_names()
}

fn options(mode: u32) -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(mode & 0o7777)
}

fn copy_entries(
    ipa: &mut Ipa,
    writer: &mut ZipWriter<File>,
    decide: &mut dyn FnMut(&str) -> Action,
) -> Result<(), String> {
    for index in 0..ipa.archive.len() {
        let name = ipa
            .archive
            .by_index_raw(index)
            .map_err(failure)?
            .name()
            .map_err(failure)?
            .into_owned();
        match decide(&name) {
            Action::Drop => {}
            Action::Keep => {
                let entry = ipa.archive.by_index_raw(index).map_err(failure)?;
                writer.raw_copy_file(entry).map_err(failure)?;
            }
            Action::Patch(patch) => {
                let mut entry = ipa.archive.by_index(index).map_err(failure)?;
                let mode = entry.unix_mode().unwrap_or(0o644);
                let mut data = Vec::new();
                entry.read_to_end(&mut data).map_err(failure)?;
                drop(entry);
                let patched = patch(data)?;
                writer.start_file(name, options(mode)).map_err(failure)?;
                writer.write_all(&patched).map_err(failure)?;
            }
        }
    }
    Ok(())
}

pub fn rewrite(
    path: &Path,
    label: &str,
    mut decide: impl FnMut(&str) -> Action,
    additions: Vec<Addition>,
) -> Result<(), String> {
    let mut ipa = Ipa::open(path)?;
    let temporary = path.with_file_name(format!(".astral-{label}-{}.ipa", std::process::id()));
    let result = (|| {
        let mut writer = ZipWriter::new(File::create(&temporary).map_err(failure)?);
        copy_entries(&mut ipa, &mut writer, &mut decide)?;
        for addition in additions {
            writer
                .start_file(addition.name, options(addition.mode))
                .map_err(failure)?;
            writer.write_all(&addition.data).map_err(failure)?;
        }
        writer.finish().map_err(failure)?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    std::fs::rename(&temporary, path).map_err(failure)
}

pub fn read_plist(data: &[u8]) -> Result<Dictionary, String> {
    match Value::from_reader(Cursor::new(data)).map_err(failure)? {
        Value::Dictionary(dictionary) => Ok(dictionary),
        _ => Err("property list is not a dictionary".into()),
    }
}

pub fn read_plist_file(path: &Path) -> Result<Dictionary, String> {
    read_plist(
        &std::fs::read(path)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?,
    )
}

pub fn write_plist(dictionary: Dictionary, binary: bool) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let value = Value::Dictionary(dictionary);
    if binary {
        value.to_writer_binary(&mut out).map_err(failure)?;
    } else {
        value.to_writer_xml(&mut out).map_err(failure)?;
    }
    Ok(out)
}

pub fn is_binary_plist(data: &[u8]) -> bool {
    data.starts_with(b"bplist")
}

pub fn text(dictionary: &Dictionary, key: &str) -> String {
    dictionary
        .get(key)
        .and_then(Value::as_string)
        .unwrap_or_default()
        .to_owned()
}

pub fn ensure_url_scheme(info: &mut Dictionary, scheme: &str) -> bool {
    let present = info
        .get("CFBundleURLTypes")
        .and_then(Value::as_array)
        .is_some_and(|types| {
            types.iter().filter_map(Value::as_dictionary).any(|entry| {
                entry
                    .get("CFBundleURLSchemes")
                    .and_then(Value::as_array)
                    .is_some_and(|schemes| {
                        schemes
                            .iter()
                            .any(|existing| existing.as_string() == Some(scheme))
                    })
            })
        });
    if present {
        return false;
    }
    let mut entry = Dictionary::new();
    entry.insert(
        "CFBundleURLName".into(),
        Value::String("app.astral.client".into()),
    );
    entry.insert(
        "CFBundleURLSchemes".into(),
        Value::Array(vec![Value::String(scheme.into())]),
    );
    let mut types = info
        .get("CFBundleURLTypes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    types.push(Value::Dictionary(entry));
    info.insert("CFBundleURLTypes".into(), Value::Array(types));
    true
}
