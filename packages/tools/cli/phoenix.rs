use astral_core::diagnostics::locate::{
    AstralImageView, astral_locate_callers, astral_locate_const_referencing,
};

const ACCESS_EXPERIMENT: &[u8] = b"2026-11-phoenix-app-access\0";
const RETURN_TRUE: [u8; 8] = [0x20, 0x00, 0x80, 0x52, 0xC0, 0x03, 0x5F, 0xD6];
const MH_MAGIC_64: u32 = 0xfeed_facf;
const LC_SEGMENT_64: u32 = 0x19;
const MAX_EVALUATORS: usize = 4;

fn little32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn little64(data: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        data.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

fn text_segment(data: &[u8]) -> Option<(u64, u64)> {
    let mut cursor = 32;
    for _ in 0..little32(data, 16)? {
        let command = little32(data, cursor)?;
        let size = little32(data, cursor + 4)? as usize;
        if command == LC_SEGMENT_64 && &data.get(cursor + 8..cursor + 14)? == b"__TEXT" {
            return Some((little64(data, cursor + 24)?, little64(data, cursor + 40)?));
        }
        cursor = cursor.checked_add(size)?;
    }
    None
}

fn evaluators(data: &[u8]) -> Result<Vec<u64>, String> {
    let view = AstralImageView {
        image: data.as_ptr(),
        length: data.len(),
        slide: 0,
        file: 1,
    };
    let helper =
        unsafe { astral_locate_const_referencing(&view, ACCESS_EXPERIMENT.as_ptr().cast()) };
    if helper == 0 {
        return Ok(Vec::new());
    }
    let mut found = [0u64; MAX_EVALUATORS];
    let count = unsafe { astral_locate_callers(&view, helper, found.as_mut_ptr(), MAX_EVALUATORS) };
    if count == 0 || count >= MAX_EVALUATORS {
        return Err(format!(
            "the Phoenix access gate has {count} evaluators; expected 1 to 3"
        ));
    }
    Ok(found[..count].to_vec())
}

pub fn patch_access_gate(mut data: Vec<u8>) -> Result<Vec<u8>, String> {
    if little32(&data, 0) != Some(MH_MAGIC_64) {
        return Err(
            "PhoenixFatClient is not a thin arm64 Mach-O; cannot open the access gate".into(),
        );
    }
    let (_vmaddr, fileoff) = text_segment(&data).ok_or("PhoenixFatClient has no __TEXT segment")?;
    for evaluator in evaluators(&data)? {
        let offset = usize::try_from(fileoff + evaluator).map_err(|error| error.to_string())?;
        let slot = data
            .get_mut(offset..offset + RETURN_TRUE.len())
            .ok_or_else(|| format!("access gate evaluator at {offset:#x} runs past the file"))?;
        slot.copy_from_slice(&RETURN_TRUE);
    }
    Ok(data)
}

pub fn access_gate_evaluators(data: &[u8]) -> usize {
    if little32(data, 0) != Some(MH_MAGIC_64) {
        return 0;
    }
    evaluators(data).map_or(0, |evaluators| evaluators.len())
}

pub fn opens_access_gate(data: &[u8]) -> bool {
    little32(data, 0) == Some(MH_MAGIC_64)
        && evaluators(data)
            .map(|evaluators| !evaluators.is_empty())
            .unwrap_or(false)
}

fn decode_adrp_add(words: &[[u8; 4]], index: usize, base: u64) -> Option<u64> {
    let word = u32::from_le_bytes(words[index]);
    if word & 0x9f00_0000 != 0x9000_0000 {
        return None;
    }
    let register = word & 31;
    let mut immediate = i64::from((((word >> 5) & 0x7ffff) << 2) | ((word >> 29) & 3));
    if immediate & (1 << 20) != 0 {
        immediate -= 1 << 21;
    }
    let pc = base + index as u64 * 4;
    let page = (pc & !0xfff).wrapping_add((immediate << 12) as u64);
    words[index + 1..]
        .iter()
        .take(4)
        .map(|add| u32::from_le_bytes(*add))
        .find_map(|add| {
            (add & 0xffc0_0000 == 0x9100_0000 && (add >> 5) & 31 == register)
                .then(|| page + u64::from((add >> 10) & 0xfff))
        })
}

fn decode_branch(pc: u64, word: u32) -> Option<u64> {
    if word & 0xfc00_0000 != 0x9400_0000 {
        return None;
    }
    let mut immediate = i64::from(word & 0x03ff_ffff);
    if immediate & (1 << 25) != 0 {
        immediate -= 1 << 26;
    }
    Some(pc.wrapping_add((immediate * 4) as u64))
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
        let section = |index: usize| -> Option<(u32, u64, u64, u64)> {
            let base = shoff + index * shentsize;
            Some((
                little32(data, base)?,
                little64(data, base + 16)?,
                little64(data, base + 24)?,
                little64(data, base + 32)?,
            ))
        };
        let strtab = section(shstrndx).ok_or("no section string table")?.2 as usize;
        let mut sections = Vec::new();
        for index in 0..shnum {
            let (name_off, addr, offset, size) =
                section(index).ok_or("truncated section header")?;
            let at = strtab + name_off as usize;
            let end = data[at..]
                .iter()
                .position(|byte| *byte == 0)
                .map_or(at, |length| at + length);
            let name = String::from_utf8_lossy(&data[at..end]).into_owned();
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
        let Some((addr, offset, _)) = self.section(".eh_frame_hdr") else {
            return Vec::new();
        };
        let base = offset as usize;
        let Some(count) = little32(self.data, base + 8) else {
            return Vec::new();
        };
        let mut starts = Vec::new();
        for index in 0..count as usize {
            let entry = base + 12 + index * 8;
            let Some(relative) = little32(self.data, entry) else {
                break;
            };
            starts.push(addr.wrapping_add(i64::from(relative as i32) as u64));
        }
        starts.sort_unstable();
        starts
    }

    fn function_containing(&self, starts: &[u64], pc: u64) -> u64 {
        match starts.binary_search(&pc) {
            Ok(_) => pc,
            Err(0) => 0,
            Err(index) => starts[index - 1],
        }
    }

    fn string_addresses(&self, needle: &[u8]) -> Vec<u64> {
        let mut found = Vec::new();
        for name in [".rodata", ".data.rel.ro"] {
            if let Some((addr, offset, size)) = self.section(name) {
                let bytes = &self.data[offset as usize..(offset + size) as usize];
                let mut at = 0;
                while at + needle.len() <= bytes.len() {
                    if &bytes[at..at + needle.len()] == needle {
                        found.push(addr + at as u64);
                    }
                    at += 1;
                }
            }
        }
        found
    }

    fn text_references(&self, targets: &[u64]) -> Vec<u64> {
        let Some((addr, offset, size)) = self.section(".text") else {
            return Vec::new();
        };
        let code = &self.data[offset as usize..(offset + size) as usize];
        let words = code.as_chunks::<4>().0;
        (0..words.len())
            .filter(|index| {
                decode_adrp_add(words, *index, addr)
                    .is_some_and(|reference| targets.contains(&reference))
            })
            .map(|index| addr + index as u64 * 4)
            .collect()
    }

    fn text_callers(&self, callee: u64) -> Vec<u64> {
        let Some((addr, offset, size)) = self.section(".text") else {
            return Vec::new();
        };
        let code = &self.data[offset as usize..(offset + size) as usize];
        let words = code.as_chunks::<4>().0;
        (0..words.len())
            .filter(|index| {
                decode_branch(addr + *index as u64 * 4, u32::from_le_bytes(words[*index]))
                    == Some(callee)
            })
            .map(|index| addr + index as u64 * 4)
            .collect()
    }

    fn evaluators(&self) -> Result<Vec<u64>, String> {
        let needle = &ACCESS_EXPERIMENT[..ACCESS_EXPERIMENT.len() - 1];
        let targets = self.string_addresses(needle);
        if targets.is_empty() {
            return Ok(Vec::new());
        }
        let starts = self.function_starts();
        if starts.is_empty() {
            return Err("liblibdiscord_wasm_host.so has no .eh_frame_hdr function table".into());
        }
        let mut votes: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
        for pc in self.text_references(&targets) {
            let function = self.function_containing(&starts, pc);
            if function != 0 {
                *votes.entry(function).or_default() += 1;
            }
        }
        let Some((helper, _)) = votes
            .into_iter()
            .max_by_key(|(function, count)| (*count, *function))
        else {
            return Ok(Vec::new());
        };
        let mut evaluators: Vec<u64> = self
            .text_callers(helper)
            .into_iter()
            .map(|pc| self.function_containing(&starts, pc))
            .filter(|function| *function != 0)
            .collect();
        evaluators.sort_unstable();
        evaluators.dedup();
        if evaluators.len() >= MAX_EVALUATORS {
            return Err(format!(
                "the Phoenix access gate has {} evaluators; expected 1 to 3",
                evaluators.len()
            ));
        }
        Ok(evaluators)
    }
}

fn little16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

pub fn patch_android_access_gate(mut data: Vec<u8>) -> Result<Vec<u8>, String> {
    if data.get(0..4) != Some(&ELF_MAGIC) {
        return Err("liblibdiscord_wasm_host.so is not an ELF object".into());
    }
    let evaluators = Elf::parse(&data)?.evaluators()?;
    for evaluator in evaluators {
        let (addr, offset, size) = Elf::parse(&data)?
            .section(".text")
            .ok_or("no .text section")?;
        if evaluator < addr || evaluator >= addr + size {
            return Err(format!(
                "access gate evaluator {evaluator:#x} is outside .text"
            ));
        }
        let at = (offset + (evaluator - addr)) as usize;
        data.get_mut(at..at + RETURN_TRUE.len())
            .ok_or_else(|| format!("access gate evaluator at {at:#x} runs past the file"))?
            .copy_from_slice(&RETURN_TRUE);
    }
    Ok(data)
}

pub fn opens_android_access_gate(data: &[u8]) -> bool {
    data.get(0..4) == Some(&ELF_MAGIC)
        && Elf::parse(data)
            .and_then(|elf| elf.evaluators())
            .map(|evaluators| !evaluators.is_empty())
            .unwrap_or(false)
}
