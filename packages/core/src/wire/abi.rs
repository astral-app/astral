use core::ffi::{CStr, c_char, c_int};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Callback(AtomicUsize);

impl Callback {
    pub const fn new() -> Self {
        Callback(AtomicUsize::new(0))
    }

    pub fn set<F: Copy>(&self, function: Option<F>) {
        assert_eq!(core::mem::size_of::<F>(), core::mem::size_of::<usize>());
        let raw = function.map_or(0, |function| unsafe { core::mem::transmute_copy::<F, usize>(&function) });
        self.0.store(raw, Ordering::Release);
    }

    pub fn get<F: Copy>(&self) -> Option<F> {
        assert_eq!(core::mem::size_of::<F>(), core::mem::size_of::<usize>());
        let raw = self.0.load(Ordering::Acquire);
        (raw != 0).then(|| unsafe { core::mem::transmute_copy::<usize, F>(&raw) })
    }
}

impl Default for Callback {
    fn default() -> Self {
        Callback::new()
    }
}

#[repr(C)]
pub struct AstralMentionContext {
    pub message_id: u64,
    pub channel_id: u64,
    pub text: *const c_char,
    pub index: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AstralMentionDecoration {
    pub image: [c_char; 512],
    pub round: c_int,
    pub hide_at: c_int,
    pub after: c_int,
    pub color: u32,
}

impl Default for AstralMentionDecoration {
    fn default() -> Self {
        AstralMentionDecoration {
            image: [0; 512],
            round: 0,
            hide_at: 0,
            after: 0,
            color: 0,
        }
    }
}

impl AstralMentionDecoration {
    pub fn image(&self) -> &[u8] {
        let mut image = self.image;
        image[511] = 0;
        let length = image.iter().position(|byte| *byte == 0).unwrap_or(0);
        unsafe { core::slice::from_raw_parts(self.image.as_ptr().cast::<u8>(), length) }
    }
}

pub const NAME_AVATAR: c_int = 0;
pub const NAME_HEADER: c_int = 1;
pub const NAME_VOTER: c_int = 2;
pub const NAME_TYPING: c_int = 3;

#[repr(C)]
pub struct AstralNameContext {
    pub kind: c_int,
    pub text: *const c_char,
    pub user_id: u64,
    pub channel_id: u64,
}

unsafe extern "C" {
    pub fn astral_wire_current_channel() -> u64;
    pub fn wirePrefixed(text: *const u8, length: usize, prefix: *const c_char, suffix: *mut u64) -> c_int;
    pub fn wireAvatarUser(text: *const u8, length: usize) -> u64;
}

pub fn current_channel() -> u64 {
    unsafe { astral_wire_current_channel() }
}

pub fn prefixed(text: &[u8], prefix: &CStr) -> Option<u64> {
    let mut suffix = 0;
    (unsafe { wirePrefixed(text.as_ptr(), text.len(), prefix.as_ptr(), &mut suffix) } != 0).then_some(suffix)
}

pub fn avatar_user(text: &[u8]) -> u64 {
    unsafe { wireAvatarUser(text.as_ptr(), text.len()) }
}
