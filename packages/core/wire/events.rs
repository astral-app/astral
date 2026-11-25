use super::cursor::Cursor;
use super::properties;

const MAX_PROPS: u32 = 128;
const EVENT_MAP: u8 = 8;

pub fn event_value(bytes: &[u8]) -> Option<f64> {
    let wanted = properties::current().value;
    let mut cursor = Cursor::new(bytes, 0);
    if bytes.len() < 5 || cursor.read8() != EVENT_MAP {
        return None;
    }
    let count = cursor.read32();
    if count > MAX_PROPS {
        return None;
    }
    let mut found = None;
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let key = cursor.read16();
        let tag = cursor.read8();
        if key != wanted {
            cursor.skip_value(tag, 0);
            continue;
        }
        if found.is_some() {
            return None;
        }
        let number = match tag {
            3 => cursor.read32() as i32 as f64,
            4 => i64::from_le_bytes(cursor.take(8)?.try_into().ok()?) as f64,
            5 => f64::from_le_bytes(cursor.take(8)?.try_into().ok()?),
            _ => return None,
        };
        found = Some(number);
    }
    let number = found?;
    (!cursor.failed() && cursor.offset() == bytes.len() && number.is_finite()).then_some(number)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_event_value(bytes: *const u8, length: usize, value: *mut f64) -> core::ffi::c_int {
    if bytes.is_null() || value.is_null() {
        return 0;
    }
    match event_value(unsafe { core::slice::from_raw_parts(bytes, length) }) {
        Some(number) => {
            unsafe { *value = number };
            1
        }
        None => 0,
    }
}
