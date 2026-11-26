use super::format::{Span, find_string, property_cursor, type_is, wire_scan};
use super::out::Out;
use super::properties;
use super::rewriter::{ImageRewriter, Slot};

static REWRITER: Slot = Slot::new();

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_image_rewriter(rewriter: Option<ImageRewriter>) {
    REWRITER.set(rewriter);
}

fn put_rewritten(out: &mut Out, bytes: &[u8], span: &Span, uri: &[u8]) {
    let properties = properties::current();
    let (mut cursor, count) = property_cursor(bytes, span);
    out.put(&bytes[span.start..cursor.offset() - 2]);
    out.put16(count);
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let start = cursor.offset();
        let property = cursor.read16();
        let tag = cursor.read8();
        cursor.skip_value(tag, 0);
        if (property == properties.uri || property == properties.cache_key) && tag == 6 {
            out.prop_string(property, uri);
        } else {
            out.put(&bytes[start..cursor.offset().min(bytes.len())]);
        }
    }
}

fn rewritten_uri(bytes: &[u8], span: &Span) -> Option<Vec<u8>> {
    if span.op != 1 && span.op != 2 {
        return None;
    }
    if span.op == 1 && !type_is(bytes, span, b"image") {
        return None;
    }
    let uri = find_string(bytes, span, properties::current().uri)?;
    if !uri.starts_with(b"http") {
        return None;
    }
    REWRITER.rewrite(uri)
}

#[unsafe(export_name = "wireRewriteImages")]
pub unsafe extern "C" fn wire_rewrite_images(
    bytes: *const u8,
    length: usize,
    spans: *mut Span,
    count: *mut usize,
    new_length: *mut usize,
) -> *mut u8 {
    if REWRITER.get().is_none() || bytes.is_null() || spans.is_null() || count.is_null() || new_length.is_null() {
        return core::ptr::null_mut();
    }
    let data = unsafe { core::slice::from_raw_parts(bytes, length) };
    let scanned = unsafe { core::slice::from_raw_parts(spans, *count) };
    let mut out = Out::default();
    let mut copied = 0;
    for span in scanned {
        let Some(uri) = rewritten_uri(&data[..span.end], span) else {
            continue;
        };
        out.put(&data[copied..span.start]);
        put_rewritten(&mut out, &data[..span.end], span, &uri);
        copied = span.end;
    }
    if copied == 0 {
        out.discard();
        return core::ptr::null_mut();
    }
    out.put(&data[copied..]);
    if out.failed != 0 || unsafe { wire_scan(out.bytes, out.length, spans, count) } != 0 {
        out.discard();
        unsafe { wire_scan(bytes, length, spans, count) };
        return core::ptr::null_mut();
    }
    unsafe { *new_length = out.length };
    out.bytes
}
