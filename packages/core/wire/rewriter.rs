use core::ffi::{CStr, c_char, c_int};
use std::ffi::CString;
use std::sync::atomic::{AtomicUsize, Ordering};

pub type ImageRewriter = unsafe extern "C" fn(*const c_char, *mut c_char, usize) -> c_int;

const MAX_URI: usize = 1024;

pub struct Slot(AtomicUsize);

impl Slot {
    pub const fn new() -> Self {
        Slot(AtomicUsize::new(0))
    }

    pub fn set(&self, rewriter: Option<ImageRewriter>) {
        self.0.store(rewriter.map_or(0, |function| function as usize), Ordering::Release);
    }

    pub fn get(&self) -> Option<ImageRewriter> {
        let raw = self.0.load(Ordering::Acquire);
        (raw != 0).then(|| unsafe { core::mem::transmute::<usize, ImageRewriter>(raw) })
    }

    pub fn rewrite(&self, uri: &[u8]) -> Option<Vec<u8>> {
        let rewriter = self.get()?;
        if uri.len() >= MAX_URI {
            return None;
        }
        let original = CString::new(uri).ok()?;
        let mut rewritten = [0 as c_char; MAX_URI];
        if unsafe { rewriter(original.as_ptr(), rewritten.as_mut_ptr(), MAX_URI) } == 0 {
            return None;
        }
        rewritten[MAX_URI - 1] = 0;
        let result = unsafe { CStr::from_ptr(rewritten.as_ptr()) }.to_bytes();
        (result != uri).then(|| result.to_vec())
    }
}

impl Default for Slot {
    fn default() -> Self {
        Slot::new()
    }
}
