use core::ffi::c_int;
use std::sync::Mutex;

use super::out::{AstralWireBuffer, Out};
use super::properties;

const MAX_TWEENS: usize = 64;
const TWEEN_MS: f64 = 220.0;
const PULSE_FLOOR: f64 = 0.3;
const MIN_PULSE_MS: f64 = 200.0;
const MAX_PULSE_MS: f64 = 10_000.0;

#[derive(Clone, Copy)]
struct Slide {
    track: i32,
    from: f64,
    to: f64,
    from_color: u32,
    to_color: u32,
}

#[derive(Clone, Copy)]
struct Tween {
    node: i32,
    width: f64,
    inset: f64,
    started: Option<f64>,
    fade_only: bool,
    pulse: Option<(f64, f64)>,
    slide: Option<Slide>,
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

fn pulse_opacity(elapsed: f64, period: f64, delay: f64) -> f64 {
    let phase = ((elapsed - delay) / period).rem_euclid(1.0);
    let wave = 0.5 - 0.5 * (phase * core::f64::consts::TAU).cos();
    PULSE_FLOOR + (1.0 - PULSE_FLOOR) * wave
}

fn mixed_channel(from: u32, to: u32, shift: u32, amount: f64) -> u32 {
    let (from, to) = (f64::from((from >> shift) & 0xFF), f64::from((to >> shift) & 0xFF));
    ((from + (to - from) * amount).round().clamp(0.0, 255.0) as u32) << shift
}

fn mixed_color(from: u32, to: u32, amount: f64) -> u32 {
    [24, 16, 8, 0]
        .iter()
        .fold(0, |color, shift| color | mixed_channel(from, to, *shift, amount))
}

fn put_frame(out: &mut Out, tween: &Tween, amount: f64) {
    let properties = properties::current();
    if let Some(slide) = tween.slide {
        out.update(tween.node, 1);
        out.prop_number(properties.translate_x, slide.from + (slide.to - slide.from) * amount);
        out.update(slide.track, 1);
        out.prop_long(
            properties.background,
            i64::from(mixed_color(slide.from_color, slide.to_color, amount)),
        );
        return;
    }
    if tween.fade_only {
        out.update(tween.node, 1);
        out.prop_number(properties.opacity, amount);
        return;
    }
    out.update(tween.node, 3);
    out.prop_number(properties.opacity, amount);
    out.prop_number(properties.max_width, tween.width * amount);
    out.prop_quad(properties.margin, [0.0, 0.0, (tween.inset * amount) as f32, 0.0]);
}

#[unsafe(export_name = "wireTweenIn")]
pub extern "C" fn wire_tween_in(node: i32, width: f64, inset: f64) {
    start(Tween {
        node,
        width,
        inset,
        started: None,
        fade_only: false,
        pulse: None,
        slide: None,
    });
}

#[unsafe(export_name = "wireTweenFade")]
pub extern "C" fn wire_tween_fade(node: i32) {
    forget(node);
    start(Tween {
        node,
        width: 0.0,
        inset: 0.0,
        started: None,
        fade_only: true,
        pulse: None,
        slide: None,
    });
}

pub fn wire_tween_slide(thumb: i32, track: i32, from: (f64, u32), to: (f64, u32)) {
    forget(thumb);
    start(Tween {
        node: thumb,
        width: 0.0,
        inset: 0.0,
        started: None,
        fade_only: false,
        pulse: None,
        slide: Some(Slide {
            track,
            from: from.0,
            to: to.0,
            from_color: from.1,
            to_color: to.1,
        }),
    });
}

pub fn wire_tween_pulse(node: i32, period: f64, delay: f64) {
    forget(node);
    start(Tween {
        node,
        width: 0.0,
        inset: 0.0,
        started: None,
        fade_only: true,
        pulse: Some((period.clamp(MIN_PULSE_MS, MAX_PULSE_MS), delay.clamp(0.0, MAX_PULSE_MS))),
        slide: None,
    });
}

pub fn forget_pulse(node: i32) {
    if let Ok(mut tweens) = TWEENS.lock() {
        tweens.retain(|tween| tween.node != node || tween.pulse.is_none());
    }
}

#[unsafe(export_name = "wireTweenForget")]
pub extern "C" fn wire_tween_forget(node: i32) {
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
        if let Some((period, delay)) = tween.pulse {
            put_frame(&mut out, tween, pulse_opacity(now - started, period, delay));
            return true;
        }
        let progress = ((now - started) / TWEEN_MS).min(1.0);
        put_frame(&mut out, tween, eased(progress));
        progress < 1.0
    });
    c_int::from(out.finish(buffer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reused_nodes_lose_their_pulse_but_keep_other_tweens() {
        wire_tween_pulse(901, 1200.0, 0.0);
        wire_tween_fade(902);
        forget_pulse(901);
        forget_pulse(902);
        let tweens = TWEENS.lock().unwrap();
        assert!(!tweens.iter().any(|tween| tween.node == 901));
        assert!(tweens.iter().any(|tween| tween.node == 902 && tween.pulse.is_none()));
    }

    #[test]
    fn switch_colors_blend_per_channel() {
        assert_eq!(mixed_color(0x1F00_0000, 0xFF58_65F2, 0.0), 0x1F00_0000);
        assert_eq!(mixed_color(0x1F00_0000, 0xFF58_65F2, 1.0), 0xFF58_65F2);
        assert_eq!(mixed_color(0x0000_0000, 0xFEFE_FEFE, 0.5), 0x7F7F_7F7F);
    }
}
