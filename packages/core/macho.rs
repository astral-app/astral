use core::ffi::{c_char, c_int};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::bridge::{AstralRustString, bytes, optional, path};
use crate::crypto::{hex, sha256};

const BLOB_HEADER: usize = 8;
const DER_SEQUENCE: u8 = 0x30;
const DER_OBJECT_IDENTIFIER: u8 = 0x06;
const DER_EXPLICIT_ZERO: u8 = 0xa0;

const MACHO_MAGIC_64: u32 = 0xfeed_facf;
const MACHO_SWAPPED_MAGIC_64: u32 = 0xcffa_edfe;
const FAT_MAGIC: u32 = 0xcafe_babe;
const FAT_MAGIC_64: u32 = 0xcafe_babf;
const CODE_SIGNATURE: u32 = 0x1d;
const EMBEDDED_SIGNATURE_MAGIC: u32 = 0xfade_0cc0;
const ENTITLEMENTS_MAGIC: u32 = 0xfade_7171;
const ENTITLEMENTS_SLOT: u32 = 5;
const SIGNATURE_SLOT: u32 = 0x10000;
const BLOB_WRAPPER_MAGIC: u32 = 0xfade_0b01;
const ENCRYPTION_INFO_64: u32 = 0x2c;
const UUID_COMMAND: u32 = 0x1b;
const BUILD_VERSION: u32 = 0x32;
const VERSION_MIN_IPHONEOS: u32 = 0x25;
const LOAD_DYLIB: u32 = 0x0c;
const LOAD_WEAK_DYLIB: u32 = 0x8000_0018;
const REEXPORT_DYLIB: u32 = 0x8000_001f;
const CPU_TYPE_ARM64: u32 = 0x0100_000c;
const MACH_HEADER_SIZE: usize = 32;
const MAX_FAT_SLICES: u32 = 16;
const MAX_HEADER_BYTES: usize = 4 * 1024 * 1024;
const MAX_SIGNATURE_BYTES: usize = 16 * 1024 * 1024;
const FINGERPRINT_LENGTH: usize = 16;

fn little(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(offset..offset.checked_add(4)?)?.try_into().ok()?))
}

fn big(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(offset..offset.checked_add(4)?)?.try_into().ok()?))
}

fn read_range(file: &mut File, offset: u64, size: usize) -> Option<Vec<u8>> {
    let length = file.metadata().ok()?.len();
    if offset.checked_add(size as u64)? > length {
        return None;
    }
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut data = vec![0; size];
    file.read_exact(&mut data).ok()?;
    Some(data)
}

fn fat_slices(file: &mut File) -> Option<Vec<(u32, u64)>> {
    let header = read_range(file, 0, 8)?;
    let magic = big(&header, 0)?;
    if magic != FAT_MAGIC && magic != FAT_MAGIC_64 {
        return None;
    }
    let wide = magic == FAT_MAGIC_64;
    let entry_size: u32 = if wide { 32 } else { 20 };
    let mut slices = Vec::new();
    for index in 0..big(&header, 4)?.min(MAX_FAT_SLICES) {
        let entry = read_range(file, 8 + u64::from(entry_size) * u64::from(index), entry_size as usize)?;
        let offset = if wide {
            (u64::from(big(&entry, 8)?) << 32) | u64::from(big(&entry, 12)?)
        } else {
            u64::from(big(&entry, 8)?)
        };
        slices.push((big(&entry, 0)?, offset));
    }
    Some(slices)
}

fn is_macho64(file: &mut File, offset: u64) -> bool {
    read_range(file, offset, 4)
        .and_then(|magic| little(&magic, 0))
        .is_some_and(|magic| magic == MACHO_MAGIC_64 || magic == MACHO_SWAPPED_MAGIC_64)
}

fn signature_entitlements(file: &mut File, offset: u64) -> Vec<Vec<u8>> {
    let Some(header) = read_range(file, offset, 12) else {
        return Vec::new();
    };
    if big(&header, 0) != Some(EMBEDDED_SIGNATURE_MAGIC) {
        return Vec::new();
    }
    let count = big(&header, 8).unwrap_or(0);
    let mut blobs = Vec::new();
    for index in 0..u64::from(count) {
        let Some(entry) = read_range(file, offset + 12 + index * 8, 8) else {
            break;
        };
        if big(&entry, 0) != Some(ENTITLEMENTS_SLOT) {
            continue;
        }
        let blob = offset + u64::from(big(&entry, 4).unwrap_or(0));
        let Some(blob_header) = read_range(file, blob, 8) else { continue };
        let length = big(&blob_header, 4).unwrap_or(0) as usize;
        if big(&blob_header, 0) != Some(ENTITLEMENTS_MAGIC) || length < 8 {
            continue;
        }
        if let Some(payload) = read_range(file, blob + 8, length - 8) {
            blobs.push(payload);
        }
    }
    blobs
}

fn macho_entitlements(file: &mut File, base: u64) -> Vec<Vec<u8>> {
    let Some(header) = read_range(file, base, MACH_HEADER_SIZE) else {
        return Vec::new();
    };
    let count = little(&header, 16).unwrap_or(0);
    let mut command = base + MACH_HEADER_SIZE as u64;
    for _ in 0..count {
        let Some(load) = read_range(file, command, 16) else {
            return Vec::new();
        };
        let (kind, size) = (little(&load, 0).unwrap_or(0), little(&load, 4).unwrap_or(0));
        if kind == CODE_SIGNATURE {
            let signature = base + u64::from(little(&load, 8).unwrap_or(0));
            if signature > u64::from(u32::MAX) {
                return Vec::new();
            }
            return signature_entitlements(file, signature);
        }
        command += u64::from(size);
    }
    Vec::new()
}

pub fn entitlement_blobs(executable: &Path) -> Vec<Vec<u8>> {
    let Ok(mut file) = File::open(executable) else { return Vec::new() };
    if let Some(slices) = fat_slices(&mut file) {
        for (_, offset) in slices {
            if !is_macho64(&mut file, offset) {
                continue;
            }
            let blobs = macho_entitlements(&mut file, offset);
            if !blobs.is_empty() {
                return blobs;
            }
        }
        return Vec::new();
    }
    if is_macho64(&mut file, 0) {
        macho_entitlements(&mut file, 0)
    } else {
        Vec::new()
    }
}

pub fn entitlements_from_blob(blob: &[u8]) -> Option<Vec<u8>> {
    let length = big(blob, 4)? as usize;
    if big(blob, 0)? != ENTITLEMENTS_MAGIC || length <= BLOB_HEADER || length > blob.len() {
        return None;
    }
    Some(blob[8..length].to_vec())
}

#[derive(Default)]
pub struct Anatomy {
    pub uuid: String,
    pub cryptid: Option<u32>,
    pub minimum_os: String,
    pub sdk: String,
    pub load_commands: u32,
    pub dylibs: Vec<Vec<u8>>,
    pub signature: Vec<u8>,
}

pub fn version_string(packed: u32) -> String {
    let (major, minor, patch) = (packed >> 16, (packed >> 8) & 0xff, packed & 0xff);
    if patch != 0 {
        format!("{major}.{minor}.{patch}")
    } else {
        format!("{major}.{minor}")
    }
}

fn uuid_string(data: &[u8]) -> String {
    let mut text = String::new();
    for (index, byte) in data.iter().take(16).enumerate() {
        text.push_str(&format!("{byte:02X}"));
        if matches!(index, 3 | 5 | 7 | 9) {
            text.push('-');
        }
    }
    text
}

pub fn parse_anatomy(image: &[u8]) -> Option<Anatomy> {
    if image.len() < MACH_HEADER_SIZE || little(image, 0)? != MACHO_MAGIC_64 {
        return None;
    }
    let count = little(image, 16)?;
    let mut anatomy = Anatomy {
        load_commands: count,
        ..Anatomy::default()
    };
    let mut offset = MACH_HEADER_SIZE;
    for _ in 0..count {
        let (Some(command), Some(size)) = (little(image, offset), little(image, offset + 4)) else {
            break;
        };
        let size = size as usize;
        let Some(load) = image.get(offset..offset.saturating_add(size)).filter(|_| size >= 8) else {
            break;
        };
        match command {
            UUID_COMMAND if size >= 24 => anatomy.uuid = uuid_string(&load[8..24]),
            ENCRYPTION_INFO_64 if size >= 20 => anatomy.cryptid = little(load, 16),
            BUILD_VERSION if size >= 24 => {
                anatomy.minimum_os = version_string(little(load, 12)?);
                anatomy.sdk = version_string(little(load, 16)?);
            }
            VERSION_MIN_IPHONEOS if size >= 16 && anatomy.minimum_os.is_empty() => {
                anatomy.minimum_os = version_string(little(load, 8)?);
                anatomy.sdk = version_string(little(load, 12)?);
            }
            LOAD_DYLIB | LOAD_WEAK_DYLIB | REEXPORT_DYLIB if size >= 24 => {
                let name_offset = little(load, 8)? as usize;
                let name = load.get(name_offset..).unwrap_or(&[]);
                anatomy
                    .dylibs
                    .push(name[..name.iter().position(|byte| *byte == 0).unwrap_or(name.len())].to_vec());
            }
            _ => {}
        }
        offset += size;
    }
    Some(anatomy)
}

fn embedded_signature(super_blob: &[u8]) -> Option<Vec<u8>> {
    if big(super_blob, 0)? != EMBEDDED_SIGNATURE_MAGIC {
        return None;
    }
    for slot in 0..big(super_blob, 8)? as usize {
        let entry = 12 + slot * 8;
        if big(super_blob, entry)? != SIGNATURE_SLOT {
            continue;
        }
        let blob = big(super_blob, entry + 4)? as usize;
        if big(super_blob, blob)? != BLOB_WRAPPER_MAGIC {
            return None;
        }
        let length = big(super_blob, blob + 4)? as usize;
        return (length >= 8)
            .then(|| super_blob.get(blob + 8..blob.checked_add(length)?).map(<[u8]>::to_vec))
            .flatten();
    }
    None
}

pub fn read_anatomy(executable: &Path) -> Option<Anatomy> {
    let mut file = File::open(executable).ok()?;
    let base = match fat_slices(&mut file) {
        Some(slices) => slices.into_iter().find(|(cpu, _)| *cpu == CPU_TYPE_ARM64)?.1,
        None => 0,
    };
    let header = read_range(&mut file, base, MACH_HEADER_SIZE)?;
    if little(&header, 0)? != MACHO_MAGIC_64 {
        return None;
    }
    let command_bytes = (little(&header, 20)? as usize).min(MAX_HEADER_BYTES);
    let image = read_range(&mut file, base, MACH_HEADER_SIZE + command_bytes)?;
    let mut anatomy = parse_anatomy(&image)?;
    let mut offset = MACH_HEADER_SIZE;
    for _ in 0..anatomy.load_commands {
        let (Some(command), Some(size)) = (little(&image, offset), little(&image, offset + 4)) else {
            break;
        };
        if size < 8 || little(&image, offset + 12).is_none() {
            break;
        }
        if command == CODE_SIGNATURE {
            let data_offset = u64::from(little(&image, offset + 8)?);
            let data_size = (little(&image, offset + 12)? as usize).min(MAX_SIGNATURE_BYTES);
            if let Some(signature) = read_range(&mut file, base + data_offset, data_size).and_then(|blob| embedded_signature(&blob)) {
                anatomy.signature = signature;
            }
            break;
        }
        offset += size as usize;
    }
    Some(anatomy)
}

struct Element<'a> {
    tag: u8,
    content: &'a [u8],
    whole: &'a [u8],
}

fn read_element<'a>(der: &'a [u8], offset: &mut usize) -> Option<Element<'a>> {
    let start = *offset;
    let tag = *der.get(start)?;
    let mut cursor = start + 2;
    let mut length = *der.get(start + 1)? as usize;
    if length & 0x80 != 0 {
        let count = length & 0x7f;
        if count == 0 || count > 4 {
            return None;
        }
        length = der
            .get(cursor..cursor + count)?
            .iter()
            .fold(0, |value, byte| (value << 8) | *byte as usize);
        cursor += count;
    }
    let end = cursor.checked_add(length).filter(|end| *end <= der.len())?;
    *offset = end;
    Some(Element {
        tag,
        content: &der[cursor..end],
        whole: &der[start..end],
    })
}

fn children(content: &[u8]) -> Vec<Element<'_>> {
    let mut offset = 0;
    std::iter::from_fn(|| read_element(content, &mut offset)).collect()
}

fn name_attribute(name: &[u8], oid: &[u8]) -> Vec<u8> {
    for set in children(name) {
        for pair in children(set.content) {
            let parts = children(pair.content);
            if parts.len() >= 2 && parts[0].tag == DER_OBJECT_IDENTIFIER && parts[0].content == oid {
                return parts[1].content.to_vec();
            }
        }
    }
    Vec::new()
}

fn tbs_fields(der: &[u8]) -> Vec<Element<'_>> {
    let mut offset = 0;
    let Some(certificate) = read_element(der, &mut offset).filter(|element| element.tag == DER_SEQUENCE) else {
        return Vec::new();
    };
    let parts = children(certificate.content);
    let Some(tbs) = parts.first().filter(|element| element.tag == DER_SEQUENCE) else {
        return Vec::new();
    };
    let mut fields = children(tbs.content);
    if fields.first().is_some_and(|field| field.tag == DER_EXPLICIT_ZERO) {
        fields.remove(0);
    }
    fields
}

pub fn cms_certificates(cms: &[u8]) -> Vec<&[u8]> {
    let mut offset = 0;
    let Some(info) = read_element(cms, &mut offset).filter(|element| element.tag == DER_SEQUENCE) else {
        return Vec::new();
    };
    let info = children(info.content);
    let Some(wrapped) = info.get(1).filter(|element| element.tag == DER_EXPLICIT_ZERO) else {
        return Vec::new();
    };
    let wrapped = children(wrapped.content);
    let Some(signed) = wrapped.first().filter(|element| element.tag == DER_SEQUENCE) else {
        return Vec::new();
    };
    children(signed.content)
        .into_iter()
        .find(|field| field.tag == DER_EXPLICIT_ZERO)
        .map(|field| {
            children(field.content)
                .into_iter()
                .filter(|element| element.tag == DER_SEQUENCE)
                .map(|element| element.whole)
                .collect()
        })
        .unwrap_or_default()
}

pub struct Certificate {
    pub issuer_common_name: Vec<u8>,
    pub fingerprint: String,
}

fn describe(der: &[u8]) -> Option<Certificate> {
    let fields = tbs_fields(der);
    if fields.len() < 5 {
        return None;
    }
    let mut fingerprint = hex(&sha256(der));
    fingerprint.truncate(FINGERPRINT_LENGTH);
    Some(Certificate {
        issuer_common_name: name_attribute(fields[2].content, b"\x55\x04\x03"),
        fingerprint,
    })
}

pub fn signer(cms: &[u8]) -> Option<Certificate> {
    let certificates = cms_certificates(cms);
    let field = |der: &[u8], index: usize| tbs_fields(der).get(index).map(|element| element.whole.to_vec()).unwrap_or_default();
    certificates.iter().enumerate().find_map(|(index, candidate)| {
        let subject = field(candidate, 4);
        let issues_another = certificates
            .iter()
            .enumerate()
            .any(|(other, certificate)| other != index && field(certificate, 2) == subject);
        if issues_another { None } else { describe(candidate) }
    })
}

pub fn short_issuer(issuer: &str) -> String {
    issuer
        .replacen("Worldwide Developer Relations", "WWDR", 1)
        .replacen("Certification Authority", "CA", 1)
}

pub fn signer_line(certificate: &Certificate) -> String {
    let line = format!("sha256 {}", certificate.fingerprint);
    if certificate.issuer_common_name.is_empty() {
        return line;
    }
    format!(
        "{line} · {}",
        short_issuer(&String::from_utf8_lossy(&certificate.issuer_common_name))
    )
}

pub fn anatomy_line(anatomy: &Anatomy, bundle: &[u8]) -> String {
    let mut parts = Vec::new();
    if let Some(cryptid) = anatomy.cryptid {
        parts.push(if cryptid != 0 {
            "encrypted".to_owned()
        } else {
            "decrypted".to_owned()
        });
    }
    if !anatomy.minimum_os.is_empty() {
        parts.push(format!("min {} · SDK {}", anatomy.minimum_os, anatomy.sdk));
    }
    let system = |library: &[u8]| {
        [b"/System/".as_slice(), b"/usr/lib/", b"@rpath/", bundle]
            .iter()
            .any(|prefix| library.starts_with(prefix))
    };
    let injected = anatomy.dylibs.iter().filter(|library| !system(library)).count();
    parts.push(format!("{} dylibs", anatomy.dylibs.len()));
    if injected > 0 {
        parts.push(format!("{injected} injected"));
    }
    if !anatomy.uuid.is_empty() {
        parts.push(format!("UUID {}", &anatomy.uuid[..anatomy.uuid.len().min(8)]));
    }
    parts.join(" · ")
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_entitlement_blobs(executable: *const c_char, length: usize) -> AstralRustString {
    AstralRustString::frames(&entitlement_blobs(path(executable, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_entitlements_from_blob(blob: *const c_char, length: usize) -> AstralRustString {
    optional(entitlements_from_blob(bytes(blob, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_binary_lines(
    executable: *const c_char,
    executable_length: usize,
    bundle: *const c_char,
    bundle_length: usize,
    binary: *mut AstralRustString,
    signer_text: *mut AstralRustString,
) -> c_int {
    let Some(anatomy) = read_anatomy(path(executable, executable_length)) else {
        return 0;
    };
    if let Some(binary) = unsafe { binary.as_mut() } {
        *binary = AstralRustString::new(anatomy_line(&anatomy, bytes(bundle, bundle_length)).into_bytes());
    }
    if let Some(signer_text) = unsafe { signer_text.as_mut() } {
        *signer_text = optional(signer(&anatomy.signature).map(|certificate| signer_line(&certificate).into_bytes()));
    }
    1
}
