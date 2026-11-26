mod support;

use std::collections::BTreeMap;
use std::path::PathBuf;

use astral_tools::android_compat;
use support::{temporary, write_zip};

const GUEST_HOST: &str = "Lcom/discord/nativeapp/bridge/GuestHost;";
const MAIN_ACTIVITY: &str = "Lcom/discord/nativeapp/MainActivity;";
const FUNCTION0: &str = "Lkotlin/jvm/functions/Function0;";
const INTENT: &str = "Landroid/content/Intent;";

fn uleb(data: &mut Vec<u8>, mut value: u32) {
    while value >= 0x80 {
        data.push((value as u8) | 0x80);
        value >>= 7;
    }
    data.push(value as u8);
}

fn put32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn index_of<T: PartialEq>(items: &[T], item: &T) -> usize {
    items
        .iter()
        .position(|candidate| candidate == item)
        .expect("indexed")
}

fn test_dex() -> Vec<u8> {
    let prototypes: Vec<(Vec<&str>, &str)> = vec![
        (vec!["[B", "I"], "V"),
        (vec!["[B", FUNCTION0], "V"),
        (vec!["I", "I", INTENT], "V"),
        (vec![], "V"),
    ];
    let declared = [
        (GUEST_HOST, "applyOps", 0usize),
        (GUEST_HOST, "a", 1),
        (MAIN_ACTIVITY, "onActivityResult", 2),
        (MAIN_ACTIVITY, "onPause", 3),
        (MAIN_ACTIVITY, "onResume", 3),
    ];
    let mut strings: Vec<&str> = vec![
        GUEST_HOST,
        MAIN_ACTIVITY,
        FUNCTION0,
        INTENT,
        "[B",
        "I",
        "V",
        "applyOps",
        "a",
        "onActivityResult",
        "onPause",
        "onResume",
        "accessibilityActions",
        "text",
        "uri",
        "press",
        "change",
    ];
    strings.sort_unstable();
    let mut types = vec![GUEST_HOST, MAIN_ACTIVITY, FUNCTION0, INTENT, "[B", "I", "V"];
    types.sort_unstable();
    let mut methods: Vec<(usize, usize, usize)> = declared
        .iter()
        .map(|(class, name, proto)| (index_of(&types, class), index_of(&strings, name), *proto))
        .collect();
    methods.sort_unstable();
    let mut classes = vec![GUEST_HOST, MAIN_ACTIVITY];
    classes.sort_by_key(|class| index_of(&types, class));
    let string_offset = 112;
    let type_offset = string_offset + strings.len() * 4;
    let proto_offset = type_offset + types.len() * 4;
    let method_offset = proto_offset + prototypes.len() * 12;
    let class_offset = method_offset + methods.len() * 8;
    let data_offset = class_offset + classes.len() * 32;
    let mut data = Vec::new();
    let mut string_data = Vec::new();
    for value in &strings {
        string_data.push(data_offset + data.len());
        uleb(&mut data, value.len() as u32);
        data.extend_from_slice(value.as_bytes());
        data.push(0);
    }
    let mut parameter_offsets = vec![0usize; prototypes.len()];
    for (index, (parameters, _)) in prototypes.iter().enumerate() {
        data.resize(data.len().div_ceil(4) * 4, 0);
        if parameters.is_empty() {
            continue;
        }
        parameter_offsets[index] = data_offset + data.len();
        data.extend_from_slice(&(parameters.len() as u32).to_le_bytes());
        for parameter in parameters {
            data.extend_from_slice(&(index_of(&types, parameter) as u16).to_le_bytes());
        }
    }
    let mut class_data = BTreeMap::new();
    for class in &classes {
        class_data.insert(*class, data_offset + data.len());
        let class_index = index_of(&types, class);
        let owned: Vec<usize> = (0..methods.len())
            .filter(|index| methods[*index].0 == class_index)
            .collect();
        uleb(&mut data, 0);
        uleb(&mut data, 0);
        uleb(&mut data, owned.len() as u32);
        uleb(&mut data, 0);
        let mut previous = 0;
        for (position, method) in owned.iter().enumerate() {
            uleb(
                &mut data,
                if position == 0 {
                    *method
                } else {
                    method - previous
                } as u32,
            );
            uleb(&mut data, 1);
            uleb(&mut data, 0);
            previous = *method;
        }
    }
    let mut result = vec![0u8; data_offset + data.len()];
    result[..8].copy_from_slice(b"dex\n039\0");
    let length = result.len() as u32;
    for (offset, value) in [
        (32, length),
        (36, 112),
        (40, 0x1234_5678),
        (56, strings.len() as u32),
        (60, string_offset as u32),
        (64, types.len() as u32),
        (68, type_offset as u32),
        (72, prototypes.len() as u32),
        (76, proto_offset as u32),
        (88, methods.len() as u32),
        (92, method_offset as u32),
        (96, classes.len() as u32),
        (100, class_offset as u32),
    ] {
        put32(&mut result, offset, value);
    }
    for (index, offset) in string_data.iter().enumerate() {
        put32(&mut result, string_offset + index * 4, *offset as u32);
    }
    for (index, descriptor) in types.iter().enumerate() {
        put32(
            &mut result,
            type_offset + index * 4,
            index_of(&strings, descriptor) as u32,
        );
    }
    for (index, (_, returned)) in prototypes.iter().enumerate() {
        put32(
            &mut result,
            proto_offset + index * 12 + 4,
            index_of(&types, returned) as u32,
        );
        put32(
            &mut result,
            proto_offset + index * 12 + 8,
            parameter_offsets[index] as u32,
        );
    }
    for (index, (class, name, proto)) in methods.iter().enumerate() {
        put16(&mut result, method_offset + index * 8, *class as u16);
        put16(&mut result, method_offset + index * 8 + 2, *proto as u16);
        put32(&mut result, method_offset + index * 8 + 4, *name as u32);
    }
    for (index, class) in classes.iter().enumerate() {
        put32(
            &mut result,
            class_offset + index * 32,
            index_of(&types, class) as u32,
        );
        put32(
            &mut result,
            class_offset + index * 32 + 24,
            class_data[class] as u32,
        );
    }
    result[data_offset..].copy_from_slice(&data);
    result
}

fn apk(directory: &std::path::Path, file: &str, dex_name: &str, dex: Vec<u8>) -> PathBuf {
    let path = directory.join(file);
    write_zip(&path, &[(dex_name, dex)]);
    path
}

fn properties(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn scans_phoenix_classes_methods_properties_and_events() {
    let directory = temporary("dex");
    let path = apk(&directory.0, "Discord.apk", "classes.dex", test_dex());
    let result = android_compat::scan(
        &[path],
        &properties(&["accessibilityActions", "text", "uri"]),
    )
    .expect("scan");
    assert!(result.supported);
    assert_eq!(result.properties_found, 3);
    assert_eq!(result.events.get("press"), Some(&true));
    assert_eq!(result.events.get("change"), Some(&true));
    assert_eq!(
        result.methods.get("GuestHost.applyOps([B, int)"),
        Some(&true)
    );
    assert_eq!(result.methods.get("GuestHost event callback"), Some(&true));
    assert_eq!(
        result.methods.get("MainActivity.onActivityResult"),
        Some(&true)
    );
}

#[test]
fn rejects_missing_runtime_surface() {
    let directory = temporary("dex-missing");
    let mut dex = test_dex();
    let index = dex
        .windows(5)
        .position(|window| window == b"press")
        .expect("press");
    dex[index] = b'x';
    let path = apk(&directory.0, "Broken.apk", "classes.dex", dex);
    let result = android_compat::scan(
        &[path],
        &properties(&["accessibilityActions", "text", "uri"]),
    )
    .expect("scan");
    assert!(!result.supported);
    assert_eq!(result.events.get("press"), Some(&false));
    let path = apk(&directory.0, "Discord.apk", "classes.dex", test_dex());
    let result = android_compat::scan(
        &[path],
        &properties(&["accessibilityActions", "text", "missingProperty"]),
    )
    .expect("scan");
    assert!(!result.supported);
    assert_eq!(
        result.missing_properties,
        vec!["missingProperty".to_owned()]
    );
}

#[test]
fn combines_base_and_split_apks() {
    let directory = temporary("dex-splits");
    let base = apk(&directory.0, "Discord.apk", "classes.dex", test_dex());
    let split = apk(
        &directory.0,
        "config.arm64_v8a.apk",
        "classes2.dex",
        test_dex(),
    );
    let resources = apk(
        &directory.0,
        "config.xxhdpi.apk",
        "resources.arsc",
        Vec::new(),
    );
    let result = android_compat::scan(
        &[base, resources, split],
        &properties(&["accessibilityActions"]),
    )
    .expect("scan");
    assert!(result.supported);
    assert_eq!(result.apks.len(), 2);
    assert_eq!(result.dex_files.len(), 2);
}
