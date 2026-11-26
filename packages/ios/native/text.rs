#[repr(C)]
#[derive(Clone, Copy)]
pub struct AstralText {
    pub bytes: *const u8,
    pub length: usize,
}

#[repr(C)]
pub struct AstralOwned {
    pub bytes: *mut u8,
    pub length: usize,
}

impl AstralText {
    pub const ABSENT: AstralText = AstralText {
        bytes: core::ptr::null(),
        length: 0,
    };

    pub fn of(bytes: &[u8]) -> AstralText {
        AstralText {
            bytes: bytes.as_ptr(),
            length: bytes.len(),
        }
    }

    pub fn present(self) -> bool {
        !self.bytes.is_null()
    }

    pub fn bytes<'a>(self) -> &'a [u8] {
        if self.bytes.is_null() || self.length == 0 {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(self.bytes, self.length) }
    }

    pub fn text(self) -> String {
        String::from_utf8_lossy(self.bytes()).into_owned()
    }

    pub fn optional(self) -> Option<String> {
        self.present().then(|| self.text())
    }
}

impl AstralOwned {
    pub const NONE: AstralOwned = AstralOwned {
        bytes: core::ptr::null_mut(),
        length: 0,
    };

    pub fn new(bytes: Vec<u8>) -> AstralOwned {
        let mut boxed = bytes.into_boxed_slice();
        let owned = AstralOwned {
            bytes: if boxed.is_empty() {
                core::ptr::NonNull::dangling().as_ptr()
            } else {
                boxed.as_mut_ptr()
            },
            length: boxed.len(),
        };
        core::mem::forget(boxed);
        owned
    }

    pub fn text(text: String) -> AstralOwned {
        AstralOwned::new(text.into_bytes())
    }

    pub fn optional(text: Option<String>) -> AstralOwned {
        text.map_or(AstralOwned::NONE, AstralOwned::text)
    }

    pub fn release(self) {
        if self.bytes.is_null() || self.length == 0 {
            return;
        }
        drop(unsafe {
            Box::from_raw(core::ptr::slice_from_raw_parts_mut(self.bytes, self.length))
        });
    }
}
