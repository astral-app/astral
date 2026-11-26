use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::json;

use crate::android;
use crate::files;
use crate::flags::{Flags, environment};
use crate::repo::{self, strings};
use crate::vphone;
use crate::vphone_client::{Client, DISCORD_BUNDLE_ID};
use crate::vphone_deploy;
use crate::vphone_ssh;

const MINIMUM_FREE_GIGABYTES: i64 = 15;
const ADHOC_ADMISSION: &str = "txm-exp-precheck_admission";

pub const PINNED_TOOLS: [(&str, &str); 4] = [
    ("clang-format", r"clang-format==([0-9.]+)"),
    ("stylua", r"StyLua/releases/download/v([0-9.]+)"),
    ("taplo", r"taplo/releases/download/([0-9.]+)/"),
    (
        "lua-language-server",
        r"lua-language-server/releases/download/([0-9.]+)/",
    ),
];

type Check = Box<dyn Fn() -> Result<String, String>>;

struct Diagnosis {
    name: String,
    required: bool,
    check: Check,
    remedy: String,
}

fn diagnosis(name: &str, required: bool, check: Check, remedy: &str) -> Diagnosis {
    Diagnosis {
        name: name.to_owned(),
        required,
        check,
        remedy: remedy.to_owned(),
    }
}

pub fn pins_from(workflow: &str) -> std::collections::BTreeMap<String, String> {
    PINNED_TOOLS
        .iter()
        .filter_map(|(binary, pattern)| {
            regex::Regex::new(pattern)
                .ok()?
                .captures(workflow)
                .map(|found| ((*binary).to_owned(), found[1].to_owned()))
        })
        .collect()
}

fn on_path(binary: &str) -> bool {
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .any(|directory| Path::new(directory).join(binary).is_file())
}

fn tool_output(binary: &str, arguments: &[&str]) -> Result<Vec<String>, String> {
    if !on_path(binary) {
        return Err("not on PATH".into());
    }
    let output = Command::new(binary)
        .args(arguments)
        .output()
        .map_err(|error| error.to_string())?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let text = text.trim().to_owned();
    if !output.status.success() && text.is_empty() {
        return Err(output.status.to_string());
    }
    Ok(text.lines().map(str::to_owned).collect())
}

fn check_tool(binary: &'static str, arguments: &'static [&'static str]) -> Check {
    Box::new(move || {
        let lines = tool_output(binary, arguments)?;
        Ok(lines
            .first()
            .map_or("", |line| line.trim())
            .chars()
            .take(60)
            .collect())
    })
}

pub fn version_matches(output: &str, pinned: &str) -> bool {
    let pattern = match pinned.strip_suffix(r".[0-9]+") {
        Some(minor) => format!(r"{}\.[0-9]+", regex::escape(minor)),
        None => regex::escape(pinned),
    };
    regex::Regex::new(&format!(r"(^|[^0-9.]){pattern}($|[^0-9.])"))
        .is_ok_and(|expression| expression.is_match(output))
}

pub fn minor_of(version: &str) -> String {
    let parts: Vec<&str> = version.splitn(3, '.').collect();
    if parts.len() < 2 {
        return version.to_owned();
    }
    format!("{}.{}", parts[0], parts[1])
}

fn check_pinned(binary: &'static str, pinned: String) -> Check {
    Box::new(move || {
        let lines = tool_output(binary, &["--version"])?;
        let version = lines.last().map_or("", |line| line.trim()).to_owned();
        if pinned.is_empty() || version_matches(&version, &pinned) {
            return Ok(version);
        }
        if version_matches(&version, &format!(r"{}.[0-9]+", minor_of(&pinned))) {
            return Ok(format!("{version} (CI pins {pinned}; same minor)"));
        }
        Err(format!("found {version}, CI pins {pinned}"))
    })
}

fn check_file(path: PathBuf) -> Check {
    Box::new(move || {
        if path.exists() {
            Ok(repo::text(&path))
        } else {
            Err(format!("missing {}", path.display()))
        }
    })
}

fn check_hooks() -> Result<String, String> {
    let hooks = repo::output(
        &repo::path(&[]),
        "git",
        &strings(&["config", "core.hooksPath"]),
    )
    .unwrap_or_default();
    if hooks.trim() != ".githooks" {
        return Err(format!("hooks path is {:?}", hooks.trim()));
    }
    Ok(".githooks".into())
}

fn check_vendor() -> Result<String, String> {
    for file in ["lua/lua.h"] {
        if !repo::path(&["vendor", file]).exists() {
            return Err(format!("missing vendor/{file}"));
        }
    }
    Ok("lua".into())
}

fn check_theos() -> Result<String, String> {
    let theos = Some(environment("THEOS"))
        .filter(|theos| !theos.is_empty())
        .map_or_else(
            || PathBuf::from(environment("HOME")).join("theos"),
            PathBuf::from,
        );
    let sdks = files::glob(&theos.join("sdks/iPhoneOS*.sdk"));
    if !theos.join("makefiles/common.mk").exists() || sdks.is_empty() {
        return Err(format!(
            "no Theos with an iPhoneOS SDK at {}",
            theos.display()
        ));
    }
    Ok(format!("{} ({} SDKs)", theos.display(), sdks.len()))
}

fn check_ndk() -> Result<String, String> {
    let toolchain = android::ndk_toolchain("")?;
    Ok(toolchain
        .ancestors()
        .nth(5)
        .map(repo::text)
        .unwrap_or_default())
}

fn check_vphone_api() -> Result<String, String> {
    let client = Client::new()?;
    let reachable = client
        .address
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| addresses.next())
        .is_some_and(|address| {
            TcpStream::connect_timeout(&address, Duration::from_secs(1)).is_ok()
        });
    if !reachable {
        return Err(format!("nothing listening on {}", client.address));
    }
    Ok(client.address)
}

fn machine_file(name: &str) -> PathBuf {
    vphone::vphone_home().join("machines/vphone").join(name)
}

fn check_vphone_preset() -> Result<String, String> {
    let preset = std::fs::read_to_string(machine_file("PatchPreset.plist"))
        .map_err(|_| "no vphone machine; run astral vphone create".to_owned())?;
    if preset.contains(ADHOC_ADMISSION) {
        return Err(format!(
            "the machine keeps {ADHOC_ADMISSION}, so ElleKit kills ad-hoc signed binaries"
        ));
    }
    let selection =
        std::fs::read_to_string(machine_file("PatchSelection.plist")).unwrap_or_default();
    Ok(if selection.contains("standard-adhoc") {
        "standard-adhoc".into()
    } else {
        "admission precheck off".into()
    })
}

fn jailbroken() -> Result<(Client, String), String> {
    let client = Client::new()?;
    let root = vphone_deploy::jailbreak_root(&client)?;
    Ok((client, root))
}

fn check_vphone_ellekit() -> Result<String, String> {
    let (client, root) = jailbroken()?;
    if !client.exists(&format!("{root}/usr/lib"), "TweakLoader.dylib") {
        return Err("ElleKit is not installed".into());
    }
    let status = vphone_ssh::run(
        "vphone",
        &format!(
            "awk '/^Package: ellekit$/,/^$/' {}",
            vphone_ssh::shell_quote(&format!("{root}/var/lib/dpkg/status"))
        ),
        false,
    )?;
    if !status.contains("Dhinak") {
        return Err(
            "ElleKit is not OwnGoal's build; the roothide repo's crashes in libinjector".into(),
        );
    }
    Ok(status
        .lines()
        .find_map(|line| line.strip_prefix("Version: "))
        .map_or_else(
            || "OwnGoal".into(),
            |version| format!("OwnGoal {}", version.trim()),
        ))
}

fn check_vphone_tweak() -> Result<String, String> {
    let (client, root) = jailbroken()?;
    let tweaks = format!("{root}/usr/lib/TweakInject");
    if !client.exists(&tweaks, "astral.dylib") || !client.exists(&tweaks, "astral.plist") {
        return Err("astral is not in TweakInject".into());
    }
    Ok("TweakInject/astral.dylib".into())
}

fn check_vphone_discord() -> Result<String, String> {
    let client = Client::new()?;
    let info = client
        .call("apps.info", json!({ "bundle_id": DISCORD_BUNDLE_ID }))
        .map_err(|_| "Discord is not installed".to_owned())?;
    let field = |name: &str| {
        info.get(name)
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_owned()
    };
    let version = field("version");
    if client.exists(
        &format!("{}/Frameworks", field("bundle_path")),
        "astral.dylib",
    ) {
        return Err(format!(
            "Discord {version} has astral built in, so the tweak would load twice"
        ));
    }
    Ok(format!("stock Discord {version}"))
}

fn running(name: &str) -> bool {
    Command::new("pgrep")
        .args(["-f", name])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn check_single_vm() -> Result<String, String> {
    match (running("qemu-system"), running("vphone-cli")) {
        (true, true) => Err("the AVD and the vphone are both running".into()),
        (true, false) => Ok("AVD running".into()),
        (false, true) => Ok("vphone running".into()),
        (false, false) => Ok("neither running".into()),
    }
}

fn check_disk() -> Result<String, String> {
    let free = vphone::free_gigabytes(&repo::path(&[]));
    if free < MINIMUM_FREE_GIGABYTES {
        return Err(format!("{free} GB free"));
    }
    Ok(format!("{free} GB free"))
}

fn diagnoses(pins: &std::collections::BTreeMap<String, String>) -> Vec<Diagnosis> {
    let mut items = vec![
        diagnosis(
            "git hooks",
            true,
            Box::new(check_hooks),
            "run: git config core.hooksPath .githooks",
        ),
        diagnosis(
            "vendored sources",
            true,
            Box::new(check_vendor),
            "run: git submodule update --init",
        ),
        diagnosis(
            "plugin signing key",
            true,
            check_file(repo::path(&["keys", "plugin-signing.pub"])),
            "restore keys/plugin-signing.pub from git",
        ),
        diagnosis(
            "clang",
            true,
            check_tool("clang", &["--version"]),
            "install the Xcode command line tools",
        ),
        diagnosis(
            "cargo",
            true,
            check_tool("cargo", &["--version"]),
            "install Rust with rustup",
        ),
        diagnosis(
            "blaze",
            false,
            check_tool("blaze", &["--version"]),
            "optional: faster host builds with ASTRAL_BLAZE=1; install from https://ix.dev/blaze",
        ),
        diagnosis(
            "luacheck",
            true,
            check_tool("luacheck", &["--version"]),
            "run: brew install luacheck",
        ),
        diagnosis(
            "yamlfmt",
            true,
            check_tool("yamlfmt", &["-version"]),
            "run: brew install yamlfmt",
        ),
        diagnosis(
            "actionlint",
            true,
            check_tool("actionlint", &["-version"]),
            "run: brew install actionlint",
        ),
    ];
    for (binary, _) in PINNED_TOOLS {
        let pinned = pins.get(binary).cloned().unwrap_or_default();
        let remedy = format!("install {binary} {pinned} to match CI");
        items.push(diagnosis(
            binary,
            true,
            check_pinned(binary, pinned),
            &remedy,
        ));
    }
    items.extend([
        diagnosis(
            "theos",
            false,
            Box::new(check_theos),
            "run: astral prepare-theos --theos ~/theos",
        ),
        diagnosis(
            "gnu make",
            false,
            check_file(PathBuf::from("/opt/homebrew/opt/make/libexec/gnubin/make")),
            "run: brew install make",
        ),
        diagnosis(
            "ldid",
            false,
            check_tool("ldid", &[]),
            "run: brew install ldid",
        ),
        diagnosis(
            "android ndk",
            false,
            Box::new(check_ndk),
            "install an NDK with Android Studio or set ANDROID_NDK_HOME",
        ),
        diagnosis(
            "vphone-cli",
            false,
            check_tool("vphone-cli", &["--help"]),
            "install vphone-cli for astral vphone",
        ),
        diagnosis(
            "ideviceinstaller",
            false,
            check_tool("ideviceinstaller", &["--help"]),
            "run: brew install ideviceinstaller",
        ),
        diagnosis(
            "vphone api",
            false,
            Box::new(check_vphone_api),
            "run: astral vphone boot",
        ),
        diagnosis(
            "vphone preset",
            false,
            Box::new(check_vphone_preset),
            "rebuild with astral vphone create (it uses the adhoc preset)",
        ),
        diagnosis(
            "vphone ellekit",
            false,
            Box::new(check_vphone_ellekit),
            "run: astral vphone setup",
        ),
        diagnosis(
            "vphone tweak",
            false,
            Box::new(check_vphone_tweak),
            "run: astral vphone deploy",
        ),
        diagnosis(
            "vphone discord",
            false,
            Box::new(check_vphone_discord),
            "run: astral vphone discord --ipa <decrypted Discord.ipa>",
        ),
        diagnosis(
            "one vm at a time",
            false,
            Box::new(check_single_vm),
            "stop the AVD or the vphone; the Mac lacks memory for both",
        ),
        diagnosis(
            "free disk",
            false,
            Box::new(check_disk),
            &format!("free at least {MINIMUM_FREE_GIGABYTES} GB"),
        ),
    ]);
    items
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    Flags::new("doctor").parse(arguments)?;
    let workflow = repo::read(&repo::path(&[".github", "workflows", "ci.yml"]))?;
    let mut failed = 0;
    for item in diagnoses(&pins_from(&workflow)) {
        match (item.check)() {
            Ok(detail) => println!("ok    {:<22} {detail}", item.name),
            Err(error) if item.required => {
                failed += 1;
                println!("fail  {:<22} {error}; {}", item.name, item.remedy);
            }
            Err(error) => println!("warn  {:<22} {error}; {}", item.name, item.remedy),
        }
    }
    if failed > 0 {
        return Err(format!("{failed} required checks failed"));
    }
    Ok(())
}
