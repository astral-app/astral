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

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Tracked {
    pub id: i32,
    pub kind: u8,
    pub value: u64,
}

pub const LABEL_AUTHOR: c_int = 0;
pub const LABEL_REPLY: c_int = 1;
pub const LABEL_CHANNEL: c_int = 2;
pub const LABEL_MEMBER: c_int = 3;
pub const LABEL_COMPOSER: c_int = 4;
pub const LABEL_PROFILE: c_int = 5;

#[repr(C)]
pub struct AstralLabelContext {
    pub kind: c_int,
    pub message_id: u64,
    pub channel_id: u64,
    pub target_id: u64,
}

pub const PATCH_BACKGROUND: u32 = 1 << 0;
pub const PATCH_RADIUS: u32 = 1 << 1;
pub const PATCH_OPACITY: u32 = 1 << 2;
pub const PATCH_PADDING: u32 = 1 << 3;
pub const PATCH_MARGIN: u32 = 1 << 4;
pub const PATCH_BORDER: u32 = 1 << 5;
pub const PATCH_TEXT_COLOR: u32 = 1 << 6;
pub const PATCH_HIDDEN: u32 = 1 << 7;
pub const PATCH_TEXT_SCALE: u32 = 1 << 8;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AstralNodePatch {
    pub fields: u32,
    pub background: u32,
    pub radius: f64,
    pub opacity: f64,
    pub padding: [f32; 4],
    pub margin: [f32; 4],
    pub border_color: u32,
    pub border_width: f64,
    pub text_color: u32,
    pub text_scale: f64,
}

#[repr(C)]
pub struct AstralPatchTarget {
    pub identifier: *const c_char,
    pub kind: *const c_char,
    pub key: usize,
    pub message_id: u64,
    pub author_id: u64,
    pub channel_id: u64,
}

pub fn tracked_author(id: i32) -> Option<u64> {
    super::decorate::tracked_value(id, super::decorate::TRACK_AUTHOR)
}

pub use super::decorate::{wireMessageAuthor, wireRowMessage};

pub use super::ids::{avatar_user, current_channel};

pub fn prefixed(text: &[u8], prefix: &CStr) -> Option<u64> {
    super::ids::prefixed(text, prefix.to_bytes())
}
