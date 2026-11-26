use core::ffi::c_char;

use crate::support::bridge::{AstralRustString, bytes, path};
use crate::support::files::list_directory;

const MOD_FOLDERS: [(&[u8], &str); 7] = [
    (b"enmity", "Enmity"),
    (b"vendetta", "Vendetta"),
    (b"pyoncord", "Bunny"),
    (b"bunny", "Bunny"),
    (b"revenge", "Revenge"),
    (b"kettu", "Kettu"),
    (b"aliucord", "Aliucord"),
];

pub fn leftover_mods(entries: &[Vec<u8>]) -> Vec<&'static str> {
    let mut names = Vec::new();
    for entry in entries {
        let folder = entry.to_ascii_lowercase();
        for (key, name) in MOD_FOLDERS {
            if folder == key && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

pub fn marker(directory: &std::path::Path) -> String {
    let names = leftover_mods(&list_directory(directory));
    if names.is_empty() {
        return String::new();
    }
    format!("leftover data from {}", names.join(", "))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_leftover_mods(entries: *const c_char, length: usize) -> AstralRustString {
    let entries: Vec<Vec<u8>> = bytes(entries, length)
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(<[u8]>::to_vec)
        .collect();
    let names: Vec<Vec<u8>> = leftover_mods(&entries).into_iter().map(|name| name.as_bytes().to_vec()).collect();
    AstralRustString::list(&names)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_leftover_mods_marker(directory: *const c_char, length: usize) -> AstralRustString {
    AstralRustString::new(marker(path(directory, length)).into_bytes())
}
