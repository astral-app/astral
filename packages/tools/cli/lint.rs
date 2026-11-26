use std::path::PathBuf;

use crate::repo::{self, strings};
use crate::workflows;

pub const LUA_PATHS: [&str; 4] = [
    "packages/addons/plugins",
    "packages/core/lua",
    "packages/core/tests",
    "packages/sdk",
];
const NATIVE_PATTERNS: [&str; 5] = [
    "packages/*.c",
    "packages/*.h",
    "packages/*.m",
    "packages/*.mm",
    "packages/*.cpp",
];
const RUST_CRATES: [&[&str]; 2] = [&["packages", "core"], &["packages", "tools"]];

pub fn taplo(arguments: &[String]) -> Result<(), String> {
    repo::run_with_env(
        &repo::path(&[]),
        &[("RUST_LOG", "warn")],
        "taplo",
        arguments,
    )
}

fn clang_format(arguments: &[&str]) -> Result<(), String> {
    let files = repo::tracked_files(&NATIVE_PATTERNS)?;
    if files.is_empty() {
        return Ok(());
    }
    let mut all = strings(arguments);
    all.extend(files);
    repo::run(&repo::path(&[]), "clang-format", &all)
}

fn crates() -> Vec<PathBuf> {
    RUST_CRATES.iter().map(|parts| repo::path(parts)).collect()
}

pub fn lint_rust() -> Result<(), String> {
    for directory in crates() {
        repo::run(&directory, "cargo", &strings(&["fmt", "--check"]))?;
        repo::run(
            &directory,
            "cargo",
            &strings(&["clippy", "--quiet", "--all-targets", "--", "-D", "warnings"]),
        )?;
    }
    Ok(())
}

pub fn test_rust() -> Result<(), String> {
    for directory in crates() {
        repo::run(&directory, "cargo", &strings(&["test", "--quiet"]))?;
    }
    Ok(())
}

pub fn typecheck_lua() -> Result<(), String> {
    let logs = repo::scratch()?;
    let result = std::process::Command::new("lua-language-server")
        .arg(format!("--check={}", repo::text(&repo::path(&[]))))
        .arg(format!(
            "--configpath={}",
            repo::text(&repo::path(&[".luarc.json"]))
        ))
        .arg("--checklevel=Warning")
        .arg(format!("--logpath={}", repo::text(&logs.0)))
        .output()
        .map_err(|error| format!("lua-language-server: {error}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    )
    .replace('\r', "\n");
    if text.contains("no problems found") {
        return Ok(());
    }
    let mut findings: Vec<String> = text
        .lines()
        .filter(|line| line.contains("[Warning]") || line.contains("[Error]"))
        .map(|line| line.trim().to_owned())
        .collect();
    if findings.is_empty() {
        findings.push(text.trim().to_owned());
    }
    Err(format!("lua-language-server:\n{}", findings.join("\n")))
}

pub fn run(_: &[String]) -> Result<(), String> {
    let root = repo::path(&[]);
    let mut stylua = strings(&["--check"]);
    stylua.extend(strings(&LUA_PATHS));
    repo::run(&root, "stylua", &stylua)?;
    taplo(&strings(&["lint"]))?;
    taplo(&strings(&["format", "--check"]))?;
    repo::run(
        &root,
        "luacheck",
        &strings(&[
            "--quiet",
            "packages/addons/plugins",
            "packages/core/lua",
            "packages/core/tests",
        ]),
    )?;
    typecheck_lua()?;
    clang_format(&["--dry-run", "-Werror"])?;
    lint_rust()?;
    workflows::lint_all()
}

pub fn format(_: &[String]) -> Result<(), String> {
    let root = repo::path(&[]);
    repo::run(&root, "stylua", &strings(&LUA_PATHS))?;
    taplo(&strings(&["format"]))?;
    clang_format(&["-i"])?;
    for directory in crates() {
        repo::run(&directory, "cargo", &strings(&["fmt"]))?;
    }
    workflows::format_all()
}
