use core::ffi::{c_char, c_int};

use astral_core::wire::format::{MAX_SPANS, Span, wire_scan};
use astral_core::wire::host_calls::{astral_host_call_rewrite, astral_host_calls_set_image_rewriter};
use astral_core::wire::identifiers::{astral_wire_identifier, astral_wire_note_identifiers};
use astral_core::wire::image_rewrite::{astral_wire_set_image_rewriter, wire_rewrite_images};
use astral_core::wire::images::{astral_wire_refresh_images, wire_track_images};
use astral_core::wire::out::{AstralWireBuffer, Out};
use astral_core::wire::properties::current;
use astral_core::wire::tag::Tag;
use astral_core::wire::tween::{astral_wire_tween_frame, astral_wire_tweens_active, wire_tween_in};

unsafe extern "C" {
    fn free(pointer: *mut core::ffi::c_void);
}

unsafe extern "C" fn swap_host(uri: *const c_char, rewritten: *mut c_char, capacity: usize) -> c_int {
    let original = unsafe { core::ffi::CStr::from_ptr(uri) }.to_bytes();
    let replacement: &[u8] = if original.starts_with(b"https://a.example/") {
        b"https://b.example/x.png\0"
    } else if original.starts_with(b"https://c.example/") {
        b"https://c.example/a_1.webp?animated=true\0"
    } else {
        return 0;
    };
    if replacement.len() > capacity {
        return 0;
    }
    unsafe { core::ptr::copy_nonoverlapping(replacement.as_ptr(), rewritten.cast::<u8>(), replacement.len()) };
    1
}

fn take(buffer: &AstralWireBuffer) -> Vec<u8> {
    let bytes = unsafe { core::slice::from_raw_parts(buffer.bytes, buffer.length) }.to_vec();
    unsafe { free(buffer.bytes.cast()) };
    bytes
}

fn creation(out: &mut Out, id: i32, kind: &[u8], strings: &[(u16, &[u8])]) {
    out.put8(1);
    out.put(&id.to_le_bytes());
    out.put16(kind.len() as u16);
    out.put(kind);
    out.put16(strings.len() as u16);
    for (property, value) in strings {
        out.prop_string(*property, value);
    }
}

fn bytes_of(out: Out) -> Vec<u8> {
    let bytes = unsafe { core::slice::from_raw_parts(out.bytes, out.length) }.to_vec();
    out.discard();
    bytes
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

#[test]
fn rewrites_https_strings_in_host_calls() {
    astral_host_calls_set_image_rewriter(Some(swap_host));
    let mut arguments = Out::default();
    arguments.put8(7);
    arguments.put32(2);
    arguments.put8(6);
    arguments.put_string(b"https://a.example/y.png");
    arguments.put8(3);
    arguments.put32(5);
    let arguments = bytes_of(arguments);
    let mut frame = vec![1u8; 12];
    frame.extend_from_slice(&(arguments.len() as u32).to_le_bytes());
    frame.extend_from_slice(&arguments);
    frame.extend_from_slice(b"tail");
    let mut buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    assert_eq!(unsafe { astral_host_call_rewrite(frame.as_ptr(), frame.len(), &mut buffer) }, 1);
    let rewritten = take(&buffer);
    assert!(contains(&rewritten, b"https://b.example/x.png"));
    assert!(rewritten.ends_with(b"tail"));
    for cut in 0..frame.len() - 4 {
        let mut buffer = AstralWireBuffer {
            bytes: core::ptr::null_mut(),
            length: 0,
        };
        assert_eq!(unsafe { astral_host_call_rewrite(frame.as_ptr(), cut, &mut buffer) }, 0);
    }
}

#[test]
fn rewrites_image_uris_and_rescans() {
    astral_wire_set_image_rewriter(Some(swap_host));
    let uri = current().uri;
    let mut out = Out::batch();
    creation(&mut out, 41, b"image", &[(uri, b"https://a.example/z.png")]);
    creation(&mut out, 42, b"text", &[(uri, b"https://a.example/z.png")]);
    let bytes = bytes_of(out);
    let mut spans = vec![Span::default(); MAX_SPANS];
    let mut count = 0;
    assert_eq!(unsafe { wire_scan(bytes.as_ptr(), bytes.len(), spans.as_mut_ptr(), &mut count) }, 0);
    let mut length = 0;
    let rewritten = unsafe { wire_rewrite_images(bytes.as_ptr(), bytes.len(), spans.as_mut_ptr(), &mut count, &mut length) };
    assert!(!rewritten.is_null());
    let result = take(&AstralWireBuffer { bytes: rewritten, length });
    assert_eq!(count, 2);
    assert!(contains(&result, b"https://b.example/x.png"));
    assert!(contains(&result, b"https://a.example/z.png"));
}

#[test]
fn animates_images_rewritten_to_animated_urls() {
    astral_wire_set_image_rewriter(Some(swap_host));
    let properties = current();
    let mut out = Out::batch();
    out.put8(1);
    out.put(&43i32.to_le_bytes());
    out.put16(5);
    out.put(b"image");
    out.put16(2);
    out.prop_string(properties.uri, b"https://c.example/a_1.webp");
    out.put16(properties.animated);
    out.put_tag(Tag::False);
    let bytes = bytes_of(out);
    let mut spans = vec![Span::default(); MAX_SPANS];
    let mut count = 0;
    assert_eq!(unsafe { wire_scan(bytes.as_ptr(), bytes.len(), spans.as_mut_ptr(), &mut count) }, 0);
    let mut length = 0;
    let rewritten = unsafe { wire_rewrite_images(bytes.as_ptr(), bytes.len(), spans.as_mut_ptr(), &mut count, &mut length) };
    assert!(!rewritten.is_null());
    let result = take(&AstralWireBuffer { bytes: rewritten, length });
    assert_eq!(count, 1);
    let mut animated = properties.animated.to_le_bytes().to_vec();
    animated.push(Tag::True as u8);
    assert!(contains(&result, &animated));
    let mut still = properties.animated.to_le_bytes().to_vec();
    still.push(Tag::False as u8);
    assert!(!contains(&result, &still));
}

#[test]
fn remembers_routable_identifiers() {
    let id = current().accessibility_id;
    let mut out = Out::batch();
    creation(&mut out, 51, b"view", &[(id, b"astral.settings.row")]);
    creation(&mut out, 52, b"view", &[(id, b"message.content:1")]);
    let bytes = bytes_of(out);
    unsafe { astral_wire_note_identifiers(bytes.as_ptr(), bytes.len()) };
    let mut text = [0 as c_char; 8];
    assert_eq!(unsafe { astral_wire_identifier(51, text.as_mut_ptr(), text.len()) }, 1);
    assert_eq!(unsafe { core::ffi::CStr::from_ptr(text.as_ptr()) }.to_bytes(), b"astral.");
    assert_eq!(unsafe { astral_wire_identifier(52, text.as_mut_ptr(), text.len()) }, 0);
}

#[test]
fn refreshes_tracked_icons_with_a_generation() {
    let uri = current().uri;
    let mut out = Out::batch();
    creation(&mut out, 61, b"image", &[(uri, b"res://ic_mana_star#1")]);
    let bytes = bytes_of(out);
    let mut spans = vec![Span::default(); MAX_SPANS];
    let mut count = 0;
    assert_eq!(unsafe { wire_scan(bytes.as_ptr(), bytes.len(), spans.as_mut_ptr(), &mut count) }, 0);
    unsafe { wire_track_images(bytes.as_ptr(), spans.as_ptr(), count) };
    let mut buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    assert!(unsafe { astral_wire_refresh_images(&mut buffer, 2) } >= 1);
    assert!(contains(&take(&buffer), b"res://ic_mana_star#2"));
}

#[test]
fn eases_tweens_to_completion() {
    wire_tween_in(71, 100.0, 8.0);
    assert!(astral_wire_tweens_active() >= 1);
    let mut buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    assert_eq!(unsafe { astral_wire_tween_frame(&mut buffer, 1000.0) }, 1);
    take(&buffer);
    assert_eq!(unsafe { astral_wire_tween_frame(&mut buffer, 1300.0) }, 1);
    take(&buffer);
    assert_eq!(astral_wire_tweens_active(), 0);
}
