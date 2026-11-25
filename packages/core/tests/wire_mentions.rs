use core::ffi::c_int;

use astral_core::wire::abi::{AstralMentionContext, AstralMentionDecoration};
use astral_core::wire::format::{MAX_SPANS, Span, wireScan};
use astral_core::wire::mentions::{astral_wire_set_mention_decorator, astral_wire_set_message_text_colorizer, wireDecorateMentions};
use astral_core::wire::out::Out;
use astral_core::wire::properties::current;

unsafe extern "C" fn decorate(context: *const AstralMentionContext, decoration: *mut AstralMentionDecoration) -> c_int {
    let text = unsafe { core::ffi::CStr::from_ptr((*context).text) }.to_bytes();
    if text != b"@adrian" {
        return 0;
    }
    let image = b"https://cdn.example/a_chip_.png\0";
    let decoration = unsafe { &mut *decoration };
    for (index, byte) in image.iter().enumerate() {
        decoration.image[index] = *byte as core::ffi::c_char;
    }
    decoration.color = 0xFFABCDEF;
    1
}

unsafe extern "C" fn tint(_: u64, _: u64, color: *mut u32) -> c_int {
    unsafe { *color = 0x80FF0000 };
    1
}

fn text_span(out: &mut Out, text: &[u8], chip: bool) {
    let properties = current();
    out.put8(8);
    out.put32(if chip { 4 } else { 2 });
    out.prop_string(properties.text, text);
    out.put16(properties.color);
    out.put8(4);
    out.put(&0xFF112233i64.to_le_bytes());
    if chip {
        out.prop_int(properties.bg, 1);
        out.prop_string(properties.link, b"user");
    }
}

fn message(spans: impl FnOnce(&mut Out) -> u32) -> Vec<u8> {
    let properties = current();
    let mut out = Out::batch();
    out.put8(1);
    out.put(&10i32.to_le_bytes());
    out.put16(4);
    out.put(b"view");
    out.put16(1);
    out.prop_string(properties.accessibilityId, b"message.content:100000000000000005");
    out.put8(1);
    out.put(&11i32.to_le_bytes());
    out.put16(4);
    out.put(b"text");
    out.put16(1);
    out.put16(properties.spans);
    out.put8(7);
    let count_at = out.length;
    out.put32(0);
    let count = spans(&mut out);
    unsafe { core::ptr::copy_nonoverlapping(count.to_le_bytes().as_ptr(), out.bytes.add(count_at), 4) };
    out.put8(3);
    out.put(&10i32.to_le_bytes());
    out.put32(1);
    out.put(&11i32.to_le_bytes());
    let bytes = unsafe { core::slice::from_raw_parts(out.bytes, out.length) }.to_vec();
    out.discard();
    bytes
}

fn decorated(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut spans = vec![Span::default(); MAX_SPANS];
    let mut count = 0;
    assert_eq!(unsafe { wireScan(bytes.as_ptr(), bytes.len(), spans.as_mut_ptr(), &mut count) }, 0);
    let mut out = Out::default();
    let mut changed = 0;
    unsafe { wireDecorateMentions(bytes.as_ptr(), spans.as_ptr(), count, &mut out, &mut changed, bytes.len()) };
    let result = (changed != 0).then(|| unsafe { core::slice::from_raw_parts(out.bytes, out.length) }.to_vec());
    out.discard();
    result
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

#[test]
fn decorates_mention_chips_and_tints_text() {
    astral_wire_set_mention_decorator(Some(decorate));
    astral_wire_set_message_text_colorizer(Some(tint));
    let bytes = message(|out| {
        text_span(out, b"hi ", false);
        text_span(out, b"@adrian", true);
        2
    });
    let result = decorated(&bytes).expect("decorated");
    assert!(contains(&result, b"avatar"));
    assert!(contains(&result, b"https://cdn.example/a_ABCDEF_.png"));

    let long = message(|out| {
        for _ in 0..65 {
            text_span(out, b"word ", false);
        }
        65
    });
    assert!(decorated(&long).is_none());
}
