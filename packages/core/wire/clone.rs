use core::ffi::{CStr, c_char, c_int};

use super::cursor::Cursor;
use super::format::Span;
use super::out::Out;

const MAX_PROPS: usize = 128;
const SPAN_HEADER: usize = 5;

#[derive(Clone)]
pub enum Edit {
    Keep,
    Drop,
    Text(Vec<u8>),
    Number(f64),
    Integer(i32),
    Flag(bool),
}

#[derive(Clone)]
pub struct Change {
    pub property: u16,
    pub edit: Edit,
}

impl Change {
    pub fn text(property: u16, value: &[u8]) -> Self {
        Change {
            property,
            edit: Edit::Text(value.to_vec()),
        }
    }

    pub fn number(property: u16, value: f64) -> Self {
        Change {
            property,
            edit: Edit::Number(value),
        }
    }

    pub fn drop(property: u16) -> Self {
        Change {
            property,
            edit: Edit::Drop,
        }
    }
}

#[repr(C)]
pub struct CChange {
    pub property: u16,
    pub edit: c_int,
    pub text: *const c_char,
    pub number: f64,
}

fn from_c(change: &CChange) -> Change {
    let edit = match change.edit {
        1 => Edit::Drop,
        2 if !change.text.is_null() => Edit::Text(unsafe { CStr::from_ptr(change.text) }.to_bytes().to_vec()),
        2 => Edit::Text(Vec::new()),
        3 => Edit::Number(change.number),
        4 => Edit::Integer(change.number as i32),
        5 => Edit::Flag(change.number != 0.0),
        _ => Edit::Keep,
    };
    Change {
        property: change.property,
        edit,
    }
}

fn put_value(out: &mut Out, edit: &Edit) {
    match edit {
        Edit::Text(text) => {
            out.put8(6);
            out.put_string(text);
        }
        Edit::Flag(value) => out.put8(if *value { 2 } else { 1 }),
        Edit::Integer(value) => {
            out.put8(3);
            out.put(&value.to_le_bytes());
        }
        Edit::Number(value) => {
            out.put8(5);
            out.put(&value.to_le_bytes());
        }
        Edit::Keep | Edit::Drop => {}
    }
}

pub fn clone_creation(out: &mut Out, bytes: &[u8], span: &Span, id: i32, changes: &[Change]) {
    let mut cursor = Cursor::new(bytes, span.start + SPAN_HEADER);
    let type_length = cursor.read16() as usize;
    let type_name = cursor.take(type_length);
    let count = cursor.read16();
    let Some(type_name) = type_name.filter(|_| !cursor.failed()) else {
        out.failed = 1;
        return;
    };
    out.put8(1);
    out.put(&id.to_le_bytes());
    out.put16(type_length as u16);
    out.put(type_name);
    let count_at = out.length;
    out.put16(0);
    let mut written: u16 = 0;
    let mut seen = vec![false; changes.len().min(MAX_PROPS)];
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let start = cursor.offset();
        let property = cursor.read16();
        let tag = cursor.read8();
        cursor.skip_value(tag, 0);
        let change = changes.iter().position(|change| change.property == property);
        if let Some(index) = change
            && index < seen.len()
        {
            seen[index] = true;
        }
        match change.map(|index| &changes[index].edit) {
            Some(Edit::Drop) => continue,
            None if tag == 12 => continue,
            Some(edit @ (Edit::Text(_) | Edit::Number(_) | Edit::Integer(_) | Edit::Flag(_))) => {
                out.put16(property);
                put_value(out, edit);
            }
            _ => out.put(&bytes[start..cursor.offset().min(bytes.len())]),
        }
        written += 1;
    }
    for (index, change) in changes.iter().enumerate().take(MAX_PROPS) {
        if seen[index] || matches!(change.edit, Edit::Drop | Edit::Keep) {
            continue;
        }
        out.put16(change.property);
        put_value(out, &change.edit);
        written += 1;
    }
    if cursor.failed() {
        out.failed = 1;
    }
    if out.failed == 0 {
        unsafe { core::ptr::copy_nonoverlapping(written.to_le_bytes().as_ptr(), out.bytes.add(count_at), 2) };
    }
}

#[unsafe(export_name = "wireCloneCreation")]
pub unsafe extern "C" fn wire_clone_creation(
    out: *mut Out,
    bytes: *const u8,
    span: *const Span,
    id: i32,
    changes: *const CChange,
    count: usize,
) {
    let (Some(out), Some(span)) = (unsafe { out.as_mut() }, unsafe { span.as_ref() }) else {
        return;
    };
    let changes: Vec<Change> = if changes.is_null() || count == 0 {
        Vec::new()
    } else {
        unsafe { core::slice::from_raw_parts(changes, count) }.iter().map(from_c).collect()
    };
    clone_creation(out, super::format::batch_of(bytes, span), span, id, &changes);
}
