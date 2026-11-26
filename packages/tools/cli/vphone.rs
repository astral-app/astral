use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use crate::files;
use crate::flags::{Flags, environment};
use crate::repo::{self, strings};
use crate::vphone_client::{Client, DISCORD_BUNDLE_ID, token_path};
use crate::vphone_deploy;
use crate::vphone_session;
use crate::vphone_ssh;

const USAGE: &str = "usage: astral vphone <command>
  create [--ipa <IPA>] [--build 24B156]  build a VM with the adhoc preset, bootstrap, ElleKit and optionally Discord and astral
  snapshot <save|revert|list> [name]      stop, snapshot or revert, and boot again
  watch                            rebuild and deploy astral on every source change
  crashes [--process Discord]      the newest crash report, with astral frames symbolicated
  capture [--seconds 30] <dir>     record Phoenix's ViewWire batches while you use Discord, decoded to <dir>/wire.jsonl
  screenshot [file]                save the screen as a JPEG (vphone-screenshot.jpg by default)
  open <target>                    open a Discord link: user <id>, channel <guild> <channel> [message], dm [channel], settings [section] or a URL
  boot [--name vphone]              restart the VM with its guest API on, using a stored token
  status                           bootstrap state and the frontmost app
  call <method> [json params]      call any guest API method
  setup                            install ElleKit on a fresh bootstrap and check that injection works
  ssh [--root] <command>           run a command in the guest over SSH (mobile, or root with --root)
  session <save|restore>           back up or restore Discord's login (kept in ~/.config/astral/vphone-session)
  install <ipa>                    upload and install an IPA
  launch <bundle id>               open an app
  info <bundle id>                 an app's version, bundle and data paths
  push <local file> <guest path>   copy a file into the guest
  pull <guest path> <local file>   copy a file out of the guest
  discord --ipa <Discord.ipa>      install stock Discord, keeping the saved login
  deploy                           build astral, install it as an ElleKit tweak and relaunch Discord
The guest API listens on VPHONE_API (default 127.0.0.1:8765) with the token in VPHONE_API_TOKEN or ~/.config/astral/vphone-token.";

const OPEN_USAGE: &str = "open <target>, where target is one of:
  user <user id>                        a profile
  channel <guild id> <channel id> [message id]
  dm [channel id]                       the DM list, or one DM
  settings [section]                    Discord's Settings, or a section such as account, privacy, notifications, voice
  discord://… or https://discord.com/…  any link Discord handles";

fn home() -> PathBuf {
    PathBuf::from(environment("HOME"))
}

pub fn vphone_home() -> PathBuf {
    home().join(".vphone")
}

fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_default()
    );
}

fn wait_for_free_port(address: &str) {
    for _ in 0..60 {
        if std::net::TcpListener::bind(address).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

pub fn boot(name: &str) -> Result<(), String> {
    let mut token = std::fs::read_to_string(token_path())
        .map(|text| text.trim().to_owned())
        .unwrap_or_default();
    if token.is_empty() {
        token = repo::output(
            &repo::path(&[]),
            "openssl",
            &strings(&["rand", "-hex", "16"]),
        )?
        .trim()
        .to_owned();
        if let Some(parent) = token_path().parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(token_path(), format!("{token}\n")).map_err(|error| error.to_string())?;
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(token_path(), std::fs::Permissions::from_mode(0o600));
    }
    if repo::output(
        &repo::path(&[]),
        "pgrep",
        &strings(&["-f", "emulator.*-avd"]),
    )
    .is_ok_and(|running| !running.trim().is_empty())
    {
        return Err(
            "the Android emulator is running; stop it first (the Mac cannot run both)".into(),
        );
    }
    let _ = Command::new("vphone-cli")
        .args(["vm", "stop", name])
        .status();
    wait_for_free_port("127.0.0.1:8765");
    let config = vphone_home()
        .join("machines")
        .join(name)
        .join("config.plist");
    let log = std::env::temp_dir().join(format!("astral-vphone-{name}.log"));
    let log_file = std::fs::File::create(&log).map_err(|error| error.to_string())?;
    Command::new("vphone-cli")
        .args([
            "boot",
            "--config",
            &repo::text(&config),
            "--api-listen",
            "127.0.0.1:8765",
        ])
        .current_dir(config.parent().unwrap_or(Path::new("/")))
        .env("VPHONE_API_TOKEN", &token)
        .stdout(log_file.try_clone().map_err(|error| error.to_string())?)
        .stderr(log_file)
        .spawn()
        .map_err(|error| error.to_string())?;
    let client = Client {
        address: "127.0.0.1:8765".into(),
        token,
    };
    for _ in 0..90 {
        if client.call("bootstrap.status", Value::Null).is_ok() {
            println!(
                "{name} is up; guest API on 127.0.0.1:8765, log in {}",
                log.display()
            );
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Err(format!(
        "the guest API did not come up; see {}",
        log.display()
    ))
}

fn ready_ipa(source: &Path, work: &Path) -> Result<PathBuf, String> {
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(source).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let target = work.join("vphone.ipa");
    let mut writer =
        zip::ZipWriter::new(std::fs::File::create(&target).map_err(|error| error.to_string())?);
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry
            .name()
            .map_err(|error| error.to_string())?
            .into_owned();
        let parts: Vec<&str> = name.split('/').collect();
        if parts.len() >= 3
            && parts[1].ends_with(".app")
            && matches!(parts[2], "PlugIns" | "Watch" | "Extensions")
        {
            continue;
        }
        if let [_, app, "Info.plist"] = parts.as_slice()
            && app.ends_with(".app")
        {
            let method = entry.compression();
            let mut data = Vec::new();
            let mut entry = entry;
            entry
                .read_to_end(&mut data)
                .map_err(|error| error.to_string())?;
            let binary = data.starts_with(b"bplist");
            let mut values: plist::Dictionary =
                plist::from_bytes(&data).map_err(|error| error.to_string())?;
            values.remove("UISupportedDevices");
            let mut encoded = Vec::new();
            if binary {
                plist::to_writer_binary(&mut encoded, &values)
                    .map_err(|error| error.to_string())?;
            } else {
                plist::to_writer_xml(&mut encoded, &values).map_err(|error| error.to_string())?;
            }
            writer
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default().compression_method(method),
                )
                .map_err(|error| error.to_string())?;
            writer
                .write_all(&encoded)
                .map_err(|error| error.to_string())?;
            continue;
        }
        writer
            .raw_copy_file(entry)
            .map_err(|error| error.to_string())?;
    }
    writer.finish().map_err(|error| error.to_string())?;
    Ok(target)
}

fn udid(name: &str) -> Result<String, String> {
    let text = repo::read(
        &vphone_home()
            .join("machines")
            .join(name)
            .join("udid-prediction.txt"),
    )?;
    text.lines()
        .find_map(|line| line.trim().strip_prefix("UDID=").map(str::to_owned))
        .ok_or_else(|| format!("no UDID for {name}"))
}

fn installed_team(client: &Client) -> String {
    client
        .app_info(DISCORD_BUNDLE_ID)
        .ok()
        .and_then(|info| {
            info.get("entitlements")
                .and_then(|entitlements| entitlements.get("com.apple.developer.team-identifier"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

fn adhoc_sign(ipa: &Path, team: &str, bundle: &str, work: &Path) -> Result<PathBuf, String> {
    let extracted = work.join("adhoc");
    files::unzip(ipa, &extracted)?;
    let apps = files::glob(&extracted.join("Payload/*.app"));
    let [app] = apps.as_slice() else {
        return Err("the IPA has no single Payload/*.app".into());
    };
    let mut entitlements = plist::Dictionary::new();
    entitlements.insert(
        "application-identifier".into(),
        format!("{team}.{bundle}").into(),
    );
    entitlements.insert("com.apple.developer.team-identifier".into(), team.into());
    entitlements.insert("get-task-allow".into(), true.into());
    entitlements.insert(
        "keychain-access-groups".into(),
        plist::Value::Array(vec![format!("{team}.{bundle}").into()]),
    );
    entitlements.insert(
        "com.apple.security.application-groups".into(),
        plist::Value::Array(vec![format!("group.{bundle}").into()]),
    );
    let path = work.join("adhoc.entitlements");
    plist::to_file_xml(&path, &entitlements).map_err(|error| error.to_string())?;
    files::sign_bundle(app, "-", &path)?;
    let signed = work.join("adhoc.ipa");
    repo::run(
        &extracted,
        "zip",
        &strings(&["-qr", "-0", &repo::text(&signed), "Payload"]),
    )?;
    Ok(signed)
}

pub fn install_discord(client: &Client, arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("vphone discord")
        .string("ipa", "")
        .string("name", "vphone")
        .parse(arguments)?;
    let ipa = flags.get("ipa");
    if ipa.is_empty() {
        return Err(
            "usage: astral vphone discord --ipa <decrypted Discord.ipa> [--name vphone]".into(),
        );
    }
    let work = repo::scratch()?;
    let ready = ready_ipa(Path::new(&ipa), &work.0)?;
    let team = installed_team(client);
    if team.is_empty() {
        client.install(&ready)?;
    } else {
        if let Ok(saved) = vphone_session::save(client) {
            println!("saved the login ({saved} files)");
        }
        let udid = udid(&flags.get("name"))?;
        let signed = adhoc_sign(&ready, &team, DISCORD_BUNDLE_ID, &work.0)?;
        repo::run(
            &work.0,
            "ideviceinstaller",
            &strings(&["-u", &udid, "install", &repo::text(&signed)]),
        )?;
    }
    if let Ok(restored) = vphone_session::restore(client, &flags.get("name")) {
        println!("restored the login ({restored} files)");
    }
    println!("installed stock Discord on the vphone; run astral vphone deploy to add astral");
    Ok(())
}

const NEVER_LOCK: i64 = 2_147_483_647;
const PROFILES: &str = "/var/mobile/Library/UserConfigurationProfiles";

fn with_never_lock(mut settings: plist::Dictionary) -> plist::Dictionary {
    let mut restricted = settings
        .get("restrictedValue")
        .and_then(|value| value.as_dictionary().cloned())
        .unwrap_or_default();
    let mut inactivity = restricted
        .get("maxInactivity")
        .and_then(|value| value.as_dictionary().cloned())
        .unwrap_or_default();
    inactivity.insert("value".into(), plist::Value::Integer(NEVER_LOCK.into()));
    restricted.insert("maxInactivity".into(), plist::Value::Dictionary(inactivity));
    settings.insert(
        "restrictedValue".into(),
        plist::Value::Dictionary(restricted),
    );
    settings
}

fn disable_auto_lock(client: &Client, name: &str) -> Result<(), String> {
    let work = repo::scratch()?;
    for file in ["EffectiveUserSettings.plist", "UserSettings.plist"] {
        let remote = format!("{PROFILES}/{file}");
        let local = work.0.join(file);
        let settings: plist::Dictionary = if client.pull(&remote, &local).is_ok() {
            plist::from_file(&local).unwrap_or_default()
        } else {
            plist::Dictionary::new()
        };
        plist::to_file_binary(&local, &with_never_lock(settings))
            .map_err(|error| error.to_string())?;
        client
            .push(&local, &remote)
            .map_err(|error| format!("write {remote}: {error}"))?;
    }
    vphone_ssh::run(name, "killall -9 profiled SpringBoard", true)
        .map_err(|error| format!("restart profiled and SpringBoard: {error}"))?;
    std::thread::sleep(Duration::from_secs(3));
    client.try_call("screen.unlock", json!({"timeout_ms": 15000}));
    let state = client.call("display.auto_lock", Value::Null)?;
    if state.get("never").and_then(Value::as_bool) != Some(true) {
        return Err("auto-lock is still on after writing the user settings".into());
    }
    Ok(())
}

const ELLEKIT_REPOSITORY: &str = "https://apt.owngoal.dev";

pub fn setup(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("vphone setup")
        .string("name", "vphone")
        .parse(arguments)?;
    let name = flags.get("name");
    let client = Client::new()?;
    vphone_ssh::run(&name, "test -x /usr/bin/irisin-cli", false)
        .map_err(|error| format!("the OwnGoal bootstrap is missing; create the VM with vphone-cli vm create --bootstrap roothide: {error}"))?;
    if vphone_ssh::run(&name, "test -f /usr/lib/TweakLoader.dylib", false).is_err() {
        vphone_ssh::run(
            &name,
            &format!("irisin-cli install -y --repository {ELLEKIT_REPOSITORY} ellekit"),
            true,
        )
        .map_err(|error| format!("installing ElleKit failed: {error}"))?;
        println!("installed ElleKit from {ELLEKIT_REPOSITORY}; restarting the VM");
        boot(&name)?;
    }
    for _ in 0..30 {
        if vphone_ssh::run(&name, "echo injected-shell-ok", false)
            .is_ok_and(|output| output.contains("injected-shell-ok"))
        {
            println!("ElleKit is injecting and bootstrap shells still run");
            disable_auto_lock(&client, &name)?;
            println!("turned auto-lock off");
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(3));
    }
    Err("shells die under ElleKit; the VM needs the txm-exp-precheck_admission patch (see the vphone skill)".into())
}

pub fn free_gigabytes(directory: &Path) -> i64 {
    let Ok(output) = Command::new("df")
        .args(["-g", &repo::text(directory)])
        .output()
    else {
        return 0;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim()
        .lines()
        .last()
        .and_then(|line| line.split_whitespace().nth(3))
        .and_then(|free| free.parse().ok())
        .unwrap_or(0)
}

fn adhoc_preset() -> Result<PathBuf, String> {
    let standard = files::glob(Path::new("/opt/homebrew/Caskroom/vphone-cli/*/VPhone.bundle/Contents/Resources/patches_presets/standard.plist"))
        .pop()
        .ok_or("no vphone-cli install found; run brew install --cask vphone-cli")?;
    let mut preset: plist::Dictionary =
        plist::from_file(&standard).map_err(|error| error.to_string())?;
    preset.insert("Identifier".into(), "standard-adhoc".into());
    preset.insert("Title".into(), "Standard with ad-hoc admission".into());
    if let Some(selection) = preset
        .get_mut("Selection")
        .and_then(plist::Value::as_dictionary_mut)
        && let Some(patches) = selection
            .get_mut("Patches")
            .and_then(plist::Value::as_array_mut)
    {
        patches.retain(|patch| patch.as_string() != Some("txm-exp-precheck_admission"));
    }
    let target = vphone_home().join("presets/standard-adhoc.plist");
    std::fs::create_dir_all(target.parent().unwrap_or(Path::new("/")))
        .map_err(|error| error.to_string())?;
    plist::to_file_xml(&target, &preset).map_err(|error| error.to_string())?;
    Ok(target)
}

fn clear_ipsw_cache() {
    for path in files::glob(&vphone_home().join("ipsws/*")) {
        let _ = Command::new("sudo")
            .args(["-n", "rm", "-rf", &repo::text(&path)])
            .status();
    }
}

fn create_vm(name: &str, build: &str, preset: &Path, disk: &str) -> Result<(), String> {
    let mut child = Command::new("sudo")
        .args([
            "-n",
            "vphone-cli",
            "vm",
            "create",
            name,
            "--iphone-build",
            build,
            "--device",
            "iPhone17,3",
            "--preset",
            &repo::text(preset),
        ])
        .args([
            "--bootstrap",
            "roothide",
            "--unlock-at-startup",
            "on",
            "--disk-size",
            disk,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| error.to_string())?;
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            println!("{line}");
            if line.contains("Firmware prepared") {
                clear_ipsw_cache();
            }
        }
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("vphone-cli vm create: {status}"))
    }
}

fn resume_template_build() -> Result<bool, String> {
    let Some(build) = files::glob(&vphone_home().join("machines/.templates/.building-*")).pop()
    else {
        return Ok(false);
    };
    let Some(first) = std::fs::read_dir(&build)
        .ok()
        .and_then(|mut entries| entries.next())
        .and_then(Result::ok)
    else {
        return Ok(false);
    };
    println!(
        "resuming the template build {}",
        build.file_name().unwrap_or_default().to_string_lossy()
    );
    repo::run(
        &repo::path(&[]),
        "sudo",
        &strings(&[
            "-n",
            "vphone-cli",
            "cfw",
            "install",
            "--library-root",
            &repo::text(&build),
            &first.file_name().to_string_lossy(),
        ]),
    )?;
    repo::run(
        &repo::path(&[]),
        "vphone-cli",
        &strings(&[
            "vm",
            "template",
            "setup",
            &build.file_name().unwrap_or_default().to_string_lossy(),
        ]),
    )?;
    Ok(true)
}

fn create(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("vphone create")
        .string("name", "vphone")
        .string("build", "24B156")
        .string("disk", "48")
        .string("ipa", "")
        .parse(arguments)?;
    let (name, build, disk) = (flags.get("name"), flags.get("build"), flags.get("disk"));
    if !Command::new("sudo")
        .args(["-n", "true"])
        .status()
        .is_ok_and(|status| status.success())
    {
        return Err("vphone-cli needs sudo without a password prompt for vm create".into());
    }
    let free = free_gigabytes(&vphone_home());
    if free < 50 {
        println!(
            "warning: {free} GB free; the firmware install needs 50 GB (the IPSW cache is cleared once firmware is prepared)"
        );
    }
    let preset = adhoc_preset()?;
    if let Err(error) = create_vm(&name, &build, &preset, &disk) {
        if !resume_template_build().unwrap_or(false) {
            return Err(format!(
                "vm create failed: {error} (free space: {} GB)",
                free_gigabytes(&vphone_home())
            ));
        }
        create_vm(&name, &build, &preset, &disk)?;
    }
    boot(&name)?;
    setup(&strings(&["--name", &name]))?;
    let ipa = flags.get("ipa");
    if ipa.is_empty() {
        println!(
            "created {name}; install Discord with astral vphone discord --ipa <IPA>, then astral vphone deploy"
        );
        return Ok(());
    }
    let client = Client::new()?;
    install_discord(&client, &strings(&["--ipa", &ipa, "--name", &name]))?;
    vphone_deploy::deploy(&client)
}

fn snapshot(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("vphone snapshot")
        .string("name", "vphone")
        .parse(arguments)?;
    let name = flags.get("name");
    let usage = || "usage: astral vphone snapshot <save|revert|list> [name]".to_owned();
    let action = flags.rest.first().cloned().ok_or_else(usage)?;
    let root = repo::path(&[]);
    if action == "list" {
        return repo::run(
            &root,
            "vphone-cli",
            &strings(&["vm", "snapshot", "list", &name]),
        );
    }
    let snapshot = flags.rest.get(1).cloned().ok_or_else(usage)?;
    repo::run(
        &root,
        "vphone-cli",
        &strings(&["vm", "stop", &name, "--timeout", "60"]),
    )?;
    match action.as_str() {
        "save" => repo::run(
            &root,
            "vphone-cli",
            &strings(&["vm", "snapshot", "create", &name, &snapshot]),
        )?,
        "revert" => {
            let mut child = Command::new("vphone-cli")
                .args(["vm", "snapshot", "revert", &name, &snapshot])
                .stdin(Stdio::piped())
                .spawn()
                .map_err(|error| error.to_string())?;
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(b"y\n");
            }
            if !child.wait().map_err(|error| error.to_string())?.success() {
                return Err("vphone-cli vm snapshot revert failed".into());
            }
        }
        _ => return Err(usage()),
    }
    boot(&name)
}

fn newest_change(roots: &[PathBuf]) -> SystemTime {
    let mut newest = SystemTime::UNIX_EPOCH;
    for root in roots {
        files::walk(
            root,
            &|path| {
                path.file_name()
                    .is_some_and(|name| name == "build" || name.to_string_lossy().starts_with('.'))
            },
            &mut |file| {
                if let Ok(modified) =
                    std::fs::metadata(file).and_then(|metadata| metadata.modified())
                {
                    newest = newest.max(modified);
                }
            },
        );
    }
    newest
}

fn watch(client: &Client) -> Result<(), String> {
    let roots = [
        repo::path(&["packages", "ios", "sources"]),
        repo::path(&["packages", "core"]),
        repo::path(&["packages", "addons", "themes"]),
        repo::path(&["packages", "addons", "plugins"]),
    ];
    let mut last = newest_change(&roots);
    println!("watching ios, core, themes and plugins; every save rebuilds and deploys");
    loop {
        std::thread::sleep(Duration::from_millis(500));
        if newest_change(&roots) <= last {
            continue;
        }
        std::thread::sleep(Duration::from_millis(300));
        last = newest_change(&roots);
        if let Err(error) = vphone_deploy::deploy(client) {
            println!("deploy failed: {error}");
        }
    }
}

const CRASH_REPORTS: &str = "/var/mobile/Library/Logs/CrashReporter";

fn symbolicate(image: &Value, offset: u64) -> Option<String> {
    let dsym = repo::path(&[
        "packages",
        "ios",
        ".theos",
        "obj",
        "arm64",
        "astral.dylib.dSYM",
    ]);
    if !dsym.exists() {
        return None;
    }
    let base = image.get("base").and_then(Value::as_u64).unwrap_or(0);
    let arch = image
        .get("arch")
        .and_then(Value::as_str)
        .filter(|arch| !arch.is_empty())
        .unwrap_or("arm64");
    let output = Command::new("atos")
        .args([
            "-arch",
            arch,
            "-o",
            &repo::text(&dsym),
            "-l",
            &format!("0x{base:x}"),
            &format!("0x{:x}", base + offset),
        ])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn crashes(client: &Client, arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("vphone crashes")
        .string("process", "Discord")
        .parse(arguments)?;
    let prefix = format!("{}-", flags.get("process"));
    let mut names: Vec<String> = client
        .names(CRASH_REPORTS)
        .into_iter()
        .filter(|name| name.starts_with(&prefix) && name.ends_with(".ips"))
        .collect();
    names.sort();
    let name = names
        .pop()
        .ok_or_else(|| format!("no {} crash reports on the vphone", flags.get("process")))?;
    let work = repo::scratch()?;
    let local = work.0.join(&name);
    client.pull(&format!("{CRASH_REPORTS}/{name}"), &local)?;
    let raw = repo::read(&local)?;
    let body = if raw.starts_with('{') {
        raw.split_once('\n').map_or(raw.as_str(), |(_, rest)| rest)
    } else {
        raw.as_str()
    };
    let report: Value =
        serde_json::from_str(body).map_err(|error| format!("parse {name}: {error}"))?;
    let text = |path: &[&str]| {
        path.iter()
            .try_fold(&report, |value, key| value.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    println!(
        "{name}\n{} {} {}; {} {}",
        text(&["exception", "type"]),
        text(&["exception", "signal"]),
        text(&["exception", "subtype"]),
        text(&["termination", "namespace"]),
        text(&["termination", "indicator"])
    );
    let images = report
        .get("usedImages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for thread in report
        .get("threads")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if thread.get("triggered").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        for (index, frame) in thread
            .get("frames")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(20)
            .enumerate()
        {
            let image = frame
                .get("imageIndex")
                .and_then(Value::as_u64)
                .and_then(|index| images.get(index as usize))
                .cloned()
                .unwrap_or(Value::Null);
            let image_name = image
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let offset = frame
                .get("imageOffset")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let mut symbol = frame
                .get("symbol")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if image_name == "astral.dylib"
                && let Some(resolved) =
                    symbolicate(&image, offset).filter(|resolved| !resolved.is_empty())
            {
                symbol = resolved;
            }
            println!("{index:2}  {image_name:<24} {symbol} +0x{offset:x}");
        }
    }
    Ok(())
}

fn screenshot(client: &Client, arguments: &[String]) -> Result<(), String> {
    let target = arguments
        .first()
        .cloned()
        .unwrap_or_else(|| "vphone-screenshot.jpg".into());
    let shot = client.call(
        "screen.screenshot",
        json!({"max_dimension": 1400, "quality": 0.8}),
    )?;
    let image = STANDARD
        .decode(shot.get("data").and_then(Value::as_str).unwrap_or_default())
        .map_err(|error| error.to_string())?;
    std::fs::write(&target, image).map_err(|error| error.to_string())?;
    println!("{target}");
    Ok(())
}

pub fn wire_dump_tool(destination: &Path) -> Result<PathBuf, String> {
    let tool = destination.join(".wire-dump");
    let support = repo::path(&["packages", "ios", "tests", "support"]);
    repo::run(
        &repo::path(&[]),
        "clang",
        &strings(&[
            "-O1",
            "-o",
            &repo::text(&tool),
            &repo::text(&support.join("wire-dump.c")),
            &repo::text(&support.join("wire-parser.c")),
        ]),
    )?;
    Ok(tool)
}

fn capture(client: &Client, arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("capture")
        .string("seconds", "30")
        .parse(arguments)?;
    let [destination] = flags.rest.as_slice() else {
        return Err("usage: astral vphone capture [--seconds 30] <output directory>".into());
    };
    let destination = PathBuf::from(destination);
    let seconds: u64 = flags
        .get("seconds")
        .parse()
        .map_err(|_| "--seconds takes a number")?;
    std::fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    let state = format!("{}/Documents/astral", client.discord_data_path()?);
    let marker = format!("{state}/capture-wire");
    vphone_ssh::run(
        "vphone",
        &format!("touch {}", vphone_ssh::shell_quote(&marker)),
        false,
    )
    .map_err(|error| format!("turn on wire capture: {error}"))?;
    let result = (|| {
        client.restart_discord()?;
        println!(
            "recording Phoenix's ViewWire batches for {seconds}s; use Discord on the vphone now"
        );
        std::thread::sleep(Duration::from_secs(seconds));
        let wire = format!("{state}/wire");
        let mut batches = Vec::new();
        for name in client.names(&wire) {
            if !name.ends_with(".bin") && !name.ends_with(".bin.out") {
                continue;
            }
            let local = destination.join(&name);
            client.pull(&format!("{wire}/{name}"), &local)?;
            if name.ends_with(".bin") {
                batches.push(repo::text(&local));
            }
        }
        batches.sort();
        if batches.is_empty() {
            return Err("no batches were captured; is astral deployed on the vphone?".to_owned());
        }
        let tool = wire_dump_tool(&destination)?;
        let dump = repo::output(&repo::path(&[]), &repo::text(&tool), &batches)?;
        let jsonl = destination.join("wire.jsonl");
        std::fs::write(&jsonl, dump).map_err(|error| error.to_string())?;
        println!("{} batches in {}", batches.len(), jsonl.display());
        Ok(())
    })();
    let _ = vphone_ssh::run(
        "vphone",
        &format!("rm -f {}", vphone_ssh::shell_quote(&marker)),
        false,
    );
    let _ = client.restart_discord();
    result
}

fn snowflake(value: &str) -> bool {
    (15..=20).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit())
}

pub fn discord_link(arguments: &[String]) -> Result<String, String> {
    let Some(kind) = arguments.first() else {
        return Err(OPEN_USAGE.into());
    };
    if kind.starts_with("discord://") || kind.starts_with("https://") {
        return Ok(kind.clone());
    }
    let ids = &arguments[1..];
    if let Some(bad) = ids.iter().find(|id| !snowflake(id) && kind != "settings") {
        return Err(format!("{bad:?} is not a Discord id"));
    }
    let joined = ids.join("/");
    match (kind.as_str(), ids.len()) {
        ("user", 1) => Ok(format!("discord://-/users/{joined}")),
        ("channel", 2 | 3) => Ok(format!("discord://-/channels/{joined}")),
        ("dm", 0 | 1) => Ok(format!("discord://-/channels/@me/{joined}")
            .trim_end_matches('/')
            .to_owned()),
        ("settings", 0 | 1) => Ok(format!("discord://-/settings/{joined}")
            .trim_end_matches('/')
            .to_owned()),
        _ => Err(OPEN_USAGE.into()),
    }
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let Some(command) = arguments.first().map(String::as_str) else {
        return Err(USAGE.into());
    };
    let rest = &arguments[1..];
    match command {
        "create" => return create(rest),
        "snapshot" => return snapshot(rest),
        "boot" => {
            return boot(
                &Flags::new("vphone boot")
                    .string("name", "vphone")
                    .parse(rest)?
                    .get("name"),
            );
        }
        "ssh" => return vphone_ssh::command(rest),
        "setup" => return setup(rest),
        _ => {}
    }
    let client = Client::new()?;
    match (command, rest) {
        ("call", [method]) => print_json(&client.call(method, json!({}))?),
        ("call", [method, params]) => {
            let params: Value = serde_json::from_str(params).map_err(|error| format!("params must be a JSON object: {error}"))?;
            print_json(&client.call(method, params)?);
        }
        ("status", []) => print_json(&client.call("bootstrap.status", Value::Null)?),
        ("install", [ipa]) => client.install(Path::new(ipa))?,
        ("info", [bundle]) => print_json(&client.app_info(bundle)?),
        ("launch", [bundle]) => {
            client.call("apps.launch", json!({"bundle_id": bundle}))?;
        }
        ("push", [local, remote]) => client.push(Path::new(local), remote)?,
        ("pull", [remote, local]) => client.pull(remote, Path::new(local))?,
        ("deploy", []) => vphone_deploy::deploy(&client)?,
        ("deploy", _) => return Err("usage: astral vphone deploy (install Discord first with astral vphone discord --ipa <IPA>)".into()),
        ("session", _) => vphone_session::command(&client, rest)?,
        ("watch", _) => watch(&client)?,
        ("crashes", _) => crashes(&client, rest)?,
        ("capture", _) => capture(&client, rest)?,
        ("screenshot", _) => screenshot(&client, rest)?,
        ("open", _) => {
            let link = discord_link(rest)?;
            client.try_call("screen.unlock", json!({"timeout_ms": 5000}));
            client.call("apps.launch", json!({"bundle_id": DISCORD_BUNDLE_ID, "url": link}))?;
        }
        ("discord", _) => install_discord(&client, rest)?,
        _ => return Err(USAGE.into()),
    }
    Ok(())
}
