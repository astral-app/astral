use std::path::Path;

use crate::flags::{Flags, environment};
use crate::lint;
use crate::repo::{self, strings};
use crate::signing;

fn lua_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "-DLUA_USE_MACOSX"
    } else {
        "-DLUA_USE_LINUX"
    }
}

pub fn test_wire(work: &Path) -> Result<(), String> {
    let ios = repo::path(&["packages", "ios"]);
    let binary = repo::text(&work.join("phoenix-wire-test"));
    let mut arguments = strings(&[
        "-Wall",
        "-Wextra",
        "-Werror",
        "-g",
        "-fsanitize=address,undefined",
        "tests/phoenix-wire-test.c",
        "tests/support/wire-parser.c",
    ]);
    arguments.push(format!(
        "-I{}",
        repo::text(&repo::path(&["packages", "core", "wire"]))
    ));
    arguments.push(crate::fuzz::core_library(false)?);
    arguments.extend(strings(&["-lm", "-o", &binary]));
    repo::run(&ios, "clang", &arguments)?;
    repo::run(&ios, &binary, &[])
}

pub fn test_core(work: &Path) -> Result<(), String> {
    let root = repo::path(&[]);
    let object = repo::text(&work.join("lua-core.o"));
    let onelua = repo::text(&repo::path(&["vendor", "lua", "onelua.c"]));
    repo::run(
        &root,
        "cc",
        &strings(&[
            "-c",
            "-O1",
            "-g",
            "-fsanitize=address,undefined",
            "-DMAKE_LIB",
            lua_platform(),
            "-w",
            &onelua,
            "-o",
            &object,
        ]),
    )?;
    let binary = repo::text(&work.join("lua-core-test"));
    let include = |parts: &[&str]| format!("-I{}", repo::text(&repo::path(parts)));
    let mut arguments = strings(&[
        "-std=c++20",
        "-Wall",
        "-Wextra",
        "-Werror",
        "-g",
        "-fsanitize=address,undefined",
    ]);
    arguments.extend([
        include(&["packages", "core", "include"]),
        include(&["vendor", "lua"]),
        format!("-I{}", repo::text(work)),
        include(&["packages", "core", "wire"]),
        "-isystem".into(),
        repo::text(&repo::path(&["vendor", "json", "single_include"])),
        "-isystem".into(),
        repo::text(&repo::path(&["vendor", "tomlplusplus", "include"])),
        repo::text(&repo::path(&[
            "packages",
            "core",
            "tests",
            "lua-core-test.cpp",
        ])),
    ]);
    if !cfg!(target_os = "macos") {
        arguments.push("-lcrypto".into());
    }
    arguments.extend([
        object,
        crate::fuzz::core_library(false)?,
        "-lsqlite3".into(),
        "-o".into(),
        binary.clone(),
    ]);
    repo::run(&root, "clang++", &arguments)?;
    repo::run(&root, &binary, &[repo::text(work), repo::text(&root)])
}

pub fn build_lua(work: &Path) -> Result<String, String> {
    let binary = repo::text(&work.join("lua"));
    let onelua = repo::text(&repo::path(&["vendor", "lua", "onelua.c"]));
    repo::run(
        &repo::path(&[]),
        "cc",
        &strings(&["-O1", lua_platform(), "-w", "-o", &binary, &onelua, "-lm"]),
    )?;
    Ok(binary)
}

pub fn enum_table() -> String {
    let mut out = String::from("{\n");
    for (name, members) in astral_core::lua::enums::ALL {
        out.push_str(&format!("\t{name} = {{\n"));
        for (member, value) in *members {
            out.push_str(&format!("\t\t{member} = {value:?},\n"));
        }
        out.push_str("\t},\n");
    }
    for (name, members) in astral_core::lua::enums::ALL_NUMBERS {
        out.push_str(&format!("\t{name} = {{\n"));
        for (member, value) in *members {
            out.push_str(&format!("\t\t{member} = {value},\n"));
        }
        out.push_str("\t},\n");
    }
    out.push('}');
    out
}

fn enum_arguments(work: &Path) -> Result<[String; 2], String> {
    let path = work.join("enums.lua");
    std::fs::write(&path, format!("return {}\n", enum_table()))
        .map_err(|error| format!("Failed to write {}: {error}", path.display()))?;
    let preamble = repo::path(&["packages", "tools", "support", "enum-preamble.lua"]);
    Ok([repo::text(&preamble), repo::text(&path)])
}

pub fn test_lua(work: &Path) -> Result<(), String> {
    let root = repo::path(&[]);
    let lua = build_lua(work)?;
    let [preamble, enums] = enum_arguments(work)?;
    let prelude = repo::text(&repo::path(&["packages", "core", "lua", "prelude.lua"]));
    repo::run(
        &root,
        &lua,
        &[
            preamble.clone(),
            enums.clone(),
            repo::text(&repo::path(&[
                "packages",
                "core",
                "tests",
                "prelude-test.lua",
            ])),
            prelude.clone(),
        ],
    )?;
    for folder in repo::plugin_folders()? {
        let manifest = signing::read_manifest(&folder)?;
        let Ok(entries) = std::fs::read_dir(folder.join("tests")) else {
            continue;
        };
        let mut tests: Vec<_> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "lua"))
            .collect();
        tests.sort();
        for test in tests {
            repo::run(
                &root,
                &lua,
                &[
                    preamble.clone(),
                    enums.clone(),
                    repo::text(&test),
                    repo::text(&folder.join(&manifest.main)),
                    prelude.clone(),
                ],
            )?;
        }
    }
    Ok(())
}

pub fn test_signatures() -> Result<(), String> {
    let key = signing::public_key()?;
    let folders = repo::plugin_folders()?;
    for folder in &folders {
        signing::verify_plugin(folder, &key)?;
    }
    signing::verify_catalog(&key)
}

fn test_locator(work: &Path, phoenix: &str) -> Result<(), String> {
    let binary = repo::text(&work.join("locate-test"));
    let sources = repo::path(&["packages", "ios"]);
    let mut arguments = strings(&["-Wall", "-Wextra", "-Werror", "-O1", "tests/locate-test.c"]);
    arguments.push(format!(
        "-I{}",
        repo::text(&repo::path(&["packages", "core", "wire"]))
    ));
    arguments.extend([
        crate::fuzz::core_library(false)?,
        "-o".into(),
        binary.clone(),
    ]);
    repo::run(&sources, "clang", &arguments)?;
    repo::run(&sources, &binary, &[phoenix.to_owned()])
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("test")
        .string("phoenix", &environment("ASTRAL_PHOENIX_BINARY"))
        .parse(arguments)?;
    let work = repo::scratch()?;
    lint::test_rust()?;
    test_wire(&work.0)?;
    test_core(&work.0)?;
    test_lua(&work.0)?;
    test_signatures()?;
    crate::readme::test()?;
    crate::catalog::test()?;
    let phoenix = flags.get("phoenix");
    if !phoenix.is_empty() {
        test_locator(&work.0, &phoenix)?;
    }
    Ok(())
}
