use core::ffi::CStr;

use astral_core::wire::format::{
    MAX_SPANS, Span, scan, type_is, wireChild, wireColorProperty, wireParentOf, wireStringProperty, wireStringPropertyPrefix,
};
use astral_core::wire::out::{Out, wirePropInt, wirePropString, wirePut8, wirePutChildren};

unsafe extern "C" {
    fn free(pointer: *mut core::ffi::c_void);
}

fn empty() -> Out {
    Out {
        bytes: core::ptr::null_mut(),
        length: 0,
        capacity: 0,
        failed: 0,
    }
}

fn batch(build: impl FnOnce(&mut Out)) -> Vec<u8> {
    let mut out = empty();
    unsafe { wirePut8(&mut out, 1) };
    build(&mut out);
    assert_eq!(out.failed, 0);
    let bytes = unsafe { core::slice::from_raw_parts(out.bytes, out.length) }.to_vec();
    unsafe { free(out.bytes.cast()) };
    bytes
}

fn string(out: &mut Out, id: u16, value: &CStr) {
    unsafe { wirePropString(out, id, value.as_ptr()) };
}

fn create(out: &mut Out, id: i32, kind: &str, properties: impl FnOnce(&mut Out) -> u16) {
    unsafe { wirePut8(out, 1) };
    out.put(&id.to_le_bytes());
    out.put16(kind.len() as u16);
    out.put(kind.as_bytes());
    let count_at = out.length;
    out.put16(0);
    let count = properties(out);
    unsafe { core::ptr::copy_nonoverlapping(count.to_le_bytes().as_ptr(), out.bytes.add(count_at), 2) };
}

fn spans(bytes: &[u8]) -> Vec<Span> {
    let mut spans = vec![Span::default(); MAX_SPANS];
    let used = scan(bytes, &mut spans).expect("scan");
    spans.truncate(used);
    spans
}

fn has_string(bytes: &[u8], span: &Span, id: u16, value: &CStr, prefix: bool) -> bool {
    let found = unsafe {
        if prefix {
            wireStringPropertyPrefix(bytes.as_ptr(), span, id, value.as_ptr())
        } else {
            wireStringProperty(bytes.as_ptr(), span, id, value.as_ptr())
        }
    };
    found == 1
}

#[test]
fn scans_creations_children_and_properties() {
    let bytes = batch(|out| {
        create(out, 7, "text", |out| {
            string(out, 9, c"message.content:42");
            unsafe { wirePropInt(out, 90, 0x00ff00) };
            2
        });
        create(out, 8, "view", |_| 0);
        let children = [7, 9];
        unsafe { wirePutChildren(out, 8, children.as_ptr(), 2) };
    });
    let spans = spans(&bytes);
    assert_eq!(spans.len(), 3);
    assert!(type_is(&bytes, &spans[0], b"text"));
    assert!(has_string(&bytes, &spans[0], 9, c"message.content:42", false));
    assert!(has_string(&bytes, &spans[0], 9, c"message.content:", true));
    assert!(!has_string(&bytes, &spans[0], 9, c"message.content:4", false));
    let mut color = 0;
    assert_eq!(unsafe { wireColorProperty(bytes.as_ptr(), &spans[0], 90, &mut color) }, 1);
    assert_eq!(color, 0x00ff00);
    assert_eq!(unsafe { wireChild(bytes.as_ptr(), &spans[2], 1) }, 9);
    assert_eq!(unsafe { wireChild(bytes.as_ptr(), &spans[2], 2) }, 0);
    assert_eq!(unsafe { wireParentOf(bytes.as_ptr(), spans.as_ptr(), spans.len(), 9) }, 8);
}

#[test]
fn rejects_truncated_and_unknown_batches() {
    let bytes = batch(|out| {
        create(out, 7, "text", |out| {
            string(out, 9, c"hello");
            1
        });
    });
    let mut spans = vec![Span::default(); MAX_SPANS];
    for cut in 2..bytes.len() {
        assert!(scan(&bytes[..cut], &mut spans).is_none());
    }
    assert!(scan(&[2], &mut spans).is_none());
    assert!(scan(&[1, 200], &mut spans).is_none());
}

#[test]
fn bounds_deep_nesting() {
    let mut bytes = vec![1, 7, 0, 0, 0, 0];
    for _ in 0..16 {
        bytes.extend_from_slice(&[7, 1, 0, 0, 0]);
    }
    bytes.push(0);
    let mut spans = vec![Span::default(); MAX_SPANS];
    assert!(scan(&bytes, &mut spans).is_none());
}

#[test]
fn writes_gradients() {
    let colors = [0xFF000000u32, 0xFFFFFFFF];
    let bytes = batch(|out| unsafe { astral_core::wire::out::wirePropGradient(out, 91, 90.0, colors.as_ptr(), 2) });
    assert_eq!(bytes[0], 1);
    assert_eq!(u16::from_le_bytes([bytes[1], bytes[2]]), 91);
    assert_eq!(bytes[3], 8);
    assert_eq!(u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]), 3);
}
