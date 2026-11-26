use crate::wire::tag::Tag;
use core::ffi::{c_char, c_int};

use crate::lua::api::{self as lua, LuaState};
use crate::lua::host;
use crate::lua::host::hooks::{busy, patch_color, running_plugins, set_text};

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
    failed: bool,
}

impl Reader<'_> {
    fn need(&mut self, count: usize) -> bool {
        if self.failed || self.bytes.len() - self.offset < count {
            self.failed = true;
        }
        !self.failed
    }

    fn u8(&mut self) -> u8 {
        if !self.need(1) {
            return 0;
        }
        self.offset += 1;
        self.bytes[self.offset - 1]
    }

    fn u16(&mut self) -> u16 {
        if !self.need(2) {
            return 0;
        }
        self.offset += 2;
        u16::from_ne_bytes([self.bytes[self.offset - 2], self.bytes[self.offset - 1]])
    }

    fn u32(&mut self) -> u32 {
        if !self.need(4) {
            return 0;
        }
        self.offset += 4;
        u32::from_ne_bytes(self.bytes[self.offset - 4..self.offset].try_into().unwrap_or_default())
    }

    fn advance(&mut self, count: usize) {
        if self.need(count) {
            self.offset += count;
        }
    }

    fn skip(&mut self, tag: u8, depth: u32) {
        if depth > 8 {
            self.failed = true;
            return;
        }
        match Tag::parse(tag) {
            Some(Tag::Null | Tag::False | Tag::True) => {}
            Some(Tag::Int) => self.advance(4),
            Some(Tag::Long | Tag::Double) => self.advance(8),
            Some(Tag::String) => {
                let size = self.u32() as usize;
                self.advance(size);
            }
            Some(Tag::Array) => {
                let count = self.u32();
                for _ in 0..count {
                    if self.failed {
                        break;
                    }
                    let tag = self.u8();
                    self.skip(tag, depth + 1);
                }
            }
            Some(Tag::Map) => {
                let count = self.u32();
                for _ in 0..count {
                    if self.failed {
                        break;
                    }
                    self.u16();
                    let tag = self.u8();
                    self.skip(tag, depth + 1);
                }
            }
            Some(Tag::Quad) => self.advance(16),
            Some(Tag::Words) => {
                let count = self.u32() as usize;
                self.advance(count * 4);
            }
            _ => self.failed = true,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Text,
    Color,
    Integer,
    Number,
    Flag,
}

fn fields() -> [(&'static str, u16, Kind); 12] {
    let properties = crate::wire::properties::current();
    [
        ("text", properties.text, Kind::Text),
        ("link", properties.link, Kind::Text),
        ("font", properties.font, Kind::Text),
        ("color", properties.color, Kind::Color),
        ("weight", properties.weight, Kind::Integer),
        ("bg", properties.bg, Kind::Integer),
        ("bg_radius", properties.bg_radius, Kind::Number),
        ("scale", properties.scale, Kind::Number),
        ("advance", properties.advance, Kind::Number),
        ("bold", properties.bold, Kind::Flag),
        ("strike", properties.strike, Kind::Flag),
        ("underline", properties.underline, Kind::Flag),
    ]
}

unsafe fn push_spans(state: *mut LuaState, bytes: &[u8]) -> bool {
    let mut reader = Reader {
        bytes,
        offset: 0,
        failed: false,
    };
    if reader.u8() != Tag::Array {
        return false;
    }
    let count = reader.u32();
    if count > 512 {
        return false;
    }
    let fields = fields();
    let background = crate::wire::properties::current().bg;
    unsafe { lua::lua_createtable(state, count as c_int, 0) };
    for index in 0..count {
        if reader.failed {
            break;
        }
        if reader.u8() != Tag::Map {
            return false;
        }
        let entries = reader.u32();
        unsafe { lua::lua_createtable(state, 0, 4) };
        let mut extra = Vec::new();
        let mut extra_count = 0i64;
        for _ in 0..entries {
            if reader.failed {
                break;
            }
            let start = reader.offset;
            let key = reader.u16();
            let tag = reader.u8();
            let field = fields.iter().find(|(_, candidate, _)| *candidate == key);
            let value = reader.offset;
            reader.skip(tag, 0);
            if reader.failed {
                return false;
            }
            let pushed = match field {
                Some((_, _, Kind::Text)) if tag == Tag::String => {
                    unsafe { lua::push_bytes(state, &bytes[value + 4..reader.offset]) };
                    true
                }
                Some((_, _, Kind::Color)) if tag == Tag::Long => {
                    let wide = i64::from_ne_bytes(bytes[value..value + 8].try_into().unwrap_or_default());
                    unsafe { lua::lua_pushinteger(state, i64::from(wide as u32)) };
                    true
                }
                Some((_, field_key, Kind::Integer)) if tag == Tag::Int => {
                    let integer = i32::from_ne_bytes(bytes[value..value + 4].try_into().unwrap_or_default());
                    let pushed = if *field_key == background {
                        i64::from(integer as u32)
                    } else {
                        i64::from(integer)
                    };
                    unsafe { lua::lua_pushinteger(state, pushed) };
                    true
                }
                Some((_, _, Kind::Number)) if tag == Tag::Double => {
                    unsafe { lua::lua_pushnumber(state, f64::from_ne_bytes(bytes[value..value + 8].try_into().unwrap_or_default())) };
                    true
                }
                Some((_, _, Kind::Flag)) if tag == Tag::False || tag == Tag::True => {
                    unsafe { lua::lua_pushboolean(state, c_int::from(tag == Tag::True)) };
                    true
                }
                _ => false,
            };
            if pushed {
                unsafe { lua::set_field(state, -2, field.map_or("", |(name, _, _)| name)) };
            } else {
                extra.extend_from_slice(&bytes[start..reader.offset]);
                extra_count += 1;
            }
        }
        if extra_count > 0 {
            unsafe {
                lua::push_bytes(state, &extra);
                lua::set_field(state, -2, "_raw");
                lua::lua_pushinteger(state, extra_count);
                lua::set_field(state, -2, "_raw_count");
            }
        }
        unsafe { lua::lua_rawseti(state, -2, i64::from(index) + 1) };
    }
    !reader.failed
}

unsafe fn encode_spans(state: *mut LuaState, index: c_int) -> Option<Vec<u8>> {
    let index = unsafe { lua::lua_absindex(state, index) };
    if !unsafe { lua::is_table(state, index) } {
        return None;
    }
    let count = unsafe { lua::luaL_len(state, index) };
    if !(0..=512).contains(&count) {
        return None;
    }
    let fields = fields();
    let mut out = vec![Tag::Array as u8];
    out.extend_from_slice(&(count as u32).to_ne_bytes());
    for position in 1..=count {
        unsafe { lua::lua_rawgeti(state, index, position) };
        if !unsafe { lua::is_table(state, -1) } {
            unsafe { lua::pop(state, 1) };
            return None;
        }
        let mut entries = Vec::new();
        let mut entry_count: u32 = 0;
        for (name, key, kind) in fields {
            unsafe { lua::get_field(state, -1, name) };
            if unsafe { lua::lua_type(state, -1) } != lua::TNIL {
                match kind {
                    Kind::Text => {
                        if unsafe { lua::lua_type(state, -1) } == lua::TSTRING
                            && let Some(text) = unsafe { lua::to_bytes(state, -1) }.filter(|text| text.len() <= 8192)
                        {
                            entries.extend_from_slice(&key.to_ne_bytes());
                            entries.push(Tag::String as u8);
                            entries.extend_from_slice(&(text.len() as u32).to_ne_bytes());
                            entries.extend_from_slice(text);
                            entry_count += 1;
                        }
                    }
                    Kind::Color => {
                        unsafe { lua::pop(state, 1) };
                        if let Some(color) = unsafe { patch_color(state, name) } {
                            entries.extend_from_slice(&key.to_ne_bytes());
                            entries.push(Tag::Long as u8);
                            entries.extend_from_slice(&i64::from(color).to_ne_bytes());
                            entry_count += 1;
                        }
                        unsafe { lua::lua_pushnil(state) };
                    }
                    Kind::Integer => {
                        if unsafe { lua::lua_isinteger(state, -1) } != 0 {
                            entries.extend_from_slice(&key.to_ne_bytes());
                            entries.push(Tag::Int as u8);
                            entries.extend_from_slice(&(unsafe { lua::to_integer(state, -1) } as i32).to_ne_bytes());
                            entry_count += 1;
                        }
                    }
                    Kind::Number => {
                        let value = unsafe { lua::to_number(state, -1) };
                        if unsafe { lua::lua_isnumber(state, -1) } != 0 && value.is_finite() {
                            entries.extend_from_slice(&key.to_ne_bytes());
                            entries.push(Tag::Double as u8);
                            entries.extend_from_slice(&value.to_ne_bytes());
                            entry_count += 1;
                        }
                    }
                    Kind::Flag => {
                        entries.extend_from_slice(&key.to_ne_bytes());
                        entries.push(Tag::flag(unsafe { lua::lua_toboolean(state, -1) } != 0) as u8);
                        entry_count += 1;
                    }
                }
            }
            unsafe { lua::pop(state, 1) };
        }
        unsafe {
            lua::get_field(state, -1, "_raw");
            lua::get_field(state, -2, "_raw_count");
        }
        if unsafe { lua::lua_type(state, -2) } == lua::TSTRING && unsafe { lua::lua_isinteger(state, -1) } != 0 {
            entries.extend_from_slice(unsafe { lua::to_bytes(state, -2) }.unwrap_or_default());
            entry_count += unsafe { lua::to_integer(state, -1) } as u32;
        }
        unsafe { lua::pop(state, 3) };
        out.push(Tag::Map as u8);
        out.extend_from_slice(&entry_count.to_ne_bytes());
        out.extend_from_slice(&entries);
    }
    Some(out)
}

unsafe extern "C" {
    fn malloc(size: usize) -> *mut core::ffi::c_void;
}

#[unsafe(export_name = "AstralLuaSpanTransform")]
pub unsafe extern "C" fn astral_lua_span_transform(
    message_id: u64,
    spans: *const u8,
    length: usize,
    transformed: *mut *mut u8,
    transformed_length: *mut usize,
) -> c_int {
    if spans.is_null() || transformed.is_null() || transformed_length.is_null() {
        return 0;
    }
    let Some(_held) = host::try_lock() else { return busy(-1) };
    let mut current = unsafe { core::slice::from_raw_parts(spans, length) }.to_vec();
    let mut changed = false;
    let message = message_id.to_string();
    let author = crate::wire::commands::astral_wire_message_author(message_id);
    for plugin in running_plugins() {
        let mut index = 0;
        while index < unsafe { (*plugin).text_transforms.len() } {
            let reference = unsafe { (&(*plugin).text_transforms)[index] };
            index += 1;
            let state = unsafe { (*plugin).state() };
            let top = unsafe { lua::lua_gettop(state) };
            unsafe { lua::push_reference(state, reference) };
            if !unsafe { push_spans(state, &current) } {
                unsafe { lua::lua_settop(state, top) };
                return c_int::from(changed);
            }
            unsafe {
                lua::lua_createtable(state, 0, 2);
                set_text(state, "message_id", &message);
                if author != 0 {
                    set_text(state, "author_id", &author.to_string());
                }
            }
            if !host::call(plugin, 2, 1, "text transform") {
                unsafe { lua::lua_settop(state, top) };
                host::fail_and_stop(plugin);
                break;
            }
            if let Some(next) = unsafe { encode_spans(state, -1) }.filter(|next| *next != current) {
                current = next;
                changed = true;
            }
            unsafe { lua::lua_settop(state, top) };
        }
    }
    if !changed {
        return 0;
    }
    let buffer = unsafe { malloc(current.len()) }.cast::<u8>();
    if buffer.is_null() {
        return 0;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(current.as_ptr(), buffer, current.len());
        *transformed = buffer;
        *transformed_length = current.len();
    }
    1
}

pub fn unused(_: *const c_char) {}
