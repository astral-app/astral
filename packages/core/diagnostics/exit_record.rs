use core::ffi::{c_char, c_int};
use std::path::PathBuf;

use crate::support::bridge::path;
use crate::support::files::{remove, write_atomically};

const FILE: &str = "exit-record";
pub const EXIT_UNKNOWN: c_int = 0;
pub const EXIT_CRASHED: c_int = 1;
pub const EXIT_ENDED: c_int = 2;

fn record(directory: *const c_char, length: usize) -> PathBuf {
    path(directory, length).join(FILE)
}

fn clear(record: &std::path::Path) -> bool {
    !record.exists() || remove(record)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_exit_record_mark_ended(directory: *const c_char, length: usize) -> c_int {
    c_int::from(write_atomically(&record(directory, length), b"ended"))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_exit_record_clear(directory: *const c_char, length: usize) -> c_int {
    c_int::from(clear(&record(directory, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_exit_record_take(directory: *const c_char, length: usize, native_crash: c_int) -> c_int {
    let record = record(directory, length);
    let ended = record.exists();
    clear(&record);
    if native_crash != 0 {
        return EXIT_CRASHED;
    }
    if ended { EXIT_ENDED } else { EXIT_UNKNOWN }
}
