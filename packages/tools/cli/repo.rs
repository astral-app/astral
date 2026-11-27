use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

pub const PACK_KINDS: [&str; 2] = ["icon-packs", "emoji-packs"];

pub fn root() -> Result<PathBuf, String> {
    let mut directory = std::env::current_dir().map_err(|error| error.to_string())?;
    loop {
        if directory.join("packages/core/Cargo.toml").is_file()
            && directory.join("packages/ios").is_dir()
        {
            return Ok(directory);
        }
        if !directory.pop() {
            return Err("not inside the astral repository".into());
        }
    }
}

pub fn path(parts: &[&str]) -> PathBuf {
    let mut path = root().unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1)
    });
    for part in parts {
        path.push(part);
    }
    path
}

pub fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn describe(name: &str, arguments: &[String]) -> String {
    let text = format!("{name} {}", arguments.join(" "));
    if text.chars().count() > 160 {
        format!("{}…", text.chars().take(160).collect::<String>())
    } else {
        text
    }
}

pub fn strings<S: AsRef<str>>(arguments: &[S]) -> Vec<String> {
    arguments
        .iter()
        .map(|argument| argument.as_ref().to_owned())
        .collect()
}

const BLAZE_SUBCOMMANDS: [&str; 3] = ["check", "run", "test"];

fn blaze_opt_in() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        std::env::var_os("ASTRAL_BLAZE").is_some()
            && Command::new("blaze")
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
    })
}

pub fn has_command(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub fn rust_profile() -> String {
    Some(std::env::var("ASTRAL_RUST_PROFILE").unwrap_or_default())
        .filter(|profile| !profile.is_empty())
        .unwrap_or_else(|| "device".to_owned())
}

pub fn rust(directory: &Path, subcommand: &str, cargo_arguments: &[&str]) -> Result<(), String> {
    if blaze_opt_in() && BLAZE_SUBCOMMANDS.contains(&subcommand) {
        return run(directory, "blaze", &strings(&[subcommand, "."]));
    }
    run(directory, "cargo", &strings(cargo_arguments))
}

pub fn run(directory: &Path, name: &str, arguments: &[String]) -> Result<(), String> {
    run_with_env(directory, &[], name, arguments)
}

pub fn run_with_env(
    directory: &Path,
    environment: &[(&str, &str)],
    name: &str,
    arguments: &[String],
) -> Result<(), String> {
    let status = Command::new(name)
        .args(arguments)
        .current_dir(directory)
        .envs(environment.iter().copied())
        .status()
        .map_err(|error| format!("{}: {error}", describe(name, arguments)))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{}: {status}", describe(name, arguments)))
    }
}

pub fn output(directory: &Path, name: &str, arguments: &[String]) -> Result<String, String> {
    let result = Command::new(name)
        .args(arguments)
        .current_dir(directory)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("{}: {error}", describe(name, arguments)))?;
    let stdout = String::from_utf8_lossy(&result.stdout).into_owned();
    if result.status.success() {
        Ok(stdout)
    } else {
        Err(format!(
            "{}: {}: {}",
            describe(name, arguments),
            result.status,
            String::from_utf8_lossy(&result.stderr).trim()
        ))
    }
}

pub fn tracked_files(patterns: &[&str]) -> Result<Vec<String>, String> {
    let mut arguments = strings(&[
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "--",
    ]);
    arguments.extend(strings(patterns));
    Ok(output(&path(&[]), "git", &arguments)?
        .split_whitespace()
        .map(str::to_owned)
        .collect())
}

pub fn plugin_folders() -> Result<Vec<PathBuf>, String> {
    let directory = path(&["packages", "addons", "plugins"]);
    let mut folders: Vec<PathBuf> = std::fs::read_dir(&directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|folder| folder.join("manifest.toml").is_file())
        .collect();
    folders.sort();
    Ok(folders)
}

pub fn pack_folders() -> Result<Vec<PathBuf>, String> {
    let mut folders = Vec::new();
    for kind in PACK_KINDS {
        let directory = path(&["packages", "addons", kind]);
        folders.extend(
            std::fs::read_dir(&directory)
                .map_err(|error| format!("{}: {error}", directory.display()))?
                .flatten()
                .map(|entry| entry.path())
                .filter(|folder| folder.join("manifest.toml").is_file()),
        );
    }
    folders.sort();
    Ok(folders)
}

pub fn file_exists(path: &Path) -> bool {
    path.exists()
}

pub struct Scratch(pub PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn scratch() -> Result<Scratch, String> {
    let directory = std::env::temp_dir().join(format!(
        "astral-{}-{}",
        std::process::id(),
        std::time::UNIX_EPOCH
            .elapsed()
            .map_or(0, |elapsed| elapsed.as_nanos())
    ));
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(Scratch(directory))
}

pub fn relative(path: &Path) -> String {
    path.strip_prefix(self::path(&[]))
        .map_or_else(|_| text(path), text)
}

pub fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn under(path: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| path.starts_with(prefix))
}

pub fn extension(path: &str) -> &str {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
}

pub fn output_bytes(directory: &Path, name: &str, arguments: &[String]) -> Result<Vec<u8>, String> {
    let result = Command::new(name)
        .args(arguments)
        .current_dir(directory)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("{}: {error}", describe(name, arguments)))?;
    if result.status.success() {
        Ok(result.stdout)
    } else {
        Err(format!(
            "{}: {}: {}",
            describe(name, arguments),
            result.status,
            String::from_utf8_lossy(&result.stderr).trim()
        ))
    }
}
