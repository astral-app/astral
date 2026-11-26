use core::ffi::c_int;

use crate::lua::api::{self as lua, LuaState};
use crate::lua::enums::TextKind;
use crate::lua::host::hooks::{opaque, running_plugins, set_text};
use crate::lua::host::{self as host, Plugin};
use crate::plugins::capability::Capability;

pub enum Field<'a> {
    Text(&'a str),
    Integer(i64),
    RepliesToMe(bool),
}

#[derive(Default)]
pub struct TextStyle {
    pub color: Option<u32>,
    pub image: Option<String>,
    pub round: Option<bool>,
    pub hide_at: bool,
    pub after: bool,
}

pub fn wanted() -> bool {
    running_plugins()
        .into_iter()
        .any(|plugin| unsafe { !(*plugin).text_stylers.is_empty() })
}

unsafe fn push_fields(state: *mut LuaState, plugin: *mut Plugin, kind: TextKind, fields: &[(&str, Field)]) {
    unsafe {
        lua::lua_createtable(state, 0, c_int::try_from(fields.len() + 1).unwrap_or(0));
        set_text(state, "kind", kind.value());
    }
    for (name, field) in fields {
        match field {
            Field::Text(text) if !text.is_empty() => unsafe { set_text(state, name, text) },
            Field::Text(_) => {}
            Field::Integer(number) => unsafe {
                lua::lua_pushinteger(state, *number);
                lua::set_field(state, -2, name);
            },
            Field::RepliesToMe(replies) => {
                if unsafe { (*plugin).permissions.contains(&Capability::DiscordMessages) } {
                    unsafe {
                        lua::lua_pushboolean(state, c_int::from(*replies));
                        lua::set_field(state, -2, name);
                    }
                }
            }
        }
    }
}

unsafe fn merge(state: *mut LuaState, style: &mut TextStyle) -> bool {
    if !unsafe { lua::is_table(state, -1) } {
        return false;
    }
    unsafe {
        lua::get_field(state, -1, "color");
        if style.color.is_none() && lua::lua_isinteger(state, -1) != 0 {
            style.color = Some(opaque(lua::to_integer(state, -1)));
        }
        lua::pop(state, 1);
        lua::get_field(state, -1, "image");
        let image = lua::to_bytes(state, -1)
            .map(|image| String::from_utf8_lossy(image).into_owned())
            .unwrap_or_default();
        lua::pop(state, 1);
        if style.image.is_none() && (image.starts_with("https://") || image.starts_with("res://astral_tinted_")) {
            lua::get_field(state, -1, "round");
            style.round = Some(lua::lua_type(state, -1) == lua::TNIL || lua::lua_toboolean(state, -1) != 0);
            lua::pop(state, 1);
            style.image = Some(image);
        }
        for (name, slot) in [("hide_at", &mut style.hide_at), ("after", &mut style.after)] {
            lua::get_field(state, -1, name);
            *slot |= lua::lua_toboolean(state, -1) != 0;
            lua::pop(state, 1);
        }
    }
    true
}

pub fn style(kind: TextKind, fields: &[(&str, Field)]) -> Option<TextStyle> {
    let mut style = TextStyle::default();
    let mut styled = false;
    for plugin in running_plugins() {
        let stylers = unsafe { &*plugin }.text_stylers.clone();
        for reference in stylers {
            let state = unsafe { (*plugin).state() };
            unsafe {
                lua::push_reference(state, reference);
                push_fields(state, plugin, kind, fields);
            }
            if host::call(plugin, 1, 1, "style_text") {
                styled |= unsafe { merge(state, &mut style) };
                unsafe { lua::pop(state, 1) };
            }
            if unsafe { (*plugin).failed } {
                break;
            }
        }
        if unsafe { (*plugin).failed } {
            host::stop_plugin(plugin);
        }
    }
    styled.then_some(style)
}
