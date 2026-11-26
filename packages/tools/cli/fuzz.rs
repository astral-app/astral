use std::path::PathBuf;

use crate::files;
use crate::flags::{Flags, environment};
use crate::repo::{self, strings};

pub fn core_library(release: bool) -> Result<String, String> {
    let mut arguments = strings(&["build", "--quiet"]);
    if release {
        arguments.push("--release".into());
    }
    repo::run(&repo::path(&["packages", "core"]), "cargo", &arguments)?;
    Ok(repo::text(&repo::path(&[
        "packages",
        "core",
        "target",
        if release { "release" } else { "debug" },
        "libastral_core.a",
    ])))
}

fn wire_sources() -> Result<Vec<String>, String> {
    Ok(vec![
        format!(
            "-I{}",
            repo::text(&repo::path(&["packages", "core", "wire"]))
        ),
        core_library(false)?,
    ])
}

fn fuzz_clang(work: &std::path::Path) -> Result<String, String> {
    let mut candidates = vec![
        environment("ASTRAL_FUZZ_CLANG"),
        "/opt/homebrew/opt/llvm/bin/clang".into(),
        "/usr/local/opt/llvm/bin/clang".into(),
    ];
    if let Ok(path) = repo::output(&repo::path(&[]), "which", &strings(&["clang"])) {
        candidates.push(path.trim().to_owned());
    }
    let probe = work.join("probe.c");
    std::fs::write(&probe, "#include <stddef.h>\n#include <stdint.h>\nint LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) { (void)data; (void)size; return 0; }\n")
        .map_err(|error| error.to_string())?;
    for candidate in candidates
        .into_iter()
        .filter(|candidate| !candidate.is_empty() && std::path::Path::new(candidate).exists())
    {
        let built = std::process::Command::new(&candidate)
            .args([
                "-fsanitize=fuzzer",
                &repo::text(&probe),
                "-o",
                &repo::text(&work.join("probe")),
            ])
            .output();
        if built.is_ok_and(|output| output.status.success()) {
            return Ok(candidate);
        }
    }
    Err("no clang with libFuzzer; install LLVM (brew install llvm) or set ASTRAL_FUZZ_CLANG".into())
}

pub fn fuzz(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("fuzz")
        .string("seconds", "60")
        .string("corpus", "")
        .parse(arguments)?;
    let work = repo::scratch()?;
    let clang = fuzz_clang(&work.0)?;
    let corpus = if flags.get("corpus").is_empty() {
        work.0.join("corpus")
    } else {
        PathBuf::from(flags.get("corpus"))
    };
    let tests = repo::path(&["packages", "ios", "tests"]);
    let generator = repo::text(&work.0.join("seeds"));
    let mut arguments = strings(&["-O1", "-w", "-o", &generator, "fuzz/seeds.c"]);
    arguments.extend(wire_sources()?);
    arguments.push("-lm".into());
    repo::run(&tests, "cc", &arguments)?;
    std::fs::create_dir_all(&corpus).map_err(|error| error.to_string())?;
    repo::run(&tests, &generator, &[repo::text(&corpus)])?;
    let target = repo::text(&work.0.join("rewrite-fuzz"));
    let mut arguments = strings(&[
        "-g",
        "-O1",
        "-fsanitize=fuzzer,address,undefined",
        "-fno-sanitize-recover=all",
        "-o",
        &target,
        "fuzz/rewrite-fuzz.c",
    ]);
    arguments.extend(wire_sources()?);
    arguments.push("-lm".into());
    repo::run(&tests, &clang, &arguments)?;
    let artifacts = work.0.join("artifacts");
    std::fs::create_dir_all(&artifacts).map_err(|error| error.to_string())?;
    let result = repo::run(
        &work.0,
        &target,
        &[
            format!("-max_total_time={}", flags.get("seconds")),
            format!("-artifact_prefix={}/", repo::text(&artifacts)),
            "-print_final_stats=1".into(),
            repo::text(&corpus),
        ],
    );
    let crashes = files::glob(&artifacts.join("*"));
    if !crashes.is_empty() {
        let kept = repo::path(&["fuzz-crashes"]);
        for crash in &crashes {
            let _ = files::copy(crash, &kept.join(crash.file_name().unwrap_or_default()));
        }
        let names: Vec<String> = crashes.iter().map(|crash| repo::text(crash)).collect();
        return Err(format!(
            "the rewriter crashed on {} input(s), saved in fuzz-crashes/: {}",
            crashes.len(),
            names.join(", ")
        ));
    }
    result
}

pub fn bench(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("bench").parse(arguments)?;
    let work = repo::scratch()?;
    let binary = repo::text(&work.0.join("rewrite-bench"));
    let library = core_library(true)?;
    repo::run(
        &repo::path(&["packages", "ios", "tests"]),
        "cc",
        &strings(&[
            "-O2",
            "-flto",
            "-w",
            "-o",
            &binary,
            "bench/rewrite-bench.c",
            &format!(
                "-I{}",
                repo::text(&repo::path(&["packages", "core", "wire"]))
            ),
            &library,
            "-lm",
        ]),
    )?;
    repo::run(&repo::path(&[]), &binary, &flags.rest)
}

pub fn replay(arguments: &[String]) -> Result<(), String> {
    let usage =
        || "usage: astral replay <plugin folder>... <capture directory or .bin batches>".to_owned();
    let (mut plugins, mut batches) = (Vec::new(), Vec::new());
    for argument in arguments {
        let path = PathBuf::from(argument);
        if path.is_dir() && path.join("manifest.toml").exists() {
            plugins.push(argument.clone());
        } else if path.is_dir() {
            batches.extend(
                files::glob(&path.join("*.bin"))
                    .iter()
                    .map(|batch| repo::text(batch)),
            );
        } else if argument.ends_with(".bin") {
            batches.push(argument.clone());
        } else if !path.exists() {
            return Err(format!("{argument}: no such file or directory"));
        }
    }
    if plugins.is_empty() || batches.is_empty() {
        return Err(usage());
    }
    let work = repo::scratch()?;
    let lua = repo::text(&work.0.join("lua.o"));
    let platform = if cfg!(target_os = "macos") {
        "-DLUA_USE_MACOSX"
    } else {
        "-DLUA_USE_LINUX"
    };
    repo::run(
        &repo::path(&[]),
        "cc",
        &strings(&[
            "-c",
            "-O1",
            "-DMAKE_LIB",
            platform,
            "-w",
            &repo::text(&repo::path(&["vendor", "lua", "onelua.c"])),
            "-o",
            &lua,
        ]),
    )?;
    let binary = repo::text(&work.0.join("replay"));
    let include = |parts: &[&str]| format!("-I{}", repo::text(&repo::path(parts)));
    let mut build = strings(&["-std=c++20", "-O1"]);
    build.extend([
        include(&["packages", "core", "include"]),
        include(&["vendor", "lua"]),
        include(&["packages", "core", "wire"]),
        "-isystem".into(),
        repo::text(&repo::path(&["vendor", "json", "single_include"])),
        "-isystem".into(),
        repo::text(&repo::path(&["vendor", "tomlplusplus", "include"])),
        repo::text(&repo::path(&["packages", "core", "replay", "replay.cpp"])),
        lua,
        core_library(false)?,
        "-lsqlite3".into(),
        "-lm".into(),
        "-o".into(),
        binary.clone(),
    ]);
    repo::run(&repo::path(&[]), "clang++", &build)?;
    let mut replay = vec![repo::text(&repo::path(&[
        "packages",
        "core",
        "lua",
        "prelude.lua",
    ]))];
    replay.extend(plugins);
    replay.push("--".into());
    replay.extend(batches);
    repo::run(&repo::path(&[]), &binary, &replay)
}
