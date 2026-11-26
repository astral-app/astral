use core::ffi::{c_char, c_int, c_void};
use std::ffi::CString;

use crate::lua::api::{self as lua, LuaDebug, LuaState};
use crate::support::bridge::{AstralRustString, bytes};

const BUDGET_MESSAGE: &[u8] = b"astral: plugin exceeded its instruction budget\0";

#[repr(C)]
pub struct Sandbox {
    state: *mut LuaState,
    used: usize,
    memory_limit: usize,
    budget_slices: c_int,
    slice_instructions: c_int,
    remaining: c_int,
}

unsafe extern "C" fn allocate(user: *mut c_void, pointer: *mut c_void, old: usize, size: usize) -> *mut c_void {
    let sandbox = unsafe { &mut *user.cast::<Sandbox>() };
    let previous = if pointer.is_null() { 0 } else { old };
    if size == 0 {
        sandbox.used -= previous;
        unsafe { libc::free(pointer) };
        return core::ptr::null_mut();
    }
    if sandbox.used - previous + size > sandbox.memory_limit {
        return core::ptr::null_mut();
    }
    let grown = unsafe { libc::realloc(pointer, size) };
    if !grown.is_null() {
        sandbox.used = sandbox.used - previous + size;
    }
    grown
}

unsafe extern "C" fn spend_budget(state: *mut LuaState, _: *mut LuaDebug) {
    let mut user: *mut c_void = core::ptr::null_mut();
    unsafe { lua::lua_getallocf(state, &mut user) };
    let sandbox = user.cast::<Sandbox>();
    if sandbox.is_null() {
        return;
    }
    let remaining = unsafe {
        (*sandbox).remaining -= 1;
        (*sandbox).remaining
    };
    if remaining <= 0 {
        unsafe { lua::luaL_error(state, BUDGET_MESSAGE.as_ptr().cast()) };
    }
}

unsafe fn open_safe_libraries(state: *mut LuaState) {
    let libraries: [(&[u8], lua::CFunction); 6] = [
        (b"_G\0", lua::luaopen_base),
        (b"table\0", lua::luaopen_table),
        (b"string\0", lua::luaopen_string),
        (b"math\0", lua::luaopen_math),
        (b"utf8\0", lua::luaopen_utf8),
        (b"coroutine\0", lua::luaopen_coroutine),
    ];
    for (name, open) in libraries {
        unsafe {
            lua::luaL_requiref(state, name.as_ptr().cast(), open, 1);
            lua::pop(state, 1);
        }
    }
    for name in [&b"dofile\0"[..], b"loadfile\0", b"load\0", b"require\0", b"collectgarbage\0"] {
        unsafe {
            lua::lua_pushnil(state);
            lua::lua_setglobal(state, name.as_ptr().cast());
        }
    }
}

impl Sandbox {
    unsafe fn pop_error(&mut self) -> Vec<u8> {
        let message = unsafe { lua::to_bytes(self.state, -1) }.map_or_else(|| b"unknown error".to_vec(), <[u8]>::to_vec);
        unsafe { lua::pop(self.state, 1) };
        message
    }

    pub unsafe fn load(&mut self, source: &[u8], chunk: &[u8], mode: &[u8]) -> Option<Vec<u8>> {
        if self.state.is_null() {
            return Some(b"out of memory".to_vec());
        }
        let mut name = b"=".to_vec();
        name.extend_from_slice(chunk);
        let name = CString::new(name).unwrap_or_default();
        let status = unsafe {
            lua::luaL_loadbufferx(
                self.state,
                source.as_ptr().cast(),
                source.len(),
                name.as_ptr(),
                mode.as_ptr().cast(),
            )
        };
        (status != lua::OK).then(|| unsafe { self.pop_error() })
    }

    pub unsafe fn call(&mut self, arguments: c_int, results: c_int) -> Option<Vec<u8>> {
        if self.state.is_null() {
            return Some(b"out of memory".to_vec());
        }
        self.remaining = self.budget_slices;
        let status = unsafe {
            lua::lua_sethook(self.state, Some(spend_budget), lua::MASKCOUNT, self.slice_instructions);
            let status = lua::pcall(self.state, arguments, results);
            lua::lua_sethook(self.state, None, 0, 0);
            status
        };
        (status != lua::OK).then(|| unsafe { self.pop_error() })
    }
}

fn outcome(error: Option<Vec<u8>>) -> AstralRustString {
    error.map_or(
        AstralRustString {
            bytes: core::ptr::null_mut(),
            length: 0,
        },
        AstralRustString::new,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_lua_sandbox_new(memory: usize, budget_slices: c_int, slice_instructions: c_int) -> *mut Sandbox {
    let sandbox = Box::into_raw(Box::new(Sandbox {
        state: core::ptr::null_mut(),
        used: 0,
        memory_limit: memory,
        budget_slices,
        slice_instructions,
        remaining: 0,
    }));
    unsafe {
        let state = lua::lua_newstate(allocate, sandbox.cast());
        (*sandbox).state = state;
        if !state.is_null() {
            open_safe_libraries(state);
        }
    }
    sandbox
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_sandbox_free(sandbox: *mut Sandbox) {
    if sandbox.is_null() {
        return;
    }
    unsafe {
        if !(*sandbox).state.is_null() {
            lua::lua_close((*sandbox).state);
        }
        drop(Box::from_raw(sandbox));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_sandbox_state(sandbox: *const Sandbox) -> *mut LuaState {
    unsafe { sandbox.as_ref() }.map_or(core::ptr::null_mut(), |sandbox| sandbox.state)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_sandbox_memory(sandbox: *const Sandbox) -> usize {
    unsafe { sandbox.as_ref() }.map_or(0, |sandbox| sandbox.used)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_sandbox_load(
    sandbox: *mut Sandbox,
    source: *const c_char,
    length: usize,
    chunk: *const c_char,
    chunk_length: usize,
    bytecode: c_int,
) -> AstralRustString {
    let Some(sandbox) = (unsafe { sandbox.as_mut() }) else {
        return outcome(Some(b"out of memory".to_vec()));
    };
    let mode: &[u8] = if bytecode != 0 { b"b\0" } else { b"t\0" };
    outcome(unsafe { sandbox.load(bytes(source, length), bytes(chunk, chunk_length), mode) })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_sandbox_call(sandbox: *mut Sandbox, arguments: c_int, results: c_int) -> AstralRustString {
    let Some(sandbox) = (unsafe { sandbox.as_mut() }) else {
        return outcome(Some(b"out of memory".to_vec()));
    };
    outcome(unsafe { sandbox.call(arguments, results) })
}
