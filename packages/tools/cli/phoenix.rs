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

pub fn opens_access_gate(data: &[u8]) -> bool {
    little32(data, 0) == Some(MH_MAGIC_64)
        && evaluators(data)
            .map(|evaluators| !evaluators.is_empty())
            .unwrap_or(false)
}
