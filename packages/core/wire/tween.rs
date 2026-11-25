use core::ffi::c_int;
use std::sync::Mutex;

use super::out::{AstralWireBuffer, Out};
use super::properties;

const MAX_TWEENS: usize = 64;
const TWEEN_MS: f64 = 220.0;

#[derive(Clone, Copy)]
struct Tween {
    node: i32,
    width: f64,
    inset: f64,
    started: Option<f64>,
    fade_only: bool,
}

static TWEENS: Mutex<Vec<Tween>> = Mutex::new(Vec::new());

fn start(tween: Tween) {
    let Ok(mut tweens) = TWEENS.lock() else { return };
    if tweens.len() < MAX_TWEENS {
        tweens.push(tween);
    }
}

fn forget(node: i32) {
    if let Ok(mut tweens) = TWEENS.lock() {
        tweens.retain(|tween| tween.node != node);
    }
}

fn eased(progress: f64) -> f64 {
    let remaining = 1.0 - progress;
    1.0 - remaining * remaining * remaining
}

fn put_frame(out: &mut Out, tween: &Tween, amount: f64) {
    let properties = properties::current();
    if tween.fade_only {
        out.update(tween.node, 1);
        out.prop_number(properties.opacity, amount);
        return;
    }
    out.update(tween.node, 3);
    out.prop_number(properties.opacity, amount);
    out.prop_number(properties.maxWidth, tween.width * amount);
    out.prop_quad(properties.margin, [0.0, 0.0, (tween.inset * amount) as f32, 0.0]);
}

#[unsafe(no_mangle)]
pub extern "C" fn wireTweenIn(node: i32, width: f64, inset: f64) {
    start(Tween {
        node,
        width,
        inset,
        started: None,
        fade_only: false,
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn wireTweenFade(node: i32) {
    forget(node);
    start(Tween {
        node,
        width: 0.0,
        inset: 0.0,
        started: None,
        fade_only: true,
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn wireTweenForget(node: i32) {
    forget(node);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_tweens_active() -> c_int {
    TWEENS.lock().map_or(0, |tweens| tweens.len() as c_int)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_tween_frame(buffer: *mut AstralWireBuffer, now: f64) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    *buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    let Ok(mut tweens) = TWEENS.lock() else { return 0 };
    if tweens.is_empty() {
        return 0;
    }
    let mut out = Out::batch();
    tweens.retain_mut(|tween| {
        let started = *tween.started.get_or_insert(now);
        let progress = ((now - started) / TWEEN_MS).min(1.0);
        put_frame(&mut out, tween, eased(progress));
        progress < 1.0
    });
    out.finish(buffer) as c_int
}
