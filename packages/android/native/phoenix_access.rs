use std::io::Read;
use std::sync::OnceLock;

use astral_core::diagnostics::{phoenix_elf, phoenix_gate::RETURN_TRUE};

const LIBRARY: &str = "liblibdiscord_wasm_host.so";
const ENTRY: &str = "lib/arm64-v8a/liblibdiscord_wasm_host.so";
static ENTRIES: OnceLock<Vec<phoenix_elf::GateSite>> = OnceLock::new();

pub fn prepare(directory: &str, apks: &[String]) -> Result<String, String> {
    if ENTRIES.get().is_some() {
        return Ok("ready".into());
    }
    let installed = std::path::Path::new(directory).join(LIBRARY);
    let bytes = if installed.is_file() {
        std::fs::read(installed).map_err(|error| error.to_string())?
    } else {
        let mut found = None;
        for apk in apks {
            let file = std::fs::File::open(apk).map_err(|error| error.to_string())?;
            let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
            let Ok(mut entry) = archive.by_name(ENTRY) else {
                continue;
            };
            if entry.size() > 512 << 20 {
                return Err("Phoenix host exceeds 512 MiB".into());
            }
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            found = Some(bytes);
            break;
        }
        found.ok_or("arm64 Phoenix host not found")?
    };
    let entries = phoenix_elf::patch_plan(&bytes)?;
    if entries.len() != 2 {
        return Err("Phoenix gate did not resolve to two evaluators".into());
    }
    let _ = ENTRIES.set(entries);
    Ok("ready".into())
}

#[cfg(target_os = "android")]
pub fn open() -> Result<String, String> {
    use core::ffi::{CStr, c_void};
    use std::collections::BTreeSet;
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());
    let _lock = LOCK.lock().map_err(|error| error.to_string())?;
    let entries = ENTRIES.get().ok_or("Phoenix gate is not prepared")?;
    unsafe extern "C" {
        fn __clear_cache(start: *mut c_void, end: *mut c_void);
        fn mremap(
            old: *mut c_void,
            old_size: usize,
            new_size: usize,
            flags: i32,
            ...
        ) -> *mut c_void;
    }
    const MREMAP_MAYMOVE: i32 = 1;
    const MREMAP_FIXED: i32 = 2;
    unsafe extern "C" fn find(
        info: *mut libc::dl_phdr_info,
        _: usize,
        context: *mut c_void,
    ) -> i32 {
        let info = unsafe { &*info };
        if info.dlpi_name.is_null() {
            return 0;
        }
        let name = unsafe { CStr::from_ptr(info.dlpi_name) }.to_bytes();
        if !name.ends_with(LIBRARY.as_bytes()) {
            return 0;
        }
        let entries = unsafe { &mut *(context as *mut (Vec<u64>, Option<usize>)) };
        let headers =
            unsafe { core::slice::from_raw_parts(info.dlpi_phdr, usize::from(info.dlpi_phnum)) };
        if entries.0.iter().all(|entry| {
            headers.iter().any(|header| {
                header.p_type == libc::PT_LOAD
                    && header.p_flags & 1 != 0
                    && *entry >= header.p_vaddr
                    && entry
                        .checked_add(32)
                        .is_some_and(|end| end <= header.p_vaddr + header.p_memsz)
            })
        }) {
            entries.1 = Some(info.dlpi_addr as usize);
            return 1;
        }
        0
    }
    let mut context: (Vec<u64>, Option<usize>) = (
        entries.iter().map(|site| site.address).collect::<Vec<_>>(),
        None,
    );
    unsafe {
        libc::dl_iterate_phdr(Some(find), (&raw mut context).cast());
    }
    let base = context.1.ok_or("Phoenix host is not loaded")?;
    let addresses: Vec<usize> = entries
        .iter()
        .map(|entry| base + entry.address as usize)
        .collect();
    for (site, address) in entries.iter().zip(&addresses) {
        let current = unsafe { core::slice::from_raw_parts(*address as *const u8, 32) };
        if current[8..] != site.original[8..]
            || (current[..8] != site.original[..8] && current[..8] != RETURN_TRUE)
        {
            return Err("loaded Phoenix gate differs from its validated file".into());
        }
    }
    if addresses.iter().all(
        |address| unsafe { core::slice::from_raw_parts(*address as *const u8, 8) } == RETURN_TRUE,
    ) {
        return Ok("already-open".into());
    }
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return Err("invalid page size".into());
    }
    let page_size = page_size as usize;
    let pages: BTreeSet<_> = addresses
        .iter()
        .flat_map(|address| [address & !(page_size - 1), (address + 7) & !(page_size - 1)])
        .collect();
    let mut replacements = Vec::new();
    for page in pages {
        let replacement = unsafe {
            libc::mmap(
                core::ptr::null_mut(),
                page_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if replacement == libc::MAP_FAILED {
            let error = std::io::Error::last_os_error();
            for (_, replacement) in replacements {
                unsafe {
                    libc::munmap(replacement, page_size);
                }
            }
            return Err(format!("Phoenix gate page allocation refused: {error}"));
        }
        unsafe {
            core::ptr::copy_nonoverlapping(page as *const u8, replacement.cast::<u8>(), page_size);
        }
        for address in &addresses {
            let start = (*address).max(page);
            let end = (address + RETURN_TRUE.len()).min(page + page_size);
            if start >= end {
                continue;
            }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    RETURN_TRUE.as_ptr().add(start - address),
                    replacement.cast::<u8>().add(start - page),
                    end - start,
                );
            }
        }
        replacements.push((page, replacement));
    }
    for (_, replacement) in &replacements {
        let protected = unsafe {
            __clear_cache(*replacement, replacement.cast::<u8>().add(page_size).cast());
            libc::mprotect(*replacement, page_size, libc::PROT_READ | libc::PROT_EXEC)
        } == 0;
        if !protected {
            let error = std::io::Error::last_os_error();
            for (_, replacement) in replacements {
                unsafe { libc::munmap(replacement, page_size) };
            }
            return Err(format!("Phoenix gate execute protection refused: {error}"));
        }
    }
    for (index, (page, replacement)) in replacements.iter().enumerate() {
        let installed = unsafe {
            mremap(
                *replacement,
                page_size,
                page_size,
                MREMAP_MAYMOVE | MREMAP_FIXED,
                *page as *mut c_void,
            )
        };
        if installed == libc::MAP_FAILED {
            let error = std::io::Error::last_os_error();
            for (_, replacement) in &replacements[index..] {
                unsafe { libc::munmap(*replacement, page_size) };
            }
            return Err(format!("Phoenix gate page replacement refused: {error}"));
        }
    }
    Ok("opened".into())
}

#[cfg(not(target_os = "android"))]
pub fn open() -> Result<String, String> {
    let _ = RETURN_TRUE;
    Err("Phoenix runtime gate requires Android".into())
}
