use std::collections::{BTreeSet, HashMap};
use std::io::Read;

use serde_json::{Map, Value};

const MAGIC: &[u8] = b"dex\n";
const HEADER_SIZE: usize = 0x70;
const STRING_IDS_SIZE: usize = 0x38;
const TYPE_IDS_SIZE: usize = 0x40;
const CLASS_DEFS_SIZE: usize = 0x60;
const CLASS_DEF_LENGTH: usize = 32;
const CLASS_DATA_OFFSET: usize = 24;
const CODE_INSTRUCTIONS_SIZE: usize = 12;
const CODE_INSTRUCTIONS: usize = 16;
const CONST_STRING: u16 = 0x1a;
const CONST_STRING_JUMBO: u16 = 0x1b;
const MAX_DEX_BYTES: u64 = 64 << 20;

struct Dex<'a> {
    bytes: &'a [u8],
}

impl Dex<'_> {
    fn u16_at(&self, offset: usize) -> Option<u16> {
        Some(u16::from_le_bytes(self.bytes.get(offset..offset + 2)?.try_into().ok()?))
    }

    fn u32_at(&self, offset: usize) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes.get(offset..offset + 4)?.try_into().ok()?))
    }

    fn index_at(&self, offset: usize) -> Option<usize> {
        self.u32_at(offset).map(|value| value as usize)
    }

    fn uleb(&self, offset: &mut usize) -> Option<usize> {
        let mut value = 0usize;
        for shift in (0..35).step_by(7) {
            let byte = *self.bytes.get(*offset)?;
            *offset += 1;
            value |= usize::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    fn string(&self, index: usize) -> Option<&[u8]> {
        let table = self.index_at(STRING_IDS_SIZE + 4)?;
        let mut offset = self.index_at(table + index * 4)?;
        self.uleb(&mut offset)?;
        let length = self.bytes.get(offset..)?.iter().position(|byte| *byte == 0)?;
        self.bytes.get(offset..offset + length)
    }

    fn type_name(&self, index: usize) -> Option<String> {
        let table = self.index_at(TYPE_IDS_SIZE + 4)?;
        let descriptor = self.string(self.index_at(table + index * 4)?)?;
        let inner = descriptor.strip_prefix(b"L")?.strip_suffix(b";")?;
        Some(String::from_utf8_lossy(inner).replace('/', "."))
    }

    fn anchor_indices(&self, anchors: &[&str]) -> HashMap<u32, usize> {
        let count = self.index_at(STRING_IDS_SIZE).unwrap_or(0);
        (0..count)
            .filter_map(|index| {
                let text = self.string(index)?;
                let anchor = anchors.iter().position(|anchor| anchor.as_bytes() == text)?;
                Some((u32::try_from(index).ok()?, anchor))
            })
            .collect()
    }

    fn code_strings(&self, code: usize, wanted: &HashMap<u32, usize>, found: &mut BTreeSet<usize>) -> Option<()> {
        let units = self.index_at(code + CODE_INSTRUCTIONS_SIZE)?;
        let start = code + CODE_INSTRUCTIONS;
        for unit in 0..units {
            let at = start + unit * 2;
            let opcode = self.u16_at(at)? & 0xff;
            let index = match opcode {
                CONST_STRING => u32::from(self.u16_at(at + 2)?),
                CONST_STRING_JUMBO => self.u32_at(at + 2)?,
                _ => continue,
            };
            if let Some(anchor) = wanted.get(&index) {
                found.insert(*anchor);
            }
        }
        Some(())
    }

    fn class_anchors(&self, class_data: usize, wanted: &HashMap<u32, usize>) -> Option<BTreeSet<usize>> {
        let mut offset = class_data;
        let static_fields = self.uleb(&mut offset)?;
        let instance_fields = self.uleb(&mut offset)?;
        let direct_methods = self.uleb(&mut offset)?;
        let virtual_methods = self.uleb(&mut offset)?;
        for _ in 0..(static_fields + instance_fields) * 2 {
            self.uleb(&mut offset)?;
        }
        let mut found = BTreeSet::new();
        for _ in 0..direct_methods + virtual_methods {
            self.uleb(&mut offset)?;
            self.uleb(&mut offset)?;
            let code = self.uleb(&mut offset)?;
            if code != 0 {
                self.code_strings(code, wanted, &mut found)?;
            }
        }
        Some(found)
    }

    fn scan(&self, anchors: &[&str], results: &mut [BTreeSet<String>]) {
        if !self.bytes.starts_with(MAGIC) || self.bytes.len() < HEADER_SIZE {
            return;
        }
        let wanted = self.anchor_indices(anchors);
        if wanted.is_empty() {
            return;
        }
        let count = self.index_at(CLASS_DEFS_SIZE).unwrap_or(0);
        let table = self.index_at(CLASS_DEFS_SIZE + 4).unwrap_or(0);
        for class in 0..count {
            let definition = table + class * CLASS_DEF_LENGTH;
            let Some(class_data) = self.index_at(definition + CLASS_DATA_OFFSET).filter(|offset| *offset != 0) else {
                continue;
            };
            let Some(found) = self.class_anchors(class_data, &wanted).filter(|found| !found.is_empty()) else {
                continue;
            };
            let Some(name) = self.index_at(definition).and_then(|index| self.type_name(index)) else {
                continue;
            };
            for anchor in found {
                results[anchor].insert(name.clone());
            }
        }
    }
}

pub fn anchored_classes(apk: &str, anchors: &[&str]) -> Value {
    let mut results = vec![BTreeSet::new(); anchors.len()];
    let archive = std::fs::File::open(apk).ok().and_then(|file| zip::ZipArchive::new(file).ok());
    if let Some(mut archive) = archive {
        let names: Vec<String> = archive
            .file_names()
            .filter_map(Result::ok)
            .filter(|name| name.starts_with("classes") && name.ends_with(".dex"))
            .map(|name| name.into_owned())
            .collect();
        for name in names {
            let Ok(entry) = archive.by_name(&name) else { continue };
            let mut bytes = Vec::new();
            if entry.take(MAX_DEX_BYTES).read_to_end(&mut bytes).is_ok() {
                Dex { bytes: &bytes }.scan(anchors, &mut results);
            }
        }
    }
    let map: Map<String, Value> = anchors
        .iter()
        .zip(results)
        .map(|(anchor, classes)| ((*anchor).to_owned(), Value::from(classes.into_iter().collect::<Vec<_>>())))
        .collect();
    Value::Object(map)
}
