use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};

pub const LABEL: &str = "ASTRAL_DEBUG_COMPRESSED:";
pub const PREFIX: &str = "ast1:";

const BITS: u32 = 15;
const MASK: u32 = (1 << BITS) - 1;
const RANGES: [(u32, u32); 3] = [(0x4E00, 20_992), (0x3400, 6592), (0xAC00, 5184)];
const DICTIONARY: &str = concat!(
    "com.hammerandchisel.discord",
    "\"install\":\"Development (",
    "\"install\":\"Distribution (",
    "\"install\":\"Ad Hoc (",
    "\"source\":\"Sideloaded\"",
    "\"source\":\"App Store\"",
    "\"source\":\"TestFlight\"",
    "\"jailbreak\":\"roothide\"",
    "\"jailbreak\":\"rootless\"",
    "\"framework\":\"ElleKit ",
    "\"loader\":\"astral.dylib\"",
    "\"https\":{\"intercepted\":false,\"issuer\":\"GTS Root R4\"}",
    "\"binary\":\"decrypted · min 16.0 · SDK ",
    " dylibs · UUID ",
    "\"markers\":[]",
    "\"support\":\"supported\"",
    "\"channel\":\"alpha\"",
    "\"channel\":\"beta\"",
    "\"channel\":\"stable\"",
    "\"system\":\"iOS ",
    "\"device\":\"iPhone ",
    "\"phoenix\":\"",
    "\"plugins\":\"",
    "\"crash\":\"",
    "\"build\":\"",
    "\"discord\":\"",
    "\"commit\":\"",
    "{\"astral\":\"",
);

fn to_char(value: u32) -> char {
    let mut offset = value;
    for (start, size) in RANGES {
        if offset < size {
            return char::from_u32(start + offset).unwrap_or('\u{4E00}');
        }
        offset -= size;
    }
    '\u{4E00}'
}

fn from_char(character: char) -> Option<u32> {
    let code = character as u32;
    let mut offset = 0;
    for (start, size) in RANGES {
        if (start..start + size).contains(&code) {
            return Some(offset + code - start);
        }
        offset += size;
    }
    None
}

fn deflate(text: &str) -> Vec<u8> {
    let mut compress = Compress::new(Compression::best(), false);
    let _ = compress.set_dictionary(DICTIONARY.as_bytes());
    let mut out = Vec::with_capacity(text.len() + 64);
    let _ = compress.compress_vec(text.as_bytes(), &mut out, FlushCompress::Finish);
    while out.len() == out.capacity() {
        out.reserve(256);
        if !matches!(
            compress.compress_vec(&[], &mut out, FlushCompress::Finish),
            Ok(Status::Ok | Status::BufError)
        ) {
            break;
        }
    }
    out
}

pub fn pack(json: &str) -> String {
    let mut text = format!("{LABEL}{PREFIX}");
    let (mut bits, mut held) = (0u32, 0u32);
    for byte in deflate(json) {
        bits = (bits << 8) | u32::from(byte);
        held += 8;
        while held >= BITS {
            held -= BITS;
            text.push(to_char((bits >> held) & MASK));
        }
        bits &= (1 << held) - 1;
    }
    if held > 0 {
        text.push(to_char((bits << (BITS - held)) & MASK));
    }
    text
}

pub fn unpack(alt: &str) -> Option<String> {
    let text = alt.strip_prefix(LABEL).unwrap_or(alt).strip_prefix(PREFIX)?;
    let (mut bytes, mut bits, mut held) = (Vec::new(), 0u32, 0u32);
    for character in text.chars() {
        bits = (bits << BITS) | from_char(character)?;
        held += BITS;
        while held >= 8 {
            held -= 8;
            bytes.push(((bits >> held) & 0xFF) as u8);
        }
        bits &= (1 << held) - 1;
    }
    let mut decompress = Decompress::new(false);
    decompress.set_dictionary(DICTIONARY.as_bytes()).ok()?;
    let mut out = Vec::with_capacity(bytes.len() * 8);
    loop {
        match decompress.decompress_vec(&bytes[decompress.total_in() as usize..], &mut out, FlushDecompress::Finish) {
            Ok(Status::StreamEnd) => break,
            Ok(_) if out.len() == out.capacity() => out.reserve(4096),
            Ok(_) => break,
            Err(_) => return None,
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_far_under_the_json_and_round_trips() {
        let json = r#"{"astral":"0.0.1","commit":"d4dde0af3","loader":"astral.dylib","framework":"ElleKit 1.2-3","jailbreak":"roothide","discord":"357.0","channel":"alpha","build":"124593","support":"supported","source":"Sideloaded","install":"Distribution (53Q6R32WPB)","device":"iPhone 16","system":"iOS 27.2.1","binary":"decrypted · min 16.0 · SDK 27.1 · 108 dylibs · UUID 44799556","markers":[],"https":{"intercepted":false,"issuer":"GTS Root R4"},"phoenix":"bc893919-ca31-3575-b708-ee4d03486c1f","plugins":"48/49"}"#;
        let packed = pack(json);
        assert!(packed.starts_with("ASTRAL_DEBUG_COMPRESSED:ast1:"));
        assert!(packed.chars().count() < json.len() / 2);
        assert_eq!(unpack(&packed).as_deref(), Some(json));
        for length in 0..40 {
            let payload = format!("{{\"astral\":\"{}\"}}", "x".repeat(length));
            assert_eq!(unpack(&pack(&payload)).as_deref(), Some(payload.as_str()));
        }
        assert!(unpack("{\"astral\":\"0.0.1\"}").is_none());
    }
}
