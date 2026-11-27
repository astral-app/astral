use core::ffi::{CStr, c_char, c_int};

const MH_MAGIC_64: u32 = 0xfeed_facf;
const LC_SEGMENT_64: u32 = 0x19;
const LC_FUNCTION_STARTS: u32 = 0x26;
const SECTION_TYPE: u32 = 0xff;
const S_ZEROFILL: u32 = 0x1;
const HEADER_SIZE: usize = 32;
const SEGMENT_SIZE: usize = 72;
const SECTION_SIZE: usize = 80;
const MAX_SEGMENTS: usize = 16;
const MAX_TARGETS: usize = 24;
const MAX_STRING_SECTIONS: usize = 8;
const MAX_DATA_SECTIONS: usize = 32;
const POINTER_SLOTS: usize = 16;
const PROPERTY_ANCHOR: [u8; 16] = [
    b't', b'e', b'x', b't', b'T', b'r', b'a', b'n', b's', b'f', b'o', b'r', b'm', 0, 0, 0xED,
];

#[repr(C)]
pub struct AstralImageView {
    pub image: *const u8,
    pub length: usize,
    pub slide: i64,
    pub file: c_int,
}

#[derive(Clone, Copy)]
struct Segment {
    vmaddr: u64,
    vmsize: u64,
    fileoff: u64,
}

#[derive(Clone, Copy)]
struct Section {
    addr: u64,
    size: u64,
}

struct Image<'a> {
    file: Option<&'a [u8]>,
    base: *const u8,
    segments: Vec<Segment>,
    text_base: u64,
    linkedit_vm: u64,
    linkedit_file: u64,
    slide: i64,
    text: Section,
    strings: Vec<Section>,
    data: Vec<Section>,
    starts: (u32, u32),
}

fn little32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(offset..offset.checked_add(4)?)?.try_into().ok()?))
}

fn little64(data: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(data.get(offset..offset.checked_add(8)?)?.try_into().ok()?))
}

fn name(data: &[u8], offset: usize) -> &[u8] {
    let field = data.get(offset..offset + 16).unwrap_or(&[]);
    &field[..field.iter().position(|byte| *byte == 0).unwrap_or(field.len())]
}

impl<'a> Image<'a> {
    fn bytes(&self, vmaddr: u64, length: u64) -> Option<&'a [u8]> {
        let segment = self.segments.iter().find(|segment| {
            let end = vmaddr.checked_add(length);
            vmaddr >= segment.vmaddr && end.is_some_and(|end| end <= segment.vmaddr.saturating_add(segment.vmsize))
        })?;
        let length = usize::try_from(length).ok()?;
        match self.file {
            Some(file) => {
                let offset = usize::try_from(segment.fileoff.checked_add(vmaddr - segment.vmaddr)?).ok()?;
                file.get(offset..offset.checked_add(length)?)
            }
            None => {
                let address = (vmaddr as i64).checked_add(self.slide)? as usize;
                Some(unsafe { core::slice::from_raw_parts(address as *const u8, length) })
            }
        }
    }

    fn function_starts(&self) -> Option<&'a [u8]> {
        let (offset, size) = self.starts;
        match self.file {
            Some(file) => file.get(offset as usize..(offset as usize).checked_add(size as usize)?),
            None => self.bytes(
                self.linkedit_vm.checked_add(u64::from(offset).checked_sub(self.linkedit_file)?)?,
                u64::from(size),
            ),
        }
    }

    fn function_start(&self, address: u64) -> u64 {
        let Some(starts) = self.function_starts() else { return 0 };
        let (mut current, mut best, mut index) = (self.text_base, 0, 0);
        while index < starts.len() {
            let (mut value, mut shift) = (0u64, 0);
            loop {
                let byte = starts[index];
                index += 1;
                if shift < 64 {
                    value |= u64::from(byte & 0x7f) << shift;
                }
                shift += 7;
                if byte & 0x80 == 0 || index >= starts.len() {
                    break;
                }
            }
            if value == 0 {
                break;
            }
            current = current.wrapping_add(value);
            if current > address {
                break;
            }
            best = current;
        }
        best
    }

    fn text_words(&self) -> Option<&'a [[u8; 4]]> {
        Some(self.bytes(self.text.addr, self.text.size)?.as_chunks::<4>().0)
    }
}

fn parse(view: &AstralImageView) -> Option<Image<'_>> {
    if view.image.is_null() {
        return None;
    }
    let file = (view.file != 0).then(|| unsafe { core::slice::from_raw_parts(view.image, view.length) });
    let header = match file {
        Some(file) => file,
        None => {
            let preamble = unsafe { core::slice::from_raw_parts(view.image, HEADER_SIZE) };
            let commands = little32(preamble, 20)? as usize;
            unsafe { core::slice::from_raw_parts(view.image, HEADER_SIZE + commands) }
        }
    };
    if little32(header, 0)? != MH_MAGIC_64 {
        return None;
    }
    let mut image = Image {
        file,
        base: view.image,
        segments: Vec::new(),
        text_base: 0,
        linkedit_vm: 0,
        linkedit_file: 0,
        slide: 0,
        text: Section { addr: 0, size: 0 },
        strings: Vec::new(),
        data: Vec::new(),
        starts: (0, 0),
    };
    let mut text = None;
    let mut starts = None;
    let mut cursor = HEADER_SIZE;
    for _ in 0..little32(header, 16)? {
        let command = little32(header, cursor)?;
        let size = little32(header, cursor + 4)? as usize;
        if command == LC_SEGMENT_64 {
            let segment = Segment {
                vmaddr: little64(header, cursor + 24)?,
                vmsize: little64(header, cursor + 32)?,
                fileoff: little64(header, cursor + 40)?,
            };
            if image.segments.len() < MAX_SEGMENTS {
                image.segments.push(segment);
            }
            match name(header, cursor + 8) {
                b"__TEXT" => image.text_base = segment.vmaddr,
                b"__LINKEDIT" => {
                    image.linkedit_vm = segment.vmaddr;
                    image.linkedit_file = segment.fileoff;
                }
                _ => {}
            }
            for index in 0..little32(header, cursor + 64)? as usize {
                let at = cursor + SEGMENT_SIZE + index * SECTION_SIZE;
                let section = Section {
                    addr: little64(header, at + 32)?,
                    size: little64(header, at + 40)?,
                };
                let flags = little32(header, at + 64)?;
                if name(header, at + 16).starts_with(b"__DATA")
                    && flags & SECTION_TYPE != S_ZEROFILL
                    && image.data.len() < MAX_DATA_SECTIONS
                {
                    image.data.push(section);
                }
                if name(header, at) == b"__const"
                    && !name(header, at + 16).starts_with(b"__DATA")
                    && flags & SECTION_TYPE != S_ZEROFILL
                    && image.strings.len() < MAX_STRING_SECTIONS
                {
                    image.strings.push(section);
                }
                if name(header, at + 16) != b"__TEXT" {
                    continue;
                }
                match name(header, at) {
                    b"__text" => text = Some(section),
                    b"__oslogstring" | b"__cstring" if image.strings.len() < MAX_STRING_SECTIONS => image.strings.push(section),
                    _ => {}
                }
            }
        } else if command == LC_FUNCTION_STARTS {
            starts = Some((little32(header, cursor + 8)?, little32(header, cursor + 12)?));
        }
        cursor = cursor.checked_add(size)?;
    }
    image.text = text?;
    image.starts = starts?;
    image.slide = (image.base as i64).wrapping_sub(image.text_base as i64);
    Some(image)
}

fn needle_addresses(image: &Image, section: Section, needle: &[u8], out: &mut Vec<u64>) {
    let Some(bytes) = image.bytes(section.addr, section.size) else {
        return;
    };
    if needle.is_empty() || bytes.len() < needle.len() {
        return;
    }
    let mut offset = 0;
    while offset + needle.len() <= bytes.len() {
        if &bytes[offset..offset + needle.len()] == needle {
            out.push(section.addr + offset as u64);
            if out.len() >= MAX_TARGETS {
                return;
            }
        }
        offset += 1;
    }
}

fn find_string(image: &Image, section: Section, needle: &[u8]) -> u64 {
    let Some(bytes) = image.bytes(section.addr, section.size) else {
        return 0;
    };
    let Some(offset) = bytes.windows(needle.len()).position(|window| window == needle) else {
        return 0;
    };
    let start = bytes[..offset].iter().rposition(|byte| *byte == 0).map_or(0, |zero| zero + 1);
    section.addr + start as u64
}

fn matching_pcs(image: &Image, words: &[[u8; 4]], matches: impl Fn(&[[u8; 4]], usize, u32) -> bool + Sync) -> Vec<u64> {
    let threads = std::thread::available_parallelism().map_or(1, usize::from).clamp(1, 8);
    let chunk = words.len().div_ceil(threads).max(1);
    let text = image.text.addr;
    let mut found: Vec<u64> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..words.len())
            .step_by(chunk)
            .map(|first| {
                let matches = &matches;
                scope.spawn(move || {
                    (first..(first + chunk).min(words.len()))
                        .filter(|index| matches(words, *index, u32::from_le_bytes(words[*index])))
                        .map(|index| text + index as u64 * 4)
                        .collect::<Vec<u64>>()
                })
            })
            .collect();
        workers.into_iter().flat_map(|worker| worker.join().unwrap_or_default()).collect()
    });
    found.sort_unstable();
    found
}

fn single_function(image: &Image, pcs: &[u64]) -> u64 {
    let mut found = 0;
    for pc in pcs {
        let function = image.function_start(*pc);
        if function == 0 || (found != 0 && found != function) {
            return 0;
        }
        found = function;
    }
    if found != 0 { found - image.text_base } else { 0 }
}

fn dominant_function(image: &Image, pcs: &[u64]) -> u64 {
    use std::collections::HashMap;
    let mut votes: HashMap<u64, usize> = HashMap::new();
    for pc in pcs {
        let function = image.function_start(*pc);
        if function != 0 {
            *votes.entry(function).or_default() += 1;
        }
    }
    votes
        .into_iter()
        .max_by_key(|(function, count)| (*count, *function))
        .map_or(0, |(function, _)| function - image.text_base)
}

fn decode_branch_target(pc: u64, word: u32) -> Option<u64> {
    if word & 0xfc00_0000 != 0x9400_0000 {
        return None;
    }
    let mut immediate = i64::from(word & 0x03ff_ffff);
    if immediate & (1 << 25) != 0 {
        immediate -= 1 << 26;
    }
    Some(pc.wrapping_add((immediate * 4) as u64))
}

fn unique_caller(image: &Image, callee: u64) -> u64 {
    if callee == 0 {
        return 0;
    }
    let Some(words) = image.text_words() else { return 0 };
    let target = image.text_base.wrapping_add(callee);
    let text = image.text.addr;
    let pcs = matching_pcs(image, words, |_, index, word| {
        decode_branch_target(text + index as u64 * 4, word) == Some(target)
    });
    single_function(image, &pcs)
}

fn callers(image: &Image, callee: u64) -> Vec<u64> {
    if callee == 0 {
        return Vec::new();
    }
    let Some(words) = image.text_words() else { return Vec::new() };
    let target = image.text_base.wrapping_add(callee);
    let text = image.text.addr;
    let pcs = matching_pcs(image, words, |_, index, word| {
        decode_branch_target(text + index as u64 * 4, word) == Some(target)
    });
    let mut functions = Vec::new();
    for pc in pcs {
        let function = image.function_start(pc);
        if function != 0 {
            let offset = function - image.text_base;
            if !functions.contains(&offset) {
                functions.push(offset);
            }
        }
    }
    functions
}

fn pointer_to(image: &Image, offset: u64) -> Vec<u64> {
    if offset == 0 {
        return Vec::new();
    }
    let target = (image.text_base.wrapping_add(offset) as i64).wrapping_add(image.slide) as u64;
    let needle = target.to_le_bytes();
    let mut found = Vec::new();
    for section in &image.data {
        let Some(bytes) = image.bytes(section.addr, section.size) else {
            continue;
        };
        for (index, window) in bytes.as_chunks::<8>().0.iter().enumerate() {
            if *window == needle {
                let slot = section.addr + (index as u64) * 8;
                found.push((slot as i64).wrapping_add(image.slide) as u64);
                if found.len() >= POINTER_SLOTS {
                    return found;
                }
            }
        }
    }
    found
}

fn decode_address_reference(words: &[[u8; 4]], index: usize, word: u32, text: u64) -> Option<u64> {
    if word & 0x9f00_0000 != 0x9000_0000 {
        return None;
    }
    let register = word & 31;
    let mut immediate = i64::from((((word >> 5) & 0x7ffff) << 2) | ((word >> 29) & 3));
    if immediate & (1 << 20) != 0 {
        immediate -= 1 << 21;
    }
    let pc = text + index as u64 * 4;
    let page = (pc & !0xfff).wrapping_add((immediate << 12) as u64);
    words[index + 1..]
        .iter()
        .take(4)
        .map(|add| u32::from_le_bytes(*add))
        .find(|add| add & 0xffc0_0000 == 0x9100_0000 && (add >> 5) & 31 == register)
        .map(|add| page + u64::from((add >> 10) & 0xfff))
}

fn function_referencing(image: &Image, needle: &[u8]) -> u64 {
    if needle.is_empty() {
        return 0;
    }
    let mut targets = Vec::new();
    for section in &image.strings {
        if targets.len() + 2 > MAX_TARGETS {
            break;
        }
        let address = find_string(image, *section, needle);
        if address != 0 {
            targets.extend([address, address.wrapping_sub(0x20)]);
        }
    }
    if targets.is_empty() {
        return 0;
    }
    let Some(words) = image.text_words() else { return 0 };
    let text = image.text.addr;
    let pcs = matching_pcs(image, words, |words, index, word| {
        decode_address_reference(words, index, word, text).is_some_and(|target| targets.contains(&target))
    });
    single_function(image, &pcs)
}

fn const_referencing(image: &Image, needle: &[u8]) -> u64 {
    if needle.is_empty() {
        return 0;
    }
    let mut targets = Vec::new();
    for section in &image.strings {
        if targets.len() >= MAX_TARGETS {
            break;
        }
        needle_addresses(image, *section, needle, &mut targets);
    }
    if targets.is_empty() {
        return 0;
    }
    let Some(words) = image.text_words() else { return 0 };
    let text = image.text.addr;
    let pcs = matching_pcs(image, words, |words, index, word| {
        decode_address_reference(words, index, word, text).is_some_and(|target| targets.contains(&target))
    });
    dominant_function(image, &pcs)
}

fn printable(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(|byte| *byte > 32 && *byte < 127)
}

fn decode_string<'a>(image: &Image<'a>, entry: &'a [u8]) -> Option<&'a [u8]> {
    let discriminator = entry[15];
    if discriminator >> 4 == 0xE {
        let length = usize::from(discriminator & 0xF);
        if entry[length..15].iter().any(|byte| *byte != 0) {
            return None;
        }
        return printable(&entry[..length]).then(|| &entry[..length]);
    }
    let flags = little64(entry, 0)?;
    let object = little64(entry, 8)?;
    if !matches!(flags >> 60, 0x8 | 0x9 | 0xC | 0xD) {
        return None;
    }
    let length = flags & 0xFFFF_FFFF_FFFF;
    if length == 0 || length > 255 {
        return None;
    }
    let address = if image.file.is_some() {
        if (object >> 36) & 0xFF != 0x80 {
            return None;
        }
        image.text_base.wrapping_add(object & 0xF_FFFF_FFFF).wrapping_add(32)
    } else {
        if object >> 60 != 0x8 {
            return None;
        }
        (object & 0x0FFF_FFFF_FFFF_FFFF).wrapping_add(32).wrapping_sub(image.slide as u64)
    };
    image.bytes(address, length).filter(|bytes| printable(bytes))
}

fn property_names<'a>(image: &Image<'a>, capacity: usize) -> Vec<&'a [u8]> {
    for section in &image.data {
        let Some(bytes) = image.bytes(section.addr, section.size).filter(|bytes| bytes.len() >= 16) else {
            continue;
        };
        let mut offset = 0;
        while offset + 16 <= bytes.len() {
            if bytes[offset..offset + 16] != PROPERTY_ANCHOR {
                offset += 8;
                continue;
            }
            let mut start = offset;
            while start >= 16 && decode_string(image, &bytes[start - 16..start]).is_some() {
                start -= 16;
            }
            return bytes[start..]
                .as_chunks::<16>()
                .0
                .iter()
                .take(capacity)
                .map_while(|entry| decode_string(image, entry))
                .collect();
        }
    }
    Vec::new()
}

fn view<'a>(view: *const AstralImageView) -> Option<Image<'a>> {
    parse(unsafe { view.as_ref() }?)
}

fn phoenix_access_gate(image: &Image) -> Option<[u64; 2]> {
    use super::phoenix_gate;
    let mut strings = Vec::new();
    for section in &image.strings {
        let bytes = image.bytes(section.addr, section.size)?;
        strings.extend(phoenix_gate::experiment_offsets(bytes).map(|offset| section.addr + offset as u64));
    }
    let mut descriptors = Vec::new();
    for section in &image.data {
        let bytes = image.bytes(section.addr, section.size)?;
        for (index, pointer) in bytes.as_chunks::<8>().0.iter().enumerate() {
            let pointer = u64::from_le_bytes(*pointer);
            let target = if image.file.is_some() {
                image.text_base + (pointer & 0xffff_ffff)
            } else {
                pointer.wrapping_sub(image.slide as u64)
            };
            if strings.contains(&target) {
                descriptors.push(section.addr + index as u64 * 8);
            }
        }
    }
    let mut starts = Vec::new();
    let mut address = image.text_base;
    let (mut value, mut shift) = (0u64, 0);
    for byte in image.function_starts()? {
        if shift >= 64 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 != 0 {
            shift += 7;
            continue;
        }
        if value == 0 {
            break;
        }
        address = address.checked_add(value)?;
        if address >= image.text.addr && address < image.text.addr + image.text.size {
            starts.push(address);
        }
        value = 0;
        shift = 0;
    }
    phoenix_gate::evaluators(
        image.bytes(image.text.addr, image.text.size)?,
        image.text.addr,
        &starts,
        &descriptors,
    )
    .ok()
    .map(|entries| entries.map(|entry| entry - image.text_base))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_locate_phoenix_access_gate(image: *const AstralImageView, out: *mut u64, capacity: usize) -> usize {
    if out.is_null() || capacity < 2 {
        return 0;
    }
    let Some(entries) = view(image).and_then(|image| phoenix_access_gate(&image)) else {
        return 0;
    };
    unsafe { core::slice::from_raw_parts_mut(out, 2) }.copy_from_slice(&entries);
    2
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_locate_function_referencing(image: *const AstralImageView, needle: *const c_char) -> u64 {
    if needle.is_null() {
        return 0;
    }
    view(image).map_or(0, |image| {
        function_referencing(&image, unsafe { CStr::from_ptr(needle) }.to_bytes())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_locate_unique_caller(image: *const AstralImageView, callee: u64) -> u64 {
    view(image).map_or(0, |image| unique_caller(&image, callee))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_locate_const_referencing(image: *const AstralImageView, needle: *const c_char) -> u64 {
    if needle.is_null() {
        return 0;
    }
    view(image).map_or(0, |image| const_referencing(&image, unsafe { CStr::from_ptr(needle) }.to_bytes()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_locate_callers(image: *const AstralImageView, callee: u64, out: *mut u64, capacity: usize) -> usize {
    if out.is_null() {
        return 0;
    }
    let Some(image) = view(image) else { return 0 };
    let found = callers(&image, callee);
    let out = unsafe { core::slice::from_raw_parts_mut(out, capacity) };
    let count = found.len().min(capacity);
    out[..count].copy_from_slice(&found[..count]);
    count
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_locate_pointer_to(image: *const AstralImageView, offset: u64, out: *mut u64, capacity: usize) -> usize {
    if out.is_null() {
        return 0;
    }
    let Some(image) = view(image) else { return 0 };
    let found = pointer_to(&image, offset);
    let out = unsafe { core::slice::from_raw_parts_mut(out, capacity) };
    let count = found.len().min(capacity);
    out[..count].copy_from_slice(&found[..count]);
    count
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_locate_host_call(image: *const AstralImageView, ops_arrived: u64) -> u64 {
    view(image).map_or(0, |image| {
        unique_caller(&image, unique_caller(&image, unique_caller(&image, ops_arrived)))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_read_property_names(
    image: *const AstralImageView,
    names: *mut *const c_char,
    capacity: usize,
    storage: *mut c_char,
    storage_size: usize,
) -> usize {
    let Some(image) = view(image) else { return 0 };
    if names.is_null() || storage.is_null() {
        return 0;
    }
    let names = unsafe { core::slice::from_raw_parts_mut(names, capacity) };
    let storage = unsafe { core::slice::from_raw_parts_mut(storage.cast::<u8>(), storage_size) };
    let mut used = 0;
    let mut count = 0;
    for text in property_names(&image, capacity) {
        if used + text.len() + 1 > storage.len() {
            break;
        }
        storage[used..used + text.len()].copy_from_slice(text);
        storage[used + text.len()] = 0;
        names[count] = storage[used..].as_ptr().cast();
        used += text.len() + 1;
        count += 1;
    }
    count
}
