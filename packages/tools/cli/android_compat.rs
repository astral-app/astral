use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

use serde::Serialize;

use crate::android;
use crate::compat;
use crate::flags::Flags;
use crate::repo;

const DEX_ENDIAN_CONSTANT: u32 = 0x1234_5678;
const DEX_HEADER_SIZE: usize = 112;

const MAXIMUM_DEX_SIZE: u64 = 128 << 20;
const TABLE_LIMIT: u32 = 1 << 22;
const GUEST_HOST: &str = "Lcom/discord/nativeapp/bridge/GuestHost;";
const MAIN_ACTIVITY: &str = "Lcom/discord/nativeapp/MainActivity;";
const APPLY_OPS: &str = "GuestHost.applyOps([B, int)";
const EVENT_CALLBACK: &str = "GuestHost event callback";
const USAGE: &str = "usage: astral android-compat (--channel <stable|beta|alpha> | --apk <Discord.apk> [--apk <split.apk> ...]) [--results android-compat.json] [--report android-compat.md]";

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AndroidCompat {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub channel: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub version_code: i64,
    pub apks: Vec<String>,
    pub phoenix: bool,
    pub supported: bool,
    pub dex_files: Vec<String>,
    pub classes: BTreeMap<String, bool>,
    pub methods: BTreeMap<String, bool>,
    pub properties_found: usize,
    pub properties_expected: usize,
    pub missing_properties: Vec<String>,
    pub events: BTreeMap<String, bool>,
    pub optional_network_hooks: BTreeMap<String, bool>,
    pub missing_hook_classes: Vec<String>,
}

fn is_zero(value: &i64) -> bool {
    *value == 0
}

#[derive(Clone)]
pub struct DexMethod {
    pub class: String,
    pub name: String,
    pub signature: String,
}

#[derive(Default)]
pub struct DexIndex {
    pub strings: BTreeSet<String>,
    pub classes: BTreeSet<String>,
    pub methods: Vec<DexMethod>,
}

struct Dex<'a> {
    data: &'a [u8],
}

impl Dex<'_> {
    fn u32(&self, offset: usize) -> Result<u32, String> {
        self.data
            .get(offset..offset + 4)
            .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            .ok_or_else(|| "DEX read is out of bounds".into())
    }

    fn u16(&self, offset: usize) -> Result<u16, String> {
        self.data
            .get(offset..offset + 2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .ok_or_else(|| "DEX read is out of bounds".into())
    }

    fn uleb(&self, cursor: &mut usize) -> Result<u32, String> {
        let mut value = 0u32;
        for shift in (0..35).step_by(7) {
            let current = *self
                .data
                .get(*cursor)
                .ok_or("DEX variable-length integer is truncated")?;
            *cursor += 1;
            value |= u32::from(current & 0x7f).wrapping_shl(shift);
            if current & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err("DEX variable-length integer is invalid".into())
    }

    fn range(&self, offset: u32, count: u32, stride: u32) -> Result<usize, String> {
        if u64::from(offset) + u64::from(count) * u64::from(stride) > self.data.len() as u64 {
            return Err("DEX index section is out of bounds".into());
        }
        Ok(offset as usize)
    }

    fn string_at(&self, offset: u32) -> Result<String, String> {
        if offset as usize >= self.data.len() {
            return Err("DEX string offset is out of bounds".into());
        }
        let mut cursor = offset as usize;
        self.uleb(&mut cursor)?;
        let length = self.data[cursor..]
            .iter()
            .position(|byte| *byte == 0)
            .ok_or("DEX string is not terminated")?;
        Ok(String::from_utf8_lossy(&self.data[cursor..cursor + length]).into_owned())
    }
}

struct Header {
    strings: (u32, u32),
    types: (u32, u32),
    protos: (u32, u32),
    fields: (u32, u32),
    methods: (u32, u32),
    classes: (u32, u32),
}

fn header(dex: &Dex) -> Result<Header, String> {
    let data = dex.data;
    if data.len() < DEX_HEADER_SIZE || &data[..4] != b"dex\n" || data[7] != 0 {
        return Err("APK contains an invalid DEX file".into());
    }
    if dex.u32(40)? != DEX_ENDIAN_CONSTANT {
        return Err("APK contains a DEX file with an unsupported byte order".into());
    }
    if dex.u32(32)? as usize != data.len() || dex.u32(36)? != DEX_HEADER_SIZE as u32 {
        return Err("APK contains a DEX file with an invalid header".into());
    }
    let pair =
        |offset| -> Result<(u32, u32), String> { Ok((dex.u32(offset)?, dex.u32(offset + 4)?)) };
    let header = Header {
        strings: pair(56)?,
        types: pair(64)?,
        protos: pair(72)?,
        fields: pair(80)?,
        methods: pair(88)?,
        classes: pair(96)?,
    };
    let counts = [
        header.strings.0,
        header.types.0,
        header.protos.0,
        header.fields.0,
        header.methods.0,
        header.classes.0,
    ];
    if counts.iter().any(|count| *count > TABLE_LIMIT) {
        return Err("DEX index table exceeds the scanner limit".into());
    }
    Ok(header)
}

fn prototypes(dex: &Dex, header: &Header, types: &[String]) -> Result<Vec<String>, String> {
    let base = dex.range(header.protos.1, header.protos.0, 12)?;
    let mut signatures = Vec::with_capacity(header.protos.0 as usize);
    for index in 0..header.protos.0 as usize {
        let entry = base + index * 12;
        let return_type = dex.u32(entry + 4)? as usize;
        let parameters = dex.u32(entry + 8)? as usize;
        let returned = types
            .get(return_type)
            .ok_or("DEX method prototype has an invalid return type")?;
        let mut signature = String::from("(");
        if parameters != 0 {
            let count =
                dex.u32(parameters)
                    .map_err(|_| "DEX parameter list is out of bounds")? as usize;
            if count > 1 << 16 || parameters + 4 + count * 2 > dex.data.len() {
                return Err("DEX parameter list is invalid".into());
            }
            for parameter in 0..count {
                let kind = dex.u16(parameters + 4 + parameter * 2)? as usize;
                signature.push_str(
                    types
                        .get(kind)
                        .ok_or("DEX method parameter has an invalid type")?,
                );
            }
        }
        signature.push(')');
        signature.push_str(returned);
        signatures.push(signature);
    }
    Ok(signatures)
}

fn declared_methods(
    dex: &Dex,
    header: &Header,
    table: &[DexMethod],
    cursor: &mut usize,
) -> Result<Vec<DexMethod>, String> {
    let static_fields = dex.uleb(cursor)?;
    let instance_fields = dex.uleb(cursor)?;
    let direct = dex.uleb(cursor)?;
    let virtual_methods = dex.uleb(cursor)?;
    if u64::from(static_fields) + u64::from(instance_fields) > u64::from(header.fields.0)
        || u64::from(direct) + u64::from(virtual_methods) > u64::from(header.methods.0)
    {
        return Err("DEX class data contains an invalid member count".into());
    }
    for _ in 0..static_fields + instance_fields {
        dex.uleb(cursor)?;
        dex.uleb(cursor)?;
    }
    let mut methods = Vec::new();
    for count in [direct, virtual_methods] {
        let mut index = 0u32;
        for _ in 0..count {
            index = index.wrapping_add(dex.uleb(cursor)?);
            dex.uleb(cursor)?;
            dex.uleb(cursor)?;
            methods.push(
                table
                    .get(index as usize)
                    .cloned()
                    .ok_or("DEX class method has an invalid index")?,
            );
        }
    }
    Ok(methods)
}

pub fn parse_dex(data: &[u8]) -> Result<DexIndex, String> {
    let dex = Dex { data };
    let header = header(&dex)?;
    let string_base = dex.range(header.strings.1, header.strings.0, 4)?;
    let type_base = dex.range(header.types.1, header.types.0, 4)?;
    dex.range(header.fields.1, header.fields.0, 8)?;
    let method_base = dex.range(header.methods.1, header.methods.0, 8)?;
    let class_base = dex.range(header.classes.1, header.classes.0, 32)?;
    let mut result = DexIndex::default();
    let mut strings = Vec::with_capacity(header.strings.0 as usize);
    for index in 0..header.strings.0 as usize {
        let value = dex.string_at(dex.u32(string_base + index * 4)?)?;
        result.strings.insert(value.clone());
        strings.push(value);
    }
    let types = (0..header.types.0 as usize)
        .map(|index| {
            let string = dex.u32(type_base + index * 4)? as usize;
            strings
                .get(string)
                .cloned()
                .ok_or_else(|| "DEX type has an invalid descriptor index".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let signatures = prototypes(&dex, &header, &types)?;
    let table = (0..header.methods.0 as usize)
        .map(|index| {
            let entry = method_base + index * 8;
            let (class, proto, name) = (
                dex.u16(entry)? as usize,
                dex.u16(entry + 2)? as usize,
                dex.u32(entry + 4)? as usize,
            );
            match (types.get(class), signatures.get(proto), strings.get(name)) {
                (Some(class), Some(signature), Some(name)) => Ok(DexMethod {
                    class: class.clone(),
                    name: name.clone(),
                    signature: signature.clone(),
                }),
                _ => Err("DEX method has an invalid index".to_owned()),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    for index in 0..header.classes.0 as usize {
        let entry = class_base + index * 32;
        let class = types
            .get(dex.u32(entry)? as usize)
            .ok_or("DEX class has an invalid type index")?;
        result.classes.insert(class.clone());
        let data_offset = dex.u32(entry + 24)? as usize;
        if data_offset != 0 {
            let mut cursor = data_offset;
            result
                .methods
                .extend(declared_methods(&dex, &header, &table, &mut cursor)?);
        }
    }
    Ok(result)
}

fn base_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

fn is_dex_name(name: &str) -> bool {
    name == "classes.dex"
        || name
            .strip_prefix("classes")
            .and_then(|rest| rest.strip_suffix(".dex"))
            .and_then(|middle| middle.parse::<i64>().ok())
            .is_some_and(|number| number >= 2)
}

fn dex_entries(archive: &mut zip::ZipArchive<File>) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|error| error.to_string())?;
        let name = entry
            .name()
            .map_err(|error| error.to_string())?
            .into_owned();
        if !is_dex_name(base_name(&name)) {
            continue;
        }
        if entry.size() > MAXIMUM_DEX_SIZE {
            return Err(format!(
                "{} exceeds the DEX scanner size limit",
                base_name(&name)
            ));
        }
        names.push(name);
    }
    names.sort();
    Ok(names)
}

fn phoenix_classes() -> Result<Vec<String>, String> {
    let source = repo::read(&repo::path(&[
        "packages",
        "android",
        "module",
        "src",
        "main",
        "kotlin",
        "app",
        "astral",
        "android",
        "PhoenixClasses.kt",
    ]))?;
    let pattern = regex::Regex::new(
        r#"(?:const val|@Volatile var) [A-Z_]+ = "([a-z][A-Za-z0-9_]*(?:\.[A-Za-z0-9_$]+)+)""#,
    )
    .map_err(|error| error.to_string())?;
    let classes: Vec<String> = pattern
        .captures_iter(&source)
        .map(|found| found[1].to_owned())
        .collect();
    if classes.is_empty() {
        return Err("could not read PhoenixClasses.kt".into());
    }
    Ok(classes)
}

pub fn dex_descriptor(class: &str) -> String {
    format!("L{};", class.replace('.', "/"))
}

pub fn properties() -> Result<Vec<String>, String> {
    let source = repo::read(&repo::path(&[
        "packages",
        "core",
        "wire",
        "PhoenixProperties.h",
    ]))?;
    let pattern = regex::Regex::new(r"X\(([A-Za-z_][A-Za-z0-9_]*),\s*[0-9]+\)")
        .map_err(|error| error.to_string())?;
    let properties: Vec<String> = pattern
        .captures_iter(&source)
        .map(|found| found[1].to_owned())
        .collect();
    if properties.is_empty() {
        return Err("could not read astral's Phoenix property list".into());
    }
    Ok(properties)
}

fn read_dex(archive: &mut zip::ZipArchive<File>, name: &str) -> Result<Vec<u8>, String> {
    let entry = archive
        .by_name(name)
        .map_err(|_| "DEX entry disappeared while scanning the APK".to_owned())?;
    let mut data = Vec::new();
    entry
        .take(MAXIMUM_DEX_SIZE + 1)
        .read_to_end(&mut data)
        .map_err(|error| error.to_string())?;
    if data.len() as u64 > MAXIMUM_DEX_SIZE {
        return Err(format!(
            "{} exceeds the DEX scanner size limit",
            base_name(name)
        ));
    }
    Ok(data)
}

fn combine(apks: &[PathBuf], result: &mut AndroidCompat) -> Result<DexIndex, String> {
    let mut combined = DexIndex::default();
    for apk in apks {
        let file = File::open(apk)
            .map_err(|error| format!("Failed to open {}: {error}", apk.display()))?;
        let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
        let names = dex_entries(&mut archive)?;
        if names.is_empty() {
            continue;
        }
        let apk_name = apk
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        result.apks.push(apk_name.clone());
        for name in names {
            let parsed = parse_dex(&read_dex(&mut archive, &name)?)
                .map_err(|error| format!("{}: {error}", base_name(&name)))?;
            result
                .dex_files
                .push(format!("{apk_name}:{}", base_name(&name)));
            combined.strings.extend(parsed.strings);
            combined.classes.extend(parsed.classes);
            combined.methods.extend(parsed.methods);
        }
    }
    if result.dex_files.is_empty() {
        return Err("the supplied APKs contain no DEX files".into());
    }
    Ok(combined)
}

fn record_methods(combined: &DexIndex, result: &mut AndroidCompat) {
    for method in &combined.methods {
        if method.class == GUEST_HOST {
            if method.name == "applyOps" && method.signature.starts_with("([BI)") {
                result.methods.insert(APPLY_OPS.into(), true);
            }
            if method.signature == "([BLkotlin/jvm/functions/Function0;)V" {
                result.methods.insert(EVENT_CALLBACK.into(), true);
            }
        }
        if method.class != MAIN_ACTIVITY {
            continue;
        }
        let expected = match method.name.as_str() {
            "onResume" | "onPause" => "()V",
            "onActivityResult" => "(IILandroid/content/Intent;)V",
            _ => continue,
        };
        if method.signature == expected {
            result
                .methods
                .insert(format!("MainActivity.{}", method.name), true);
        }
    }
}

pub fn scan(apks: &[PathBuf], properties: &[String]) -> Result<AndroidCompat, String> {
    let mut result = AndroidCompat::default();
    let combined = combine(apks, &mut result)?;
    let has_class = |class: &str| combined.classes.contains(class);
    let has_string = |value: &str| combined.strings.contains(value);
    result.phoenix = has_class(GUEST_HOST) && has_class(MAIN_ACTIVITY);
    result
        .classes
        .insert("GuestHost".into(), has_class(GUEST_HOST));
    result
        .classes
        .insert("MainActivity".into(), has_class(MAIN_ACTIVITY));
    record_methods(&combined, &mut result);
    for property in properties {
        if has_string(property) {
            result.properties_found += 1;
        } else {
            result.missing_properties.push(property.clone());
        }
    }
    result.missing_properties.sort();
    result.properties_expected = properties.len();
    result.events.insert("press".into(), has_string("press"));
    result.events.insert("change".into(), has_string("change"));
    result.optional_network_hooks.insert(
        "Cronet".into(),
        has_class("Lorg/chromium/net/CronetEngine$Builder;"),
    );
    result.missing_hook_classes = phoenix_classes()?
        .into_iter()
        .filter(|class| !has_class(&dex_descriptor(class)))
        .collect();
    result.optional_network_hooks.insert(
        "Phoenix shaded OkHttp".into(),
        result.missing_hook_classes.is_empty(),
    );
    let method = |name: &str| result.methods.get(name).copied().unwrap_or(false);
    result.supported = result.phoenix
        && method(APPLY_OPS)
        && method(EVENT_CALLBACK)
        && result.properties_expected == result.properties_found
        && has_string("press")
        && has_string("change");
    Ok(result)
}

fn lookup(map: &BTreeMap<String, bool>, key: &str) -> bool {
    map.get(key).copied().unwrap_or(false)
}

pub fn report(result: &AndroidCompat) -> String {
    let status = if result.supported {
        "supported"
    } else {
        "**not supported**"
    };
    let mut report = format!("## astral on Android Phoenix\n\nThis APK is {status}.\n\n");
    if !result.channel.is_empty() {
        report.push_str(&format!(
            "Discord {}, version code {}, signed by Discord.\n\n",
            result.channel, result.version_code
        ));
    }
    report.push_str(&format!(
        "Scanned APKs: `{}`.\n\n",
        result.apks.join("`, `")
    ));
    report.push_str(&format!(
        "Scanned DEX files: `{}`.\n\n",
        result.dex_files.join("`, `")
    ));
    report.push_str("| Check | Result |\n| --- | --- |\n");
    report.push_str(&format!(
        "| Phoenix host classes | GuestHost {}, MainActivity {} |\n",
        lookup(&result.classes, "GuestHost"),
        lookup(&result.classes, "MainActivity")
    ));
    report.push_str(&format!(
        "| `GuestHost.applyOps(byte[], int)` | {} |\n",
        lookup(&result.methods, APPLY_OPS)
    ));
    report.push_str(&format!(
        "| GuestHost event callback | {} |\n",
        lookup(&result.methods, EVENT_CALLBACK)
    ));
    report.push_str(&format!(
        "| MainActivity lifecycle/document hooks | resume {}, pause {}, activity result {} |\n",
        lookup(&result.methods, "MainActivity.onResume"),
        lookup(&result.methods, "MainActivity.onPause"),
        lookup(&result.methods, "MainActivity.onActivityResult")
    ));
    report.push_str(&format!(
        "| Phoenix property names | {} / {} |\n",
        result.properties_found, result.properties_expected
    ));
    report.push_str(&format!(
        "| Event identifiers | press {}, change {} |\n",
        lookup(&result.events, "press"),
        lookup(&result.events, "change")
    ));
    report.push_str(&format!(
        "| Optional network hooks | Cronet {}, Phoenix shaded OkHttp {} |\n",
        lookup(&result.optional_network_hooks, "Cronet"),
        lookup(&result.optional_network_hooks, "Phoenix shaded OkHttp")
    ));
    if !result.missing_hook_classes.is_empty() {
        report.push_str(&format!(
            "\nMissing classes from PhoenixClasses.kt: `{}`.\n",
            result.missing_hook_classes.join("`, `")
        ));
    }
    if !result.missing_properties.is_empty() {
        report.push_str(&format!(
            "\nMissing Phoenix properties: `{}`.\n",
            result.missing_properties.join("`, `")
        ));
    }
    report
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("android-compat")
        .list("apk")
        .string("channel", "")
        .string("results", "android-compat.json")
        .string("report", "android-compat.md")
        .parse(arguments)?;
    let channel = flags.get("channel");
    let listed = flags.all("apk");
    if listed.iter().any(|apk| apk.trim().is_empty()) {
        return Err("APK path cannot be empty".into());
    }
    if listed.is_empty() == channel.is_empty() {
        return Err(USAGE.into());
    }
    let work = repo::scratch()?;
    let (version_code, apks) = if channel.is_empty() {
        (0, listed.iter().map(PathBuf::from).collect())
    } else {
        android::download_apks(&channel, &android::SCAN_SPLITS, &work.0)?
    };
    let mut result = scan(&apks, &properties()?)?;
    result.channel = channel;
    result.version_code = version_code;
    let report = report(&result);
    compat::write_results(
        &result,
        &flags.get("results"),
        &flags.get("report"),
        &report,
    )?;
    if !result.supported {
        return Err("this Android Discord build is not supported".into());
    }
    Ok(())
}
