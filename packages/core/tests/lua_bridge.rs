use std::ffi::CString;

use astral_core::lua::api as lua;
use astral_core::lua::json::{push_json, to_json};
use astral_core::lua::sandbox::{
    astral_lua_sandbox_call, astral_lua_sandbox_free, astral_lua_sandbox_load, astral_lua_sandbox_memory, astral_lua_sandbox_new,
    astral_lua_sandbox_state,
};
use serde_json::json;

fn taken(string: astral_core::support::bridge::AstralRustString) -> Option<String> {
    if string.bytes.is_null() {
        return None;
    }
    let text = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(string.bytes, string.length) }).into_owned();
    unsafe { astral_core::support::bridge::astral_rust_string_free(string) };
    Some(text)
}

fn run(sandbox: *mut astral_core::lua::sandbox::Sandbox, source: &str) -> Option<String> {
    let chunk = "test";
    unsafe {
        if let Some(error) = taken(astral_lua_sandbox_load(
            sandbox,
            source.as_ptr().cast(),
            source.len(),
            chunk.as_ptr().cast(),
            chunk.len(),
            0,
        )) {
            return Some(error);
        }
        taken(astral_lua_sandbox_call(sandbox, 0, 1))
    }
}

#[test]
fn sandbox_hides_unsafe_globals_and_limits_work() {
    let sandbox = astral_lua_sandbox_new(8 << 20, 40, 5000);
    assert!(
        run(
            sandbox,
            "return load == nil and require == nil and os == nil and io == nil and string ~= nil"
        )
        .is_none()
    );
    let state = unsafe { astral_lua_sandbox_state(sandbox) };
    assert!(unsafe { lua::lua_toboolean(state, -1) } != 0);
    unsafe { lua::pop(state, 1) };
    let error = run(sandbox, "while true do end").expect("budget error");
    assert!(error.contains("instruction budget"), "{error}");
    assert!(unsafe { astral_lua_sandbox_memory(sandbox) } > 0);
    let tight = astral_lua_sandbox_new(64 << 10, 40, 5000);
    let error = run(tight, "local t = {} for i = 1, 100000 do t[i] = tostring(i) end").expect("memory error");
    assert!(error.contains("memory"), "{error}");
    unsafe {
        astral_lua_sandbox_free(sandbox);
        astral_lua_sandbox_free(tight);
    }
}

#[test]
fn json_round_trips_through_lua() {
    let sandbox = astral_lua_sandbox_new(8 << 20, 40, 5000);
    let state = unsafe { astral_lua_sandbox_state(sandbox) };
    let value = json!({"name": "astral", "count": 3, "ratio": 0.5, "on": true, "list": [1, 2, {"deep": "x"}], "empty": []});
    unsafe { push_json(state, &value, 0) };
    let back = unsafe { to_json(state, -1, 0) }.expect("json");
    assert_eq!(back, value);
    unsafe { lua::pop(state, 1) };
    assert!(run(sandbox, "return {1, nil, 3}").is_none());
    assert!(unsafe { to_json(state, -1, 0) }.is_some());
    unsafe { lua::pop(state, 1) };
    assert!(run(sandbox, "return {[true] = 1}").is_none());
    assert!(unsafe { to_json(state, -1, 0) }.is_none());
    unsafe { lua::pop(state, 1) };
    let deep = CString::new("local t = {} local c = t for i = 1, 20 do c.x = {} c = c.x end return t").unwrap();
    assert!(run(sandbox, deep.to_str().unwrap()).is_none());
    assert!(unsafe { to_json(state, -1, 0) }.is_none());
    unsafe { astral_lua_sandbox_free(sandbox) };
}
