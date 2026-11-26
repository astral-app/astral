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

pub fn lint_rust() -> Result<(), String> {
    let root = repo::path(&[]);
    repo::run(&root, "cargo", &strings(&["fmt", "--all", "--check"]))?;
    repo::run(
        &root,
        "cargo",
        &strings(&[
            "clippy",
            "--quiet",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ]),
    )
}

pub const CRANELIFT_TOOLCHAIN: &str = "nightly-2026-10-27";

pub fn cranelift_ready() -> bool {
    std::env::var("ASTRAL_CRANELIFT").as_deref() != Ok("0")
        && repo::output(
            &repo::path(&[]),
            "rustup",
            &strings(&[
                "component",
                "list",
                "--installed",
                "--toolchain",
                CRANELIFT_TOOLCHAIN,
            ]),
        )
        .is_ok_and(|installed| installed.contains("rustc-codegen-cranelift"))
}

pub fn test_rust() -> Result<(), String> {
    let root = repo::path(&[]);
    let cranelift = cranelift_ready();
    let target = repo::text(&root.join("target").join("cranelift"));
    let environment: Vec<(&str, &str)> = if cranelift {
        vec![
            ("CARGO_UNSTABLE_CODEGEN_BACKEND", "true"),
            ("CARGO_PROFILE_DEV_CODEGEN_BACKEND", "cranelift"),
            ("CARGO_TARGET_DIR", &target),
        ]
    } else {
        Vec::new()
    };
    let mut arguments = Vec::new();
    if cranelift {
        arguments.push(format!("+{CRANELIFT_TOOLCHAIN}"));
    }
    if repo::has_command("cargo-nextest") {
        arguments.extend(strings(&[
            "nextest",
            "run",
            "--workspace",
            "--no-fail-fast",
            "--status-level",
            "fail",
        ]));
    } else {
        arguments.extend(strings(&["test", "--quiet", "--workspace"]));
    }
    repo::run_with_env(&root, &environment, "cargo", &arguments)
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
    crate::lua_enums::check_plugins()?;
    crate::lua_enums::sync_sdk(true)?;
    clang_format(&["--dry-run", "-Werror"])?;
    lint_rust()?;
    crate::rust_magic::check_all()?;
    workflows::lint_all()
}

pub fn format(_: &[String]) -> Result<(), String> {
    let root = repo::path(&[]);
    repo::run(&root, "stylua", &strings(&LUA_PATHS))?;
    taplo(&strings(&["format"]))?;
    clang_format(&["-i"])?;
    repo::run(&repo::path(&[]), "cargo", &strings(&["fmt", "--all"]))?;
    workflows::format_all()
}
