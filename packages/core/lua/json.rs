use core::ffi::{c_char, c_int};

use serde_json::{Map, Number, Value};

use crate::lua::api::{self as lua, LuaState};
use crate::support::bridge::{AstralRustString, bytes};

pub const MAXIMUM_TABLE_DEPTH: i32 = 8;
const ARRAY_MARKER: &[u8] = b"astral.json.array\0";

pub unsafe fn push_array_metatable(state: *mut LuaState) {
    unsafe {
        if lua::lua_getfield(state, lua::REGISTRYINDEX, ARRAY_MARKER.as_ptr().cast()) == lua::TTABLE {
            return;
        }
        lua::pop(state, 1);
        lua::new_table(state);
        lua::lua_pushvalue(state, -1);
        lua::lua_setfield(state, lua::REGISTRYINDEX, ARRAY_MARKER.as_ptr().cast());
    }
}

unsafe fn marked_array(state: *mut LuaState, index: c_int) -> bool {
    unsafe {
        if lua::lua_getmetatable(state, index) == 0 {
            return false;
        }
        push_array_metatable(state);
        let marked = lua::lua_rawequal(state, -1, -2) != 0;
        lua::pop(state, 2);
        marked
    }
}

unsafe fn is_array(state: *mut LuaState, index: c_int, length: lua::Integer) -> bool {
    unsafe {
        lua::lua_pushnil(state);
        while lua::lua_next(state, index) != 0 {
            let position = (lua::lua_isinteger(state, -2) != 0).then(|| lua::to_integer(state, -2));
            lua::pop(state, 1);
            if !position.is_some_and(|position| (1..=length).contains(&position)) {
                lua::pop(state, 1);
                return false;
            }
        }
        true
    }
}

pub unsafe fn to_json(state: *mut LuaState, index: c_int, depth: i32) -> Option<Value> {
    unsafe {
        let index = lua::lua_absindex(state, index);
        match lua::lua_type(state, index) {
            lua::TNIL => Some(Value::Null),
            lua::TBOOLEAN => Some(Value::Bool(lua::lua_toboolean(state, index) != 0)),
            lua::TNUMBER if lua::lua_isinteger(state, index) != 0 => Some(Value::from(lua::to_integer(state, index))),
            lua::TNUMBER => Some(Number::from_f64(lua::to_number(state, index)).map_or(Value::Null, Value::Number)),
            lua::TSTRING => lua::to_bytes(state, index).map(|text| Value::String(String::from_utf8_lossy(text).into_owned())),
            lua::TTABLE => {
                if depth > MAXIMUM_TABLE_DEPTH {
                    return None;
                }
                let length = lua::lua_rawlen(state, index) as lua::Integer;
                if (length > 0 || marked_array(state, index)) && is_array(state, index, length) {
                    let mut items = Vec::with_capacity(length as usize);
                    for position in 1..=length {
                        lua::lua_rawgeti(state, index, position);
                        let item = to_json(state, -1, depth + 1);
                        lua::pop(state, 1);
                        items.push(item?);
                    }
                    return Some(Value::Array(items));
                }
                let mut object = Map::new();
                lua::lua_pushnil(state);
                while lua::lua_next(state, index) != 0 {
                    let key = (lua::lua_type(state, -2) == lua::TSTRING)
                        .then(|| lua::to_bytes(state, -2))
                        .flatten()
                        .map(|key| String::from_utf8_lossy(key).into_owned());
                    let item = key.as_ref().and_then(|_| to_json(state, -1, depth + 1));
                    let (Some(key), Some(item)) = (key, item) else {
                        lua::pop(state, 2);
                        return None;
                    };
                    object.insert(key, item);
                    lua::pop(state, 1);
                }
                Some(Value::Object(object))
            }
            _ => None,
        }
    }
}

pub unsafe fn push_json(state: *mut LuaState, value: &Value, depth: i32) {
    unsafe {
        match value {
            _ if depth > MAXIMUM_TABLE_DEPTH => lua::lua_pushnil(state),
            Value::Null => lua::lua_pushnil(state),
            Value::Bool(flag) => lua::lua_pushboolean(state, c_int::from(*flag)),
            Value::Number(number) => match number.as_i64() {
                Some(integer) => lua::lua_pushinteger(state, integer),
                None => lua::lua_pushnumber(state, number.as_f64().unwrap_or(0.0)),
            },
            Value::String(text) => {
                lua::lua_pushlstring(state, text.as_ptr().cast(), text.len());
            }
            Value::Array(items) => {
                lua::lua_createtable(state, items.len() as c_int, 0);
                for (position, item) in items.iter().enumerate() {
                    push_json(state, item, depth + 1);
                    lua::lua_rawseti(state, -2, position as lua::Integer + 1);
                }
                push_array_metatable(state);
                lua::lua_setmetatable(state, -2);
            }
            Value::Object(object) => {
                lua::lua_createtable(state, 0, object.len() as c_int);
                for (key, item) in object {
                    let Ok(key) = std::ffi::CString::new(key.as_str()) else { continue };
                    push_json(state, item, depth + 1);
                    lua::lua_setfield(state, -2, key.as_ptr());
                }
            }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_to_json(state: *mut LuaState, index: c_int, depth: c_int) -> AstralRustString {
    match unsafe { to_json(state, index, depth) } {
        Some(value) => AstralRustString::new(value.to_string().into_bytes()),
        None => AstralRustString {
            bytes: core::ptr::null_mut(),
            length: 0,
        },
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_push_json(state: *mut LuaState, json: *const c_char, length: usize, depth: c_int) {
    let value: Value = serde_json::from_slice(bytes(json, length)).unwrap_or(Value::Null);
    unsafe { push_json(state, &value, depth) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_push_array_metatable(state: *mut LuaState) {
    unsafe { push_array_metatable(state) };
}
