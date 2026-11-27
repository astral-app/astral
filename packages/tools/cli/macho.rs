pub const LOAD_DYLIB: u32 = 0xc;
pub const LOAD_WEAK_DYLIB: u32 = 0x8000_0018;
pub const ID_DYLIB: u32 = 0xd;
pub const CPU_ARM64: u32 = 0x0100_000c;
pub const TYPE_DYLIB: u32 = 6;
const SEGMENT_64: u32 = 0x19;
const CODE_SIGNATURE: u32 = 0x1d;
const MAGIC_64: u32 = 0xfeed_facf;
const MAGIC_32: u32 = 0xfeed_face;
const HEADER_64: usize = 32;
const HEADER_32: usize = 28;
const DYLIB_COMMAND: usize = 24;
const SUPER_BLOB: u32 = 0xfade_0cc0;
const ENTITLEMENTS_BLOB: u32 = 0xfade_7171;
const ENTITLEMENTS_SLOT: u32 = 5;
const APP_GROUPS: &str = "com.apple.security.application-groups";

pub struct MachO<'a> {
    pub data: &'a [u8],
    pub is_64: bool,
    pub cpu: u32,
    pub file_type: u32,
    pub command_count: u32,
    pub commands_size: u32,
    pub loads: Vec<&'a [u8]>,
}

fn little(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn big(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

pub fn parse(data: &[u8]) -> Result<MachO<'_>, String> {
    let header = match little(data, 0) {
        Some(MAGIC_64) => HEADER_64,
        Some(MAGIC_32) => HEADER_32,
        _ => return Err("not a little-endian thin Mach-O".into()),
    };
    let field = |offset| little(data, offset).ok_or("truncated Mach-O header");
    let command_count = field(16)?;
    let commands_size = field(20)?;
    let end = header + commands_size as usize;
    if end > data.len() {
        return Err("Mach-O load commands run past the file".into());
    }
    let mut loads = Vec::new();
    let mut offset = header;
    for _ in 0..command_count {
        let size = little(data, offset + 4).ok_or("truncated load command")? as usize;
        if size < 8 || offset + size > end {
            return Err("invalid load command size".into());
        }
        loads.push(&data[offset..offset + size]);
        offset += size;
    }
    Ok(MachO {
        data,
        is_64: header == HEADER_64,
        cpu: field(4)?,
        file_type: field(12)?,
        command_count,
        commands_size,
        loads,
    })
}

fn command(raw: &[u8]) -> u32 {
    little(raw, 0).unwrap_or(0)
}

fn dylib_name(raw: &[u8], kinds: &[u32]) -> Option<String> {
    if raw.len() < DYLIB_COMMAND || !kinds.contains(&command(raw)) {
        return None;
    }
    let name = raw.get(little(raw, 8)? as usize..)?;
    let end = name
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(name.len());
    Some(String::from_utf8_lossy(&name[..end]).into_owned())
}

impl MachO<'_> {
    pub fn loaded_dylibs(&self) -> Vec<String> {
        self.loads
            .iter()
            .filter_map(|raw| dylib_name(raw, &[LOAD_DYLIB, LOAD_WEAK_DYLIB]))
            .collect()
    }

    pub fn loads_dylib(&self, name: &str) -> bool {
        self.loads.iter().any(|raw| {
            dylib_name(raw, &[LOAD_DYLIB, LOAD_WEAK_DYLIB, ID_DYLIB]).as_deref() == Some(name)
        })
    }

    fn first_section_offset(&self) -> u32 {
        let mut first = u32::MAX;
        for raw in self.loads.iter().filter(|raw| command(raw) == SEGMENT_64) {
            let count = little(raw, 64).unwrap_or(0) as usize;
            for index in 0..count {
                if let Some(offset) = little(raw, 72 + index * 80 + 48)
                    && offset != 0
                    && offset < first
                {
                    first = offset;
                }
            }
        }
        first
    }

    fn code_signature(&self) -> Option<&[u8]> {
        let raw = self
            .loads
            .iter()
            .find(|raw| raw.len() >= 16 && command(raw) == CODE_SIGNATURE)?;
        let offset = little(raw, 8)? as usize;
        let size = little(raw, 12)? as usize;
        self.data.get(offset..offset.checked_add(size)?)
    }
}

pub fn loads_dylib(data: &[u8], name: &str) -> bool {
    parse(data).is_ok_and(|file| file.loads_dylib(name))
}

fn weak_dylib_command(name: &str) -> Vec<u8> {
    let size = (DYLIB_COMMAND + name.len() + 1 + 7) & !7;
    let mut command = vec![0u8; size];
    for (index, value) in [
        LOAD_WEAK_DYLIB,
        size as u32,
        DYLIB_COMMAND as u32,
        2,
        0x10000,
        0x10000,
    ]
    .into_iter()
    .enumerate()
    {
        command[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    command[DYLIB_COMMAND..DYLIB_COMMAND + name.len()].copy_from_slice(name.as_bytes());
    command
}

pub fn add_weak_dylib(data: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let file = parse(data).map_err(|error| format!("binary is not a thin Mach-O: {error}"))?;
    if !file.is_64 {
        return Err("binary is not a little-endian 64-bit Mach-O".into());
    }
    if file.loads_dylib(name) {
        return Ok(data.to_vec());
    }
    let command = weak_dylib_command(name);
    let end = HEADER_64 + file.commands_size as usize;
    let command_end = end + command.len();
    if command_end > file.first_section_offset() as usize || command_end > data.len() {
        return Err("binary has no room for another load command".into());
    }
    if data[end..command_end].iter().any(|byte| *byte != 0) {
        return Err("binary has data after its load commands".into());
    }
    let mut patched = data.to_vec();
    patched[end..command_end].copy_from_slice(&command);
    patched[16..20].copy_from_slice(&(file.command_count + 1).to_le_bytes());
    patched[20..24].copy_from_slice(&(file.commands_size + command.len() as u32).to_le_bytes());
    Ok(patched)
}

fn entitlements(signature: &[u8], offset: usize) -> Option<&[u8]> {
    let blob = signature.get(offset..)?;
    if big(blob, 0)? != ENTITLEMENTS_BLOB {
        return None;
    }
    let length = big(blob, 4)? as usize;
    if length < 8 {
        return None;
    }
    blob.get(8..length)
}

pub fn entitlement_dictionary(data: &[u8]) -> Option<plist::Dictionary> {
    let file = parse(data).ok()?;
    let signature = file.code_signature()?;
    if big(signature, 0) != Some(SUPER_BLOB) {
        return None;
    }
    let count = big(signature, 8).unwrap_or(0) as usize;
    for index in 0..count {
        let entry = 12 + index * 8;
        if big(signature, entry)? != ENTITLEMENTS_SLOT {
            continue;
        }
        let blob = big(signature, entry + 4)
            .and_then(|offset| entitlements(signature, offset as usize))?;
        return match plist::Value::from_reader(std::io::Cursor::new(blob)) {
            Ok(plist::Value::Dictionary(values)) => Some(values),
            _ => None,
        };
    }
    None
}

pub fn app_groups(data: &[u8]) -> Vec<String> {
    entitlement_dictionary(data)
        .and_then(|values| {
            values
                .get(APP_GROUPS)
                .and_then(plist::Value::as_array)
                .cloned()
        })
        .map(|groups| {
            groups
                .iter()
                .filter_map(plist::Value::as_string)
                .filter(|group| !group.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
