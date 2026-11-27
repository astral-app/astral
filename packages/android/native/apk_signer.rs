use std::io::{Read, Seek, SeekFrom};

use sha2::{Digest, Sha256};

const END_OF_DIRECTORY: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const END_OF_DIRECTORY_SIZE: usize = 22;
const DIRECTORY_OFFSET_FIELD: usize = 16;
const COMMENT_LIMIT: usize = 0xffff;
const BLOCK_MAGIC: &[u8; 16] = b"APK Sig Block 42";
const BLOCK_FOOTER: usize = 24;
const SIGNATURE_V3_1: u32 = 0x1b93_ad61;
const SIGNATURE_V3: u32 = 0xf053_68c0;
const SIGNATURE_V2: u32 = 0x7109_871a;
const SEQUENCE: u8 = 0x30;
const SET: u8 = 0x31;
const OBJECT_IDENTIFIER: u8 = 0x06;
const EXPLICIT_VERSION: u8 = 0xa0;
const ORGANIZATION: [u8; 3] = [0x55, 0x04, 0x0a];
const COMMON_NAME: [u8; 3] = [0x55, 0x04, 0x03];
const LONG_LENGTH: u8 = 0x80;
const SUBJECT_FIELD: usize = 5;

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn u64_at(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

fn directory_offset(tail: &[u8], tail_start: u64) -> Option<u64> {
    let start = tail.len().checked_sub(END_OF_DIRECTORY_SIZE)?;
    let record = (0..=start)
        .rev()
        .find(|offset| tail[*offset..].starts_with(&END_OF_DIRECTORY))?;
    let offset = u64::from(u32_at(tail, record + DIRECTORY_OFFSET_FIELD)?);
    (offset <= tail_start + u64::try_from(record).ok()?).then_some(offset)
}

fn read_at(file: &mut (impl Read + Seek), offset: u64, length: usize) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

fn signing_block(file: &mut (impl Read + Seek)) -> Option<Vec<u8>> {
    let length = file.seek(SeekFrom::End(0)).ok()?;
    let tail_length = length.min(u64::try_from(END_OF_DIRECTORY_SIZE + COMMENT_LIMIT).ok()?);
    let tail_start = length - tail_length;
    let tail = read_at(file, tail_start, usize::try_from(tail_length).ok()?)?;
    let directory = directory_offset(&tail, tail_start)?;
    let footer_start = directory.checked_sub(u64::try_from(BLOCK_FOOTER).ok()?)?;
    let footer = read_at(file, footer_start, BLOCK_FOOTER)?;
    if footer.get(8..)? != BLOCK_MAGIC {
        return None;
    }
    let size = u64_at(&footer, 0)?;
    let start = directory.checked_sub(size + 8)?;
    let pairs_length = usize::try_from(size).ok()?.checked_sub(BLOCK_FOOTER - 8)?;
    read_at(file, start + 8, pairs_length)
}

fn blocks(pairs: &[u8]) -> Vec<(u32, &[u8])> {
    let mut found = Vec::new();
    let mut offset = 0;
    while offset + 12 <= pairs.len() {
        let (Some(length), Some(id)) = (
            u64_at(pairs, offset).and_then(|length| usize::try_from(length).ok()),
            u32_at(pairs, offset + 8),
        ) else {
            break;
        };
        let Some(value) = pairs.get(offset + 12..offset + 8 + length) else {
            break;
        };
        found.push((id, value));
        offset += 8 + length;
    }
    found
}

fn prefixed(bytes: &[u8], offset: usize) -> Option<(&[u8], usize)> {
    let length = usize::try_from(u32_at(bytes, offset)?).ok()?;
    Some((
        bytes.get(offset + 4..offset + 4 + length)?,
        offset + 4 + length,
    ))
}

fn first_certificate(block: &[u8]) -> Option<&[u8]> {
    let (signers, _) = prefixed(block, 0)?;
    let (signer, _) = prefixed(signers, 0)?;
    let (signed_data, _) = prefixed(signer, 0)?;
    let (_, after_digests) = prefixed(signed_data, 0)?;
    let (certificates, _) = prefixed(signed_data, after_digests)?;
    prefixed(certificates, 0).map(|(certificate, _)| certificate)
}

pub fn certificate(file: &mut (impl Read + Seek)) -> Option<Vec<u8>> {
    let pairs = signing_block(file)?;
    let found = blocks(&pairs);
    [SIGNATURE_V3_1, SIGNATURE_V3, SIGNATURE_V2]
        .iter()
        .find_map(|wanted| {
            found
                .iter()
                .find(|(id, _)| id == wanted)
                .and_then(|(_, block)| first_certificate(block))
                .map(<[u8]>::to_vec)
        })
}

pub fn digest(certificate: &[u8]) -> String {
    Sha256::digest(certificate)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn element(bytes: &[u8], offset: usize) -> Option<(u8, &[u8], usize)> {
    let tag = *bytes.get(offset)?;
    let first = *bytes.get(offset + 1)?;
    let (length, header) = if first & LONG_LENGTH == 0 {
        (usize::from(first), 2)
    } else {
        let count = usize::from(first & !LONG_LENGTH);
        let length = bytes
            .get(offset + 2..offset + 2 + count)?
            .iter()
            .fold(0usize, |total, byte| (total << 8) | usize::from(*byte));
        (length, 2 + count)
    };
    let body = bytes.get(offset + header..offset + header + length)?;
    Some((tag, body, offset + header + length))
}

fn children(bytes: &[u8]) -> Vec<(u8, &[u8])> {
    let mut found = Vec::new();
    let mut offset = 0;
    while let Some((tag, body, next)) = element(bytes, offset) {
        found.push((tag, body));
        offset = next;
    }
    found
}

fn attribute<'a>(subject: &'a [u8], wanted: &[u8]) -> Option<&'a [u8]> {
    children(subject)
        .into_iter()
        .filter(|(tag, _)| *tag == SET)
        .flat_map(|(_, set)| children(set))
        .filter(|(tag, _)| *tag == SEQUENCE)
        .find_map(|(_, pair)| {
            let parts = children(pair);
            let (tag, oid) = *parts.first()?;
            (tag == OBJECT_IDENTIFIER && oid == wanted)
                .then(|| parts.get(1).map(|(_, value)| *value))?
        })
}

pub fn subject(certificate: &[u8]) -> String {
    let Some((SEQUENCE, outer, _)) = element(certificate, 0) else {
        return String::new();
    };
    let Some((SEQUENCE, signed, _)) = element(outer, 0) else {
        return String::new();
    };
    let fields = children(signed);
    let skip = usize::from(
        fields
            .first()
            .is_some_and(|(tag, _)| *tag == EXPLICIT_VERSION),
    );
    let Some((SEQUENCE, subject)) = fields.get(skip + SUBJECT_FIELD - 1).copied() else {
        return String::new();
    };
    [("O=", ORGANIZATION), ("CN=", COMMON_NAME)]
        .iter()
        .find_map(|(label, oid)| {
            attribute(subject, oid)
                .map(|value| format!("{label}{}", String::from_utf8_lossy(value)))
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn der(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![tag, u8::try_from(body.len()).unwrap()];
        out.extend_from_slice(body);
        out
    }

    fn name(oid: [u8; 3], value: &str) -> Vec<u8> {
        let mut pair = der(OBJECT_IDENTIFIER, &oid);
        pair.extend(der(0x0c, value.as_bytes()));
        der(SET, &der(SEQUENCE, &pair))
    }

    fn certificate_with(subject_names: &[u8]) -> Vec<u8> {
        let mut signed = der(EXPLICIT_VERSION, &[2, 1, 2]);
        for _ in 0..3 {
            signed.extend(der(SEQUENCE, &[]));
        }
        signed.extend(der(SEQUENCE, &[]));
        signed.extend(der(SEQUENCE, subject_names));
        der(SEQUENCE, &der(SEQUENCE, &signed))
    }

    fn lengthed(body: &[u8]) -> Vec<u8> {
        let mut out = u32::try_from(body.len()).unwrap().to_le_bytes().to_vec();
        out.extend_from_slice(body);
        out
    }

    fn apk_with(certificate: &[u8]) -> Vec<u8> {
        let mut signed_data = lengthed(&[]);
        signed_data.extend(lengthed(&lengthed(certificate)));
        let block = lengthed(&lengthed(&lengthed(&signed_data)));
        let mut pair = (u64::try_from(block.len() + 4).unwrap())
            .to_le_bytes()
            .to_vec();
        pair.extend(SIGNATURE_V2.to_le_bytes());
        pair.extend(&block);
        let size = u64::try_from(pair.len() + BLOCK_FOOTER).unwrap();
        let mut apk = vec![0u8; 4];
        apk.extend(size.to_le_bytes());
        apk.extend(&pair);
        apk.extend(size.to_le_bytes());
        apk.extend(BLOCK_MAGIC);
        let directory = u32::try_from(apk.len()).unwrap();
        let mut record = END_OF_DIRECTORY.to_vec();
        record.extend([0u8; 12]);
        record.extend(directory.to_le_bytes());
        record.extend([0u8; 2]);
        apk.extend(record);
        apk
    }

    #[test]
    fn reads_the_signer_from_the_signing_block() {
        let certificate = certificate_with(&name(COMMON_NAME, "Jason"));
        let apk = apk_with(&certificate);
        assert_eq!(
            super::certificate(&mut std::io::Cursor::new(apk)).as_deref(),
            Some(certificate.as_slice())
        );
        assert_eq!(digest(b"").len(), 64);
    }

    #[test]
    fn prefers_the_organisation_in_the_subject() {
        let mut names = name(COMMON_NAME, "Jason Citron");
        names.extend(name(ORGANIZATION, "Hammer and Chisel"));
        assert_eq!(subject(&certificate_with(&names)), "O=Hammer and Chisel");
        assert_eq!(
            subject(&certificate_with(&name(COMMON_NAME, "Android"))),
            "CN=Android"
        );
        assert_eq!(subject(&[1, 2, 3]), "");
    }

    #[test]
    fn unsigned_files_have_no_signer() {
        assert!(super::certificate(&mut std::io::Cursor::new(b"not an apk".to_vec())).is_none());
    }
}
