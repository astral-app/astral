const ACCESS_SUFFIX: &[u8] = b"-phoenix-app-access";

pub fn experiment_offsets(bytes: &[u8]) -> impl Iterator<Item = usize> + '_ {
    bytes.windows(7 + ACCESS_SUFFIX.len()).enumerate().filter_map(|(offset, name)| {
        (name[7..] == *ACCESS_SUFFIX
            && name[4] == b'-'
            && name[..4].iter().all(u8::is_ascii_digit)
            && name[5..7].iter().all(u8::is_ascii_digit))
        .then_some(offset)
    })
}
pub const RETURN_TRUE: [u8; 8] = [0x20, 0x00, 0x80, 0x52, 0xc0, 0x03, 0x5f, 0xd6];

pub fn decode_address_reference(words: &[[u8; 4]], index: usize, base: u64) -> Option<u64> {
    let word = u32::from_le_bytes(*words.get(index)?);
    if word & 0x9f00_0000 != 0x9000_0000 {
        return None;
    }
    let register = word & 31;
    let immediate = ((((word >> 5) & 0x7ffff) << 2) | ((word >> 29) & 3)) as i32;
    let immediate = i64::from(immediate << 11 >> 11);
    let page = ((base + index as u64 * 4) & !0xfff).wrapping_add((immediate << 12) as u64);
    words[index + 1..].iter().take(4).find_map(|add| {
        let add = u32::from_le_bytes(*add);
        (add & 0xffc0_0000 == 0x9100_0000 && (add >> 5) & 31 == register).then(|| page + u64::from((add >> 10) & 0xfff))
    })
}

fn decode_guard(words: &[[u8; 4]]) -> Option<&[[u8; 4]]> {
    let prefix = words.get(..words.len().min(20))?;
    let end = prefix.windows(4).position(|tail| {
        let load = u32::from_le_bytes(tail[0]);
        let branch = u32::from_le_bytes(tail[1]);
        (load & 0xffc0_0000 == 0x3940_0000 || load & 0xffe0_0c00 == 0x3860_0800)
            && branch & 0xfff8_001f == 0x3600_0000 | (load & 31)
            && (branch >> 5) & 0x3fff == 3
            && u32::from_le_bytes(tail[2]) == 0x5280_0040
            && u32::from_le_bytes(tail[3]) == 0xd65f_03c0
    })? + 4;
    let signature = prefix.get(2..end)?;
    let checks = signature
        .windows(2)
        .enumerate()
        .filter(|(index, check)| {
            let compare = u32::from_le_bytes(check[0]);
            let branch = u32::from_le_bytes(check[1]);
            (compare & 0xffff_fc1f == 0xf100_081f || compare & 0xffff_fc1f == 0xb100_041f)
                && branch & 0xff00_001f == 0x5400_0000
                && 2 + index + 1 + ((branch >> 5) & 0x7ffff) as usize == end - 2
        })
        .count();
    (checks == 2).then_some(signature)
}

pub fn evaluators(text: &[u8], base: u64, starts: &[u64], descriptors: &[u64]) -> Result<[u64; 2], String> {
    let words = text.as_chunks::<4>().0;
    let mut candidates = Vec::new();
    for (index, start) in starts.iter().enumerate() {
        let Some(offset) = start.checked_sub(base).and_then(|offset| usize::try_from(offset / 4).ok()) else {
            continue;
        };
        let end = starts.get(index + 1).copied().unwrap_or(base + text.len() as u64);
        let Some(length) = end.checked_sub(*start).and_then(|length| usize::try_from(length / 4).ok()) else {
            continue;
        };
        let Some(body) = words.get(offset..offset.saturating_add(length)) else {
            continue;
        };
        let Some(signature) = decode_guard(body) else { continue };
        let anchored = (0..body.len())
            .any(|instruction| decode_address_reference(body, instruction, *start).is_some_and(|target| descriptors.contains(&target)));
        candidates.push((*start, signature, anchored));
    }
    let anchored: Vec<_> = candidates.iter().filter(|(_, _, anchored)| *anchored).collect();
    let Some((_, signature, _)) = anchored.first() else {
        return Err("Phoenix access experiment has no validated evaluator".into());
    };
    if anchored.iter().any(|(_, other, _)| other != signature) {
        return Err("Phoenix access experiment has conflicting evaluator guards".into());
    }
    let found: Vec<_> = candidates
        .iter()
        .filter(|(_, other, _)| other == signature)
        .map(|(start, _, _)| *start)
        .collect();
    found
        .try_into()
        .map_err(|found: Vec<u64>| format!("Phoenix access gate has {} validated evaluators; expected exactly 2", found.len()))
}
