const XML: u16 = 0x0003;
const STRING_POOL: u16 = 0x0001;
const START_ELEMENT: u16 = 0x0102;
const END_ELEMENT: u16 = 0x0103;
const UTF8_FLAG: u32 = 0x100;
const NO_INDEX: u32 = u32::MAX;
const ATTRIBUTE_SIZE: usize = 20;
const ELEMENT_HEADER: usize = 16;
const ELEMENT_BODY: usize = 20;
const STRING_TYPE: u8 = 0x03;

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, String> {
    bytes
        .get(at..at + 2)
        .map(|slice| u16::from_le_bytes([slice[0], slice[1]]))
        .ok_or_else(|| format!("manifest ends before byte {at}"))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    bytes
        .get(at..at + 4)
        .map(|slice| u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
        .ok_or_else(|| format!("manifest ends before byte {at}"))
}

fn put_u16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

struct Pool {
    strings: Vec<String>,
}

fn utf8_length(bytes: &[u8], at: usize) -> (usize, usize) {
    let first = usize::from(bytes[at]);
    if first & 0x80 == 0 {
        (first, 1)
    } else {
        (((first & 0x7F) << 8) | usize::from(bytes[at + 1]), 2)
    }
}

fn utf16_length(bytes: &[u8], at: usize) -> Result<(usize, usize), String> {
    let first = usize::from(u16_at(bytes, at)?);
    if first & 0x8000 == 0 {
        Ok((first, 2))
    } else {
        Ok((
            ((first & 0x7FFF) << 16) | usize::from(u16_at(bytes, at + 2)?),
            4,
        ))
    }
}

fn read_string(data: &[u8], at: usize, utf8: bool) -> Result<String, String> {
    if utf8 {
        let (_, skip_chars) = utf8_length(data, at);
        let (length, skip_bytes) = utf8_length(data, at + skip_chars);
        let start = at + skip_chars + skip_bytes;
        let slice = data
            .get(start..start + length)
            .ok_or("string runs past the pool")?;
        return Ok(String::from_utf8_lossy(slice).into_owned());
    }
    let (length, skip) = utf16_length(data, at)?;
    let units = (0..length)
        .map(|index| u16_at(data, at + skip + index * 2))
        .collect::<Result<Vec<u16>, String>>()?;
    Ok(String::from_utf16_lossy(&units))
}

fn encode_string(text: &str, utf8: bool) -> Vec<u8> {
    let mut encoded = Vec::new();
    if utf8 {
        let push_length = |encoded: &mut Vec<u8>, length: usize| {
            if length > 0x7F {
                encoded.push(0x80 | ((length >> 8) as u8));
            }
            encoded.push((length & 0xFF) as u8);
        };
        push_length(&mut encoded, text.chars().count());
        push_length(&mut encoded, text.len());
        encoded.extend_from_slice(text.as_bytes());
        encoded.push(0);
    } else {
        let units: Vec<u16> = text.encode_utf16().collect();
        encoded.extend_from_slice(&(units.len() as u16).to_le_bytes());
        for unit in units {
            encoded.extend_from_slice(&unit.to_le_bytes());
        }
        encoded.extend_from_slice(&0u16.to_le_bytes());
    }
    encoded
}

fn read_pool(chunk: &[u8]) -> Result<Pool, String> {
    let header = usize::from(u16_at(chunk, 2)?);
    let count = u32_at(chunk, 8)? as usize;
    let utf8 = u32_at(chunk, 16)? & UTF8_FLAG != 0;
    let strings_start = u32_at(chunk, 20)? as usize;
    let strings = (0..count)
        .map(|index| {
            read_string(
                &chunk[strings_start..],
                u32_at(chunk, header + index * 4)? as usize,
                utf8,
            )
        })
        .collect::<Result<Vec<String>, String>>()?;
    Ok(Pool { strings })
}

fn append_string(chunk: &[u8], text: &str) -> Result<Vec<u8>, String> {
    let header = usize::from(u16_at(chunk, 2)?);
    let size = u32_at(chunk, 4)? as usize;
    let count = u32_at(chunk, 8)? as usize;
    let style_count = u32_at(chunk, 12)? as usize;
    let utf8 = u32_at(chunk, 16)? & UTF8_FLAG != 0;
    let strings_start = u32_at(chunk, 20)? as usize;
    let styles_start = u32_at(chunk, 24)? as usize;
    let offsets_end = header + count * 4;
    let style_offsets_end = offsets_end + style_count * 4;
    let strings_end = if styles_start == 0 {
        size
    } else {
        styles_start
    };
    let mut data = chunk[strings_start..strings_end].to_vec();
    let new_offset = data.len() as u32;
    data.extend(encode_string(text, utf8));
    while !data.len().is_multiple_of(4) {
        data.push(0);
    }
    let mut rebuilt = chunk[..offsets_end].to_vec();
    rebuilt.extend_from_slice(&new_offset.to_le_bytes());
    rebuilt.extend_from_slice(&chunk[offsets_end..style_offsets_end]);
    let new_strings_start = rebuilt.len();
    rebuilt.extend(data);
    let new_styles_start = if styles_start == 0 { 0 } else { rebuilt.len() };
    if styles_start != 0 {
        rebuilt.extend_from_slice(&chunk[styles_start..size]);
    }
    let rebuilt_size = rebuilt.len() as u32;
    put_u32(&mut rebuilt, 4, rebuilt_size);
    put_u32(&mut rebuilt, 8, (count + 1) as u32);
    put_u32(&mut rebuilt, 20, new_strings_start as u32);
    put_u32(&mut rebuilt, 24, new_styles_start as u32);
    Ok(rebuilt)
}

struct Element {
    start: usize,
    end: usize,
    kind: u16,
    name: String,
    attributes: Vec<(String, Option<String>, usize)>,
}

fn string_at(pool: &Pool, index: u32) -> Option<String> {
    (index != NO_INDEX)
        .then(|| pool.strings.get(index as usize).cloned())
        .flatten()
}

fn elements(manifest: &[u8], from: usize, pool: &Pool) -> Result<Vec<Element>, String> {
    let mut found = Vec::new();
    let mut at = from;
    while at + 8 <= manifest.len() {
        let kind = u16_at(manifest, at)?;
        let size = u32_at(manifest, at + 4)? as usize;
        if size < 8 || at + size > manifest.len() {
            return Err(format!("manifest chunk at {at} has a bad size"));
        }
        if kind == START_ELEMENT || kind == END_ELEMENT {
            let name =
                string_at(pool, u32_at(manifest, at + ELEMENT_HEADER + 4)?).unwrap_or_default();
            let mut attributes = Vec::new();
            if kind == START_ELEMENT {
                let attribute_start = usize::from(u16_at(manifest, at + ELEMENT_HEADER + 8)?);
                let count = usize::from(u16_at(manifest, at + ELEMENT_HEADER + 12)?);
                for index in 0..count {
                    let offset = at + ELEMENT_HEADER + attribute_start + index * ATTRIBUTE_SIZE;
                    let attribute =
                        string_at(pool, u32_at(manifest, offset + 4)?).unwrap_or_default();
                    let value = string_at(pool, u32_at(manifest, offset + 8)?);
                    attributes.push((attribute, value, offset));
                }
            }
            found.push(Element {
                start: at,
                end: at + size,
                kind,
                name,
                attributes,
            });
        }
        at += size;
    }
    Ok(found)
}

fn value<'a>(element: &'a Element, attribute: &str) -> Option<&'a str> {
    element
        .attributes
        .iter()
        .find(|(name, _, _)| name == attribute)
        .and_then(|(_, value, _)| value.as_deref())
}

fn subtree_end(elements: &[Element], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, element) in elements.iter().enumerate().skip(start) {
        if element.kind == START_ELEMENT {
            depth += 1;
        } else {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn scheme_only_data(manifest: &[u8], data: &Element, scheme_index: u32) -> Option<Vec<u8>> {
    let (_, _, offset) = data
        .attributes
        .iter()
        .find(|(name, _, _)| name == "scheme")?;
    let mut chunk = manifest[data.start..data.start + ELEMENT_HEADER + ELEMENT_BODY].to_vec();
    let mut attribute = manifest[*offset..*offset + ATTRIBUTE_SIZE].to_vec();
    put_u32(&mut attribute, 8, scheme_index);
    attribute[15] = STRING_TYPE;
    put_u32(&mut attribute, 16, scheme_index);
    chunk.extend(attribute);
    let size = chunk.len() as u32;
    put_u32(&mut chunk, 4, size);
    put_u16(&mut chunk, ELEMENT_HEADER + 8, ELEMENT_BODY as u16);
    put_u16(&mut chunk, ELEMENT_HEADER + 12, 1);
    put_u16(&mut chunk, ELEMENT_HEADER + 14, 0);
    put_u16(&mut chunk, ELEMENT_HEADER + 16, 0);
    put_u16(&mut chunk, ELEMENT_HEADER + 18, 0);
    Some(chunk)
}

pub fn add_scheme_filter(
    manifest: &[u8],
    activity: &str,
    scheme: &str,
) -> Result<Option<Vec<u8>>, String> {
    if u16_at(manifest, 0)? != XML || u16_at(manifest, 8)? != STRING_POOL {
        return Err("not a binary Android manifest".into());
    }
    let pool_size = u32_at(manifest, 12)? as usize;
    let pool_chunk = manifest
        .get(8..8 + pool_size)
        .ok_or("string pool runs past the manifest")?;
    let pool = read_pool(pool_chunk)?;
    let listed = elements(manifest, 8 + pool_size, &pool)?;
    if listed
        .iter()
        .any(|element| element.name == "data" && value(element, "scheme") == Some(scheme))
    {
        return Ok(None);
    }
    let activity_start = listed
        .iter()
        .position(|element| {
            element.kind == START_ELEMENT
                && element.name == "activity"
                && value(element, "name") == Some(activity)
        })
        .ok_or_else(|| format!("no {activity} in the manifest"))?;
    let activity_end = subtree_end(&listed, activity_start).ok_or("unclosed activity")?;
    let browsable = |range: &[Element]| {
        range
            .iter()
            .any(|element| value(element, "name") == Some("android.intent.category.BROWSABLE"))
            && range.iter().any(|element| {
                element.name == "data"
                    && value(element, "scheme").is_some_and(|scheme| !scheme.starts_with("http"))
            })
    };
    let (filter_start, filter_end) = (activity_start..activity_end)
        .filter(|index| {
            listed[*index].kind == START_ELEMENT && listed[*index].name == "intent-filter"
        })
        .filter_map(|index| subtree_end(&listed, index).map(|end| (index, end)))
        .find(|(start, end)| browsable(&listed[*start..=*end]))
        .ok_or("no browsable intent filter to copy")?;
    let rebuilt_pool = append_string(pool_chunk, scheme)?;
    let scheme_index = pool.strings.len() as u32;
    let template = &listed[filter_start..=filter_end];
    let data = template
        .iter()
        .find(|element| element.name == "data" && element.kind == START_ELEMENT)
        .ok_or("filter has no data")?;
    let data_end = &listed[filter_start
        + template
            .iter()
            .position(|element| std::ptr::eq(element, data))
            .ok_or("lost data")?
        + 1];
    let mut filter = Vec::new();
    filter.extend_from_slice(&manifest[template[0].start..template[0].end]);
    for element in &template[1..template.len() - 1] {
        if element.name != "data" {
            filter.extend_from_slice(&manifest[element.start..element.end]);
        }
    }
    filter.extend(scheme_only_data(manifest, data, scheme_index).ok_or("data has no scheme")?);
    filter.extend_from_slice(&manifest[data_end.start..data_end.end]);
    let closing = &template[template.len() - 1];
    filter.extend_from_slice(&manifest[closing.start..closing.end]);
    let insert_at = template[0].start;
    let mut patched = manifest[..8].to_vec();
    patched.extend(rebuilt_pool);
    patched.extend_from_slice(&manifest[8 + pool_size..insert_at]);
    patched.extend(filter);
    patched.extend_from_slice(&manifest[insert_at..]);
    let total = patched.len() as u32;
    put_u32(&mut patched, 4, total);
    Ok(Some(patched))
}
