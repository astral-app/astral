use core::ffi::{c_char, c_int, c_void};

pub enum LuaState {}
pub enum LuaDebug {}

pub type Integer = i64;
pub type Number = f64;
pub type Alloc = unsafe extern "C" fn(*mut c_void, *mut c_void, usize, usize) -> *mut c_void;
pub type Hook = unsafe extern "C" fn(*mut LuaState, *mut LuaDebug);
pub type CFunction = unsafe extern "C" fn(*mut LuaState) -> c_int;

pub const OK: c_int = 0;
pub const TNIL: c_int = 0;
pub const TBOOLEAN: c_int = 1;
pub const TNUMBER: c_int = 3;
pub const TSTRING: c_int = 4;
pub const TTABLE: c_int = 5;
pub const MASKCOUNT: c_int = 1 << 3;
pub const REGISTRYINDEX: c_int = -1_000_000 - 1000;

unsafe extern "C" {
    pub fn lua_newstate(allocate: Alloc, user: *mut c_void) -> *mut LuaState;
    pub fn lua_close(state: *mut LuaState);
    pub fn lua_getallocf(state: *mut LuaState, user: *mut *mut c_void) -> Alloc;
    pub fn lua_sethook(state: *mut LuaState, hook: Option<Hook>, mask: c_int, count: c_int);
    pub fn lua_settop(state: *mut LuaState, index: c_int);
    pub fn lua_absindex(state: *mut LuaState, index: c_int) -> c_int;
    pub fn lua_type(state: *mut LuaState, index: c_int) -> c_int;
    pub fn lua_toboolean(state: *mut LuaState, index: c_int) -> c_int;
    pub fn lua_tointegerx(state: *mut LuaState, index: c_int, is_number: *mut c_int) -> Integer;
    pub fn lua_tonumberx(state: *mut LuaState, index: c_int, is_number: *mut c_int) -> Number;
    pub fn lua_isinteger(state: *mut LuaState, index: c_int) -> c_int;
    pub fn lua_tolstring(state: *mut LuaState, index: c_int, length: *mut usize) -> *const c_char;
    pub fn lua_rawlen(state: *mut LuaState, index: c_int) -> u64;
    pub fn lua_getmetatable(state: *mut LuaState, index: c_int) -> c_int;
    pub fn lua_setmetatable(state: *mut LuaState, index: c_int) -> c_int;
    pub fn lua_getfield(state: *mut LuaState, index: c_int, key: *const c_char) -> c_int;
    pub fn lua_setfield(state: *mut LuaState, index: c_int, key: *const c_char);
    pub fn lua_setglobal(state: *mut LuaState, name: *const c_char);
    pub fn lua_pushvalue(state: *mut LuaState, index: c_int);
    pub fn lua_pushnil(state: *mut LuaState);
    pub fn lua_pushboolean(state: *mut LuaState, value: c_int);
    pub fn lua_pushinteger(state: *mut LuaState, value: Integer);
    pub fn lua_pushnumber(state: *mut LuaState, value: Number);
    pub fn lua_pushlstring(state: *mut LuaState, text: *const c_char, length: usize) -> *const c_char;
    pub fn lua_createtable(state: *mut LuaState, array: c_int, record: c_int);
    pub fn lua_rawequal(state: *mut LuaState, left: c_int, right: c_int) -> c_int;
    pub fn lua_next(state: *mut LuaState, index: c_int) -> c_int;
    pub fn lua_rawgeti(state: *mut LuaState, index: c_int, position: Integer) -> c_int;
    pub fn lua_rawseti(state: *mut LuaState, index: c_int, position: Integer);
    pub fn lua_touserdata(state: *mut LuaState, index: c_int) -> *mut c_void;
    pub fn lua_error(state: *mut LuaState) -> c_int;
    pub fn lua_pcallk(
        state: *mut LuaState,
        arguments: c_int,
        results: c_int,
        handler: c_int,
        context: isize,
        continuation: *const c_void,
    ) -> c_int;
    pub fn luaL_loadbufferx(state: *mut LuaState, buffer: *const c_char, size: usize, name: *const c_char, mode: *const c_char) -> c_int;
    pub fn luaL_requiref(state: *mut LuaState, name: *const c_char, open: CFunction, global: c_int);
    pub fn luaL_error(state: *mut LuaState, format: *const c_char, ...) -> c_int;
    pub fn luaopen_base(state: *mut LuaState) -> c_int;
    pub fn luaopen_table(state: *mut LuaState) -> c_int;
    pub fn luaopen_string(state: *mut LuaState) -> c_int;
    pub fn luaopen_math(state: *mut LuaState) -> c_int;
    pub fn luaopen_utf8(state: *mut LuaState) -> c_int;
    pub fn luaopen_coroutine(state: *mut LuaState) -> c_int;
}

pub unsafe fn pop(state: *mut LuaState, count: c_int) {
    unsafe { lua_settop(state, -count - 1) };
}

pub unsafe fn new_table(state: *mut LuaState) {
    unsafe { lua_createtable(state, 0, 0) };
}

pub unsafe fn to_integer(state: *mut LuaState, index: c_int) -> Integer {
    unsafe { lua_tointegerx(state, index, core::ptr::null_mut()) }
}

pub unsafe fn to_number(state: *mut LuaState, index: c_int) -> Number {
    unsafe { lua_tonumberx(state, index, core::ptr::null_mut()) }
}

pub unsafe fn to_bytes<'a>(state: *mut LuaState, index: c_int) -> Option<&'a [u8]> {
    let mut length = 0;
    let text = unsafe { lua_tolstring(state, index, &mut length) };
    (!text.is_null()).then(|| unsafe { core::slice::from_raw_parts(text.cast::<u8>(), length) })
}

pub unsafe fn pcall(state: *mut LuaState, arguments: c_int, results: c_int) -> c_int {
    unsafe { lua_pcallk(state, arguments, results, 0, 0, core::ptr::null()) }
}
