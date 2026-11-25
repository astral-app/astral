use core::ffi::{c_char, c_int};

use crate::bridge::{AstralRustString, bytes};

const MAGIC: &[u8; 4] = b"UBSC";
const VERSION: u64 = 1;

#[repr(C)]
pub struct AstralCachedStore {
    pub name: *const c_char,
    pub name_length: usize,
    pub json_offset: u64,
    pub json_length: u64,
    pub packed: *const c_char,
    pub packed_length: usize,
}

pub fn settings_hash(json: &[u8]) -> u64 {
    json.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn put(out: &mut Vec<u8>, value: u64, width: usize) {
    out.extend_from_slice(&value.to_le_bytes()[..width]);
}

struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, length: u64) -> Option<&'a [u8]> {
        let length = usize::try_from(length).ok()?;
        let slice = self.data.get(self.offset..self.offset.checked_add(length)?)?;
        self.offset += length;
        Some(slice)
    }

    fn integer(&mut self, width: usize) -> Option<u64> {
        let slice = self.bytes(width as u64)?;
        Some(slice.iter().rev().fold(0, |value, byte| (value << 8) | u64::from(*byte)))
    }
}

pub struct Decoded {
    pub name: Vec<u8>,
    pub offset: u64,
    pub length: u64,
    pub packed: Vec<u8>,
}

pub fn decode(cache: &[u8], json_hash: u64, json_size: u64) -> Option<Vec<Decoded>> {
    let mut reader = Reader { data: cache, offset: 0 };
    if reader.bytes(4)? != MAGIC || reader.integer(4)? != VERSION || reader.integer(8)? != json_hash || reader.integer(8)? != json_size {
        return None;
    }
    let count = reader.integer(4)?;
    let mut stores = Vec::new();
    let mut packed_lengths = Vec::new();
    for _ in 0..count {
        let name_length = reader.integer(4)?;
        let name = reader.bytes(name_length)?.to_vec();
        let offset = reader.integer(8)?;
        let length = reader.integer(8)?;
        let packed_length = reader.integer(8)?;
        if offset > json_size || length > json_size - offset {
            return None;
        }
        stores.push(Decoded {
            name,
            offset,
            length,
            packed: Vec::new(),
        });
        packed_lengths.push(packed_length);
    }
    for (store, length) in stores.iter_mut().zip(packed_lengths) {
        store.packed = reader.bytes(length)?.to_vec();
    }
    (reader.offset == cache.len()).then_some(stores)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_hash(json: *const c_char, length: usize) -> u64 {
    settings_hash(bytes(json, length))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_encode_settings_cache(
    json_hash: u64,
    json_size: u64,
    stores: *const AstralCachedStore,
    count: usize,
) -> AstralRustString {
    let stores = if stores.is_null() || count == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(stores, count) }
    };
    let mut out = MAGIC.to_vec();
    put(&mut out, VERSION, 4);
    put(&mut out, json_hash, 8);
    put(&mut out, json_size, 8);
    put(&mut out, stores.len() as u64, 4);
    for store in stores {
        put(&mut out, store.name_length as u64, 4);
        out.extend_from_slice(bytes(store.name, store.name_length));
        put(&mut out, store.json_offset, 8);
        put(&mut out, store.json_length, 8);
        put(&mut out, store.packed_length as u64, 8);
    }
    for store in stores {
        out.extend_from_slice(bytes(store.packed, store.packed_length));
    }
    AstralRustString::new(out)
}

pub type DecodedVisitor = unsafe extern "C" fn(*mut core::ffi::c_void, *const c_char, usize, u64, u64, *const c_char, usize);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_decode_settings_cache(
    cache: *const c_char,
    length: usize,
    json_hash: u64,
    json_size: u64,
    visit: DecodedVisitor,
    context: *mut core::ffi::c_void,
) -> c_int {
    let Some(stores) = decode(bytes(cache, length), json_hash, json_size) else {
        return 0;
    };
    for store in &stores {
        unsafe {
            visit(
                context,
                store.name.as_ptr().cast(),
                store.name.len(),
                store.offset,
                store.length,
                store.packed.as_ptr().cast(),
                store.packed.len(),
            )
        };
    }
    1
}
