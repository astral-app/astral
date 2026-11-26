use core::ffi::{c_char, c_int};

use astral_core::wire::children::{wire_discord_children, wire_expand_child_edits};
use astral_core::wire::emoji::{astral_wire_set_emoji_resolver, glyph, wire_emoji_batch};
use astral_core::wire::format::{MAX_SPANS, Span, wire_scan};
use astral_core::wire::out::Out;
use astral_core::wire::properties::current;

unsafe extern "C" {
    fn free(pointer: *mut core::ffi::c_void);
}

unsafe extern "C" fn twemoji(codepoints: *const c_char, url: *mut c_char, capacity: usize) -> c_int {
    let points = unsafe { core::ffi::CStr::from_ptr(codepoints) }.to_bytes();
    let resolved = format!("https://emoji.example/{}.png\0", core::str::from_utf8(points).unwrap());
    if resolved.len() > capacity {
        return 0;
    }
    unsafe { core::ptr::copy_nonoverlapping(resolved.as_ptr(), url.cast::<u8>(), resolved.len()) };
    1
}

fn bytes_of(out: Out) -> Vec<u8> {
    let bytes = unsafe { core::slice::from_raw_parts(out.bytes, out.length) }.to_vec();
    out.discard();
    bytes
}

fn scanned(bytes: &[u8]) -> (Vec<Span>, usize) {
    let mut spans = vec![Span::default(); MAX_SPANS];
    let mut count = 0;
    assert_eq!(unsafe { wire_scan(bytes.as_ptr(), bytes.len(), spans.as_mut_ptr(), &mut count) }, 0);
    (spans, count)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

#[test]
fn encodes_codepoints_as_utf8() {
    assert_eq!(glyph(b"1f600", 64), "😀".as_bytes());
    assert_eq!(glyph(b"2764-fe0f", 64), "❤️".as_bytes());
    assert_eq!(glyph(b"41", 64), b"A");
    assert!(glyph(b"1f600-1f600-1f600", 6).len() <= 6);
}

#[test]
fn replaces_emoji_uris_with_resolved_images() {
    astral_wire_set_emoji_resolver(Some(twemoji));
    let mut out = Out::batch();
    out.put8(1);
    out.put(&81i32.to_le_bytes());
    out.put16(5);
    out.put(b"image");
    out.put16(1);
    out.prop_string(current().uri, b"emoji://1f600");
    let bytes = bytes_of(out);
    let (mut spans, mut count) = scanned(&bytes);
    let mut length = 0;
    let rewritten = unsafe { wire_emoji_batch(bytes.as_ptr(), bytes.len(), spans.as_mut_ptr(), &mut count, &mut length) };
    assert!(!rewritten.is_null());
    let result = unsafe { core::slice::from_raw_parts(rewritten, length) }.to_vec();
    unsafe { free(rewritten.cast()) };
    assert!(contains(&result, b"https://emoji.example/1f600.png"));
    assert_eq!(count, 1);
}

#[test]
fn expands_child_edits_against_remembered_lists() {
    let mut out = Out::batch();
    out.put8(3);
    out.put(&91i32.to_le_bytes());
    out.put32(3);
    for id in [1i32, 2, 3] {
        out.put(&id.to_le_bytes());
    }
    out.put8(10);
    out.put(&91i32.to_le_bytes());
    out.put32(1);
    out.put32(1);
    out.put32(1);
    out.put32(1);
    out.put(&9i32.to_le_bytes());
    let bytes = bytes_of(out);
    let (mut spans, mut count) = scanned(&bytes);
    let mut length = 0;
    let rewritten = unsafe { wire_expand_child_edits(bytes.as_ptr(), bytes.len(), spans.as_mut_ptr(), &mut count, &mut length) };
    assert!(!rewritten.is_null());
    unsafe { free(rewritten.cast()) };
    let mut children: *const i32 = core::ptr::null();
    let total = unsafe { wire_discord_children(91, &mut children) };
    assert_eq!(unsafe { core::slice::from_raw_parts(children, total as usize) }, &[1, 9, 3]);
}
