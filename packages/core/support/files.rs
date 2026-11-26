use core::ffi::{CStr, c_char, c_int};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::diagnostics::disk_full::note_write_failure;
use crate::support::bridge::{AstralRustString, bytes, epoch_ms, give, path};

static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn read_text(path: &Path) -> Option<Vec<u8>> {
    fs::read(path).ok()
}

fn error_text(code: i32) -> String {
    let text = unsafe { libc::strerror(code) };
    if text.is_null() {
        return format!("error {code}");
    }
    unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
}

pub fn unreadable_reason(path: &Path) -> String {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return "the file is missing".to_owned(),
        Err(error) => return error_text(error.raw_os_error().unwrap_or(0)),
    };
    match File::open(path) {
        Ok(_) => "it could not be read".to_owned(),
        Err(error) => {
            let code = error.raw_os_error().unwrap_or(0);
            let reason = error_text(code);
            if code == libc::EACCES {
                format!("{reason} (owned by uid {})", metadata.uid())
            } else {
                reason
            }
        }
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut joined = path.as_os_str().to_owned();
    joined.push(suffix);
    PathBuf::from(joined)
}

pub fn move_aside_unreadable(path: &Path) -> bool {
    let code = match File::open(path) {
        Ok(_) => return false,
        Err(error) => error.raw_os_error().unwrap_or(0),
    };
    if code != libc::EACCES && code != libc::EPERM {
        return false;
    }
    let aside = with_suffix(path, ".unreadable");
    let _ = fs::remove_file(&aside);
    fs::rename(path, aside).is_ok()
}

fn staging_file(path: &Path) -> std::io::Result<(PathBuf, File)> {
    loop {
        let sequence = STAGING_COUNTER.fetch_add(1, Ordering::Relaxed);
        let seed = (epoch_ms() as u64) ^ (u64::from(std::process::id()) << 20) ^ sequence.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let staging = with_suffix(path, &format!(".tmp-{:06x}", seed & 0xff_ffff));
        match OpenOptions::new().write(true).create_new(true).mode(0o644).open(&staging) {
            Ok(file) => return Ok((staging, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

fn flush(file: &File) -> std::io::Result<()> {
    if unsafe { libc::fsync(file.as_raw_fd()) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn sync_directory(directory: Option<&Path>) {
    let directory = directory
        .filter(|directory| !directory.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if let Ok(handle) = File::open(directory) {
        let _ = flush(&handle);
    }
}

fn note(error: &std::io::Error, path: &Path) {
    note_write_failure(error.raw_os_error().unwrap_or(0), path, epoch_ms());
}

pub fn write_atomically(path: &Path, contents: &[u8]) -> bool {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        let _ = fs::create_dir_all(parent);
    }
    let (staging, mut file) = match staging_file(path) {
        Ok(created) => created,
        Err(error) => {
            note(&error, path);
            return false;
        }
    };
    let _ = file.set_permissions(fs::Permissions::from_mode(0o644));
    if let Err(error) = file.write_all(contents).and_then(|()| flush(&file)) {
        note(&error, path);
        drop(file);
        let _ = fs::remove_file(&staging);
        return false;
    }
    drop(file);
    if fs::rename(&staging, path).is_err() {
        let _ = fs::remove_file(&staging);
        return false;
    }
    sync_directory(path.parent());
    true
}

pub fn remove(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir(path).is_ok(),
        Ok(_) => fs::remove_file(path).is_ok(),
        Err(_) => false,
    }
}

pub fn is_contained_relative(relative: &[u8]) -> bool {
    if relative.is_empty() || relative.starts_with(b"/") {
        return false;
    }
    relative.split(|byte| *byte == b'/').all(|part| part != b".." && part != b".")
}

pub fn remove_tree(root: &[u8], relative: &[u8]) -> bool {
    if root.is_empty() || !is_contained_relative(relative) {
        return false;
    }
    let target = Path::new(OsStr::from_bytes(root)).join(OsStr::from_bytes(relative));
    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(&target).is_ok(),
        Ok(_) => fs::remove_file(&target).is_ok(),
        Err(_) => false,
    }
}

pub fn file_size(path: &Path) -> u64 {
    fs::metadata(path)
        .ok()
        .filter(|metadata| metadata.is_file())
        .map_or(0, |metadata| metadata.len())
}

pub fn list_directory(path: &Path) -> Vec<Vec<u8>> {
    let Ok(entries) = fs::read_dir(path) else { return Vec::new() };
    let mut names: Vec<Vec<u8>> = entries.flatten().map(|entry| entry.file_name().as_bytes().to_vec()).collect();
    names.sort();
    names
}

pub fn join_path(base: &[u8], component: &[u8]) -> Vec<u8> {
    if component.is_empty() {
        return base.to_vec();
    }
    if component.starts_with(b"/") {
        return component.to_vec();
    }
    let mut joined = base.to_vec();
    if !base.is_empty() && !base.ends_with(b"/") {
        joined.push(b'/');
    }
    joined.extend_from_slice(component);
    joined
}

fn lexically_normal(relative: &[u8]) -> Vec<u8> {
    let mut parts: Vec<&[u8]> = Vec::new();
    for part in relative.split(|byte| *byte == b'/') {
        match part {
            b"" | b"." => {}
            b".." if parts.last().is_some_and(|last| *last != b"..") => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        return if relative.is_empty() { Vec::new() } else { b".".to_vec() };
    }
    let mut normal = parts.join(&b'/');
    let last = relative.rsplit(|byte| *byte == b'/').next().unwrap_or(b"");
    if matches!(last, b"" | b"." | b"..") && parts.last().is_some_and(|part| *part != b"..") {
        normal.push(b'/');
    }
    normal
}

fn weakly_canonical(path: &Path) -> Option<PathBuf> {
    let components: Vec<Component> = path.components().collect();
    for split in (0..=components.len()).rev() {
        let prefix: PathBuf = components[..split].iter().collect();
        let base = if split == 0 {
            std::env::current_dir().ok()?
        } else {
            match prefix.canonicalize() {
                Ok(canonical) => canonical,
                Err(_) => continue,
            }
        };
        let mut resolved = base;
        for component in &components[split..] {
            match component {
                Component::ParentDir => {
                    resolved.pop();
                }
                Component::CurDir => {}
                other => resolved.push(other.as_os_str()),
            }
        }
        return Some(resolved);
    }
    None
}

pub fn resolve_inside(directory: &[u8], relative: &[u8]) -> Option<Vec<u8>> {
    let normal = lexically_normal(relative);
    if relative.is_empty() || normal.is_empty() || normal.starts_with(b"/") || normal == b".." || normal.starts_with(b"../") {
        return None;
    }
    let directory = Path::new(OsStr::from_bytes(directory));
    let root = weakly_canonical(directory)?;
    let joined = directory.join(OsStr::from_bytes(&normal));
    let resolved = weakly_canonical(&joined)?;
    if !resolved.starts_with(&root) {
        return None;
    }
    Some(joined.as_os_str().as_bytes().to_vec())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_read_text_file(data: *const c_char, length: usize, into: *mut AstralRustString) -> c_int {
    c_int::from(unsafe { give(read_text(path(data, length)), into) })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_unreadable_reason(data: *const c_char, length: usize) -> AstralRustString {
    AstralRustString::new(unreadable_reason(path(data, length)).into_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_move_aside_unreadable(data: *const c_char, length: usize) -> c_int {
    c_int::from(move_aside_unreadable(path(data, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_write_text_file_atomically(
    data: *const c_char,
    length: usize,
    contents: *const c_char,
    contents_length: usize,
) -> c_int {
    c_int::from(write_atomically(path(data, length), bytes(contents, contents_length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_copy_file(
    source: *const c_char,
    source_length: usize,
    target: *const c_char,
    target_length: usize,
) -> c_int {
    c_int::from(fs::copy(path(source, source_length), path(target, target_length)).is_ok())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_remove_file(data: *const c_char, length: usize) -> c_int {
    c_int::from(remove(path(data, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_is_contained_relative_path(data: *const c_char, length: usize) -> c_int {
    c_int::from(is_contained_relative(bytes(data, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_remove_tree(
    root: *const c_char,
    root_length: usize,
    relative: *const c_char,
    relative_length: usize,
) -> c_int {
    c_int::from(remove_tree(bytes(root, root_length), bytes(relative, relative_length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_file_size(data: *const c_char, length: usize) -> u64 {
    file_size(path(data, length))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_file_exists(data: *const c_char, length: usize) -> c_int {
    c_int::from(path(data, length).exists())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_is_directory(data: *const c_char, length: usize) -> c_int {
    c_int::from(path(data, length).is_dir())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_create_directories(data: *const c_char, length: usize) -> c_int {
    let target = path(data, length);
    let _ = fs::create_dir_all(target);
    c_int::from(target.is_dir())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_list_directory(data: *const c_char, length: usize) -> AstralRustString {
    AstralRustString::list(&list_directory(path(data, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_join_path(
    base: *const c_char,
    base_length: usize,
    component: *const c_char,
    component_length: usize,
) -> AstralRustString {
    AstralRustString::new(join_path(bytes(base, base_length), bytes(component, component_length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_resolve_inside_directory(
    directory: *const c_char,
    directory_length: usize,
    relative: *const c_char,
    relative_length: usize,
    into: *mut AstralRustString,
) -> c_int {
    let resolved = resolve_inside(bytes(directory, directory_length), bytes(relative, relative_length));
    c_int::from(unsafe { give(resolved, into) })
}
