use super::phoenix_gate::{self, RETURN_TRUE};

fn little32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(offset..offset.checked_add(4)?)?.try_into().ok()?))
}

fn little64(data: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(data.get(offset..offset.checked_add(8)?)?.try_into().ok()?))
}

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

struct Elf<'a> {
    data: &'a [u8],
    sections: Vec<(String, u64, u64, u64)>,
}

impl<'a> Elf<'a> {
    fn parse(data: &'a [u8]) -> Result<Elf<'a>, String> {
        let shoff = little64(data, 40).ok_or("truncated ELF")? as usize;
        let shentsize = little16(data, 58).ok_or("truncated ELF")? as usize;
        let shnum = little16(data, 60).ok_or("truncated ELF")? as usize;
        let shstrndx = little16(data, 62).ok_or("truncated ELF")? as usize;
        if shentsize < 64 || shstrndx >= shnum {
            return Err("invalid ELF section table".into());
        }
        let section = |index: usize| -> Option<(u32, u64, u64, u64)> {
            let base = shoff.checked_add(index.checked_mul(shentsize)?)?;
            Some((
                little32(data, base)?,
                little64(data, base + 16)?,
                little64(data, base + 24)?,
                little64(data, base + 32)?,
            ))
        };
        let (_, _, offset, size) = section(shstrndx).ok_or("no section string table")?;
        let strings = data
            .get(offset as usize..offset.checked_add(size).ok_or("invalid ELF string table")? as usize)
            .ok_or("truncated ELF string table")?;
        let mut sections = Vec::new();
        for index in 0..shnum {
            let (name_off, addr, offset, size) = section(index).ok_or("truncated section header")?;
            let name = strings.get(name_off as usize..).ok_or("invalid ELF section name")?;
            let end = name.iter().position(|byte| *byte == 0).ok_or("unterminated ELF section name")?;
            let name = String::from_utf8_lossy(&name[..end]).into_owned();
            if little32(data, shoff + index * shentsize + 4) != Some(8)
                && data
                    .get(offset as usize..offset.checked_add(size).ok_or("invalid ELF section size")? as usize)
                    .is_none()
            {
                return Err("ELF section exceeds the file".into());
            }
            sections.push((name, addr, offset, size));
        }
        Ok(Elf { data, sections })
    }

    fn section(&self, name: &str) -> Option<(u64, u64, u64)> {
        self.sections
            .iter()
            .find(|(section, ..)| section == name)
            .map(|(_, addr, offset, size)| (*addr, *offset, *size))
    }

    fn function_starts(&self) -> Vec<u64> {
        let Some((addr, offset, size)) = self.section(".eh_frame_hdr") else {
            return Vec::new();
        };
        let base = offset as usize;
        if self.data.get(base..base + 4) != Some(&[1, 0x1b, 3, 0x3b]) {
            return Vec::new();
        }
        let Some(count) = little32(self.data, base + 8) else {
            return Vec::new();
        };
        if 12 + u64::from(count) * 8 > size {
            return Vec::new();
        }
        let mut starts = Vec::new();
        for index in 0..count as usize {
            let entry = base + 12 + index * 8;
            let Some(relative) = little32(self.data, entry) else {
                break;
            };
            starts.push(addr.wrapping_add(i64::from(relative as i32) as u64));
        }
        starts.sort_unstable();
        starts.dedup();
        starts
    }

    fn string_addresses(&self) -> Vec<u64> {
        let mut found = Vec::new();
        for name in [".rodata", ".data.rel.ro"] {
            if let Some((addr, offset, size)) = self.section(name) {
                let bytes = &self.data[offset as usize..(offset + size) as usize];
                found.extend(phoenix_gate::experiment_offsets(bytes).map(|offset| addr + offset as u64));
            }
        }
        found
    }

    fn evaluators(&self) -> Result<Vec<u64>, String> {
        let targets = self.string_addresses();
        if targets.is_empty() {
            return Ok(Vec::new());
        }
        if self.data.get(0..6) != Some(&[0x7f, b'E', b'L', b'F', 2, 1]) || little16(self.data, 18) != Some(183) {
            return Err("Phoenix access gate requires a little-endian arm64 ELF".into());
        }
        let mut descriptors = Vec::new();
        if let Some((addr, offset, size)) = self.section(".data.rel.ro") {
            let bytes = self
                .data
                .get(offset as usize..(offset + size) as usize)
                .ok_or("truncated .data.rel.ro")?;
            for (index, pointer) in bytes.as_chunks::<8>().0.iter().enumerate() {
                if targets.contains(&u64::from_le_bytes(*pointer)) {
                    descriptors.push(addr + index as u64 * 8);
                }
            }
        }
        if let Some((_, offset, size)) = self.section(".rela.dyn") {
            let relocations = self
                .data
                .get(offset as usize..(offset + size) as usize)
                .ok_or("truncated relocations")?;
            for (address, addend) in decode_relocation_addends(relocations)? {
                if targets.contains(&addend) {
                    descriptors.push(address);
                }
            }
        }
        let (addr, offset, size) = self.section(".text").ok_or("no .text section")?;
        let code = self.data.get(offset as usize..(offset + size) as usize).ok_or("truncated .text")?;
        phoenix_gate::evaluators(code, addr, &self.function_starts(), &descriptors).map(Vec::from)
    }
}

fn little16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(offset..offset.checked_add(2)?)?.try_into().ok()?))
}

pub fn patch_access_gate(mut data: Vec<u8>) -> Result<Vec<u8>, String> {
    if data.get(0..4) != Some(&ELF_MAGIC) {
        return Err("liblibdiscord_wasm_host.so is not an ELF object".into());
    }
    let evaluators = Elf::parse(&data)?.evaluators()?;
    for evaluator in evaluators {
        let (addr, offset, size) = Elf::parse(&data)?.section(".text").ok_or("no .text section")?;
        if evaluator < addr || evaluator >= addr + size {
            return Err(format!("access gate evaluator {evaluator:#x} is outside .text"));
        }
        let at = (offset + (evaluator - addr)) as usize;
        data.get_mut(at..at + RETURN_TRUE.len())
            .ok_or_else(|| format!("access gate evaluator at {at:#x} runs past the file"))?
            .copy_from_slice(&RETURN_TRUE);
    }
    Ok(data)
}

pub fn opens_access_gate(data: &[u8]) -> bool {
    data.get(0..4) == Some(&ELF_MAGIC)
        && Elf::parse(data)
            .and_then(|elf| elf.evaluators())
            .map(|evaluators| !evaluators.is_empty())
            .unwrap_or(false)
}

fn signed_leb(bytes: &[u8], cursor: &mut usize) -> Result<i64, String> {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        let byte = *bytes.get(*cursor).ok_or("truncated packed relocation")?;
        *cursor += 1;
        if shift >= 64 {
            return Err("packed relocation integer overflow".into());
        }
        value |= u64::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            if shift < 64 && byte & 0x40 != 0 {
                value |= u64::MAX << shift;
            }
            return Ok(value as i64);
        }
    }
}

fn decode_relocation_addends(bytes: &[u8]) -> Result<Vec<(u64, u64)>, String> {
    if !bytes.starts_with(b"APS2") {
        if !bytes.len().is_multiple_of(24) {
            return Err("truncated ELF relocations".into());
        }
        return Ok(bytes
            .as_chunks::<24>()
            .0
            .iter()
            .filter_map(|entry| (little64(entry, 8)? as u32 == 1027).then_some((little64(entry, 0)?, little64(entry, 16)?)))
            .collect());
    }
    let mut cursor = 4;
    let mut remaining = signed_leb(bytes, &mut cursor)?;
    let mut address = signed_leb(bytes, &mut cursor)?;
    let mut addend = 0i64;
    if remaining < 0 || remaining as usize > bytes.len() * 16 {
        return Err("invalid packed relocation count".into());
    }
    let mut found = Vec::new();
    while remaining > 0 {
        let count = signed_leb(bytes, &mut cursor)?;
        let flags = signed_leb(bytes, &mut cursor)?;
        if count <= 0 || count > remaining || flags & !15 != 0 || flags & 4 != 0 && flags & 8 == 0 {
            return Err("invalid packed relocation group".into());
        }
        let delta = if flags & 2 != 0 { signed_leb(bytes, &mut cursor)? } else { 0 };
        let info = if flags & 1 != 0 { signed_leb(bytes, &mut cursor)? } else { 0 };
        let increment = if flags & 12 == 12 { signed_leb(bytes, &mut cursor)? } else { 0 };
        for _ in 0..count {
            address = address
                .checked_add(if flags & 2 != 0 { delta } else { signed_leb(bytes, &mut cursor)? })
                .ok_or("relocation address overflow")?;
            let info = if flags & 1 != 0 { info } else { signed_leb(bytes, &mut cursor)? };
            if flags & 8 == 0 {
                addend = 0;
            } else {
                addend = addend
                    .checked_add(if flags & 4 != 0 {
                        increment
                    } else {
                        signed_leb(bytes, &mut cursor)?
                    })
                    .ok_or("relocation addend overflow")?;
            }
            if info as u32 == 1027 {
                found.push((address as u64, addend as u64));
            }
        }
        remaining -= count;
    }
    Ok(found)
}

#[derive(Clone)]
pub struct GateSite {
    pub address: u64,
    pub original: [u8; 32],
}

pub fn patch_plan(data: &[u8]) -> Result<Vec<GateSite>, String> {
    if data.get(0..4) != Some(&ELF_MAGIC) {
        return Err("Phoenix host is not an ELF object".into());
    }
    let elf = Elf::parse(data)?;
    let entries = elf.evaluators()?;
    let (address, offset, size) = elf.section(".text").ok_or("no .text section")?;
    entries
        .into_iter()
        .map(|entry| {
            let relative = entry
                .checked_sub(address)
                .filter(|relative| relative.checked_add(32).is_some_and(|end| end <= size))
                .ok_or("Phoenix evaluator is outside .text")?;
            let at = (offset + relative) as usize;
            let original = data
                .get(at..at + 32)
                .ok_or("truncated Phoenix evaluator")?
                .try_into()
                .map_err(|_| "invalid Phoenix evaluator")?;
            Ok(GateSite { address: entry, original })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_grouped_android_decode_relocation_addends() {
        let mut bytes = b"APS2".to_vec();
        bytes.extend([2, 0, 2, 15, 8, 0x83, 8, 0xe4, 0]);
        assert_eq!(decode_relocation_addends(&bytes).unwrap(), vec![(8, 100), (16, 200)]);
    }

    #[test]
    fn rejects_invalid_packed_relocation_groups() {
        for data in [b"APS2".as_slice(), b"APS2\x01\x00\x00\x00", b"APS2\x01\x00\x02\x0f"] {
            assert!(decode_relocation_addends(data).is_err());
        }
    }

    #[test]
    fn truncated_elf_is_rejected_without_reading_outside_the_file() {
        for size in 0..128 {
            assert!(patch_plan(&vec![0; size]).is_err());
        }
    }
}
