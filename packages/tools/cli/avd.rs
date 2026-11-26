use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use regex::Regex;

use crate::android;
use crate::flags::{Flags, environment};
use crate::repo::{self, strings};

const USAGE: &str = "usage: astral avd <command>
  boot [--name <avd>]            start the emulator headless and wait until it is ready (refuses while the vphone runs)
  stop                           shut the emulator down
  status                         Discord version, launch client, module state and astral's dev status
  discord --apk <APK>...         install Discord's base APK and splits, keeping its data
  deploy                         build the astral module, install and enable it, relaunch Discord on Phoenix
  launch                         relaunch Discord and wait for Phoenix
  screenshot <file.png>          save the screen
  open <target>                  open a Discord link: user <id>, channel <guild> <channel> [message], dm [channel], settings [section] or a URL
  tap <text|id|label>            tap the first view whose text, resource id or content description matches
  settings [row]                 open Discord's Settings, then optionally an astral row (general, plugins, design…)
  logs [--since <seconds>]       astral's logcat lines";
const DISCORD: &str = "com.discord";
const ACTIVITY: &str = "nativeapp.MainActivity";
const VECTOR: &str = "sh /data/adb/modules/zygisk_vector/cli";
const MODULE: &str = "app.astral.android";
const PHOENIX_PREFERENCE: &str = "/data/data/com.discord/shared_prefs/discord_fat_client.xml";

fn sdk() -> PathBuf {
    let configured = environment("ANDROID_HOME");
    if configured.is_empty() {
        PathBuf::from(environment("HOME")).join("Library/Android/sdk")
    } else {
        PathBuf::from(configured)
    }
}

fn adb_binary() -> String {
    let candidate = sdk().join("platform-tools/adb");
    if candidate.exists() {
        repo::text(&candidate)
    } else {
        "adb".into()
    }
}

fn adb_arguments(arguments: &[&str]) -> Vec<String> {
    let serial = environment("ANDROID_SERIAL");
    let mut all = Vec::new();
    if !serial.is_empty() {
        all.extend(strings(&["-s", &serial]));
    }
    all.extend(strings(arguments));
    all
}

fn shell(arguments: &[&str]) -> Result<(), String> {
    repo::run(&repo::path(&[]), &adb_binary(), &adb_arguments(arguments))
}

fn output(arguments: &[&str]) -> Result<String, String> {
    repo::output(&repo::path(&[]), &adb_binary(), &adb_arguments(arguments))
}

fn root(command: &str) -> Result<String, String> {
    output(&["shell", "su", "0", "sh", "-c", command])
}

fn booted() -> bool {
    output(&["shell", "getprop", "sys.boot_completed"]).is_ok_and(|value| value.trim() == "1")
}

fn prepare_screen() -> Result<(), String> {
    for command in [
        &["shell", "svc", "power", "stayon", "true"][..],
        &[
            "shell",
            "settings",
            "put",
            "system",
            "screen_off_timeout",
            "2147483647",
        ],
        &["shell", "input", "keyevent", "KEYCODE_WAKEUP"],
        &["shell", "wm", "dismiss-keyguard"],
    ] {
        shell(command)?;
    }
    Ok(())
}

fn boot(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("avd boot").string("name", "").parse(arguments)?;
    if Command::new("pgrep")
        .args(["-f", "vphone-cli boot"])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        return Err("the vphone is running; stop it first (vphone-cli vm stop vphone): the Mac cannot run both".into());
    }
    if booted() {
        println!("the emulator is already running");
        return prepare_screen();
    }
    let emulator = sdk().join("emulator/emulator");
    let mut name = flags.get("name");
    if name.is_empty() {
        name = environment("ASTRAL_AVD");
    }
    if name.is_empty() {
        name = repo::output(
            &repo::path(&[]),
            &repo::text(&emulator),
            &strings(&["-list-avds"]),
        )?
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or("no AVD exists; create one with the Android SDK first")?;
    }
    let log = PathBuf::from(environment("HOME")).join(".cache/astral/avd.log");
    std::fs::create_dir_all(log.parent().unwrap_or(std::path::Path::new("/")))
        .map_err(|error| error.to_string())?;
    let file = std::fs::File::create(&log).map_err(|error| error.to_string())?;
    use std::os::unix::process::CommandExt;
    Command::new(&emulator)
        .args(["-avd", &name, "-no-snapshot-save", "-no-boot-anim"])
        .stdout(file.try_clone().map_err(|error| error.to_string())?)
        .stderr(file)
        .process_group(0)
        .spawn()
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(240);
    while !booted() {
        if Instant::now() > deadline {
            return Err(format!(
                "the emulator did not finish booting in 4 minutes; read {}",
                log.display()
            ));
        }
        std::thread::sleep(Duration::from_secs(3));
    }
    let _ = shell(&["root"]);
    std::thread::sleep(Duration::from_secs(2));
    prepare_screen()?;
    println!("{name} booted; log in {}", log.display());
    Ok(())
}

fn top_activity() -> String {
    output(&["shell", "dumpsys", "activity", "activities"])
        .unwrap_or_default()
        .lines()
        .find(|line| line.contains("topResumedActivity"))
        .unwrap_or_default()
        .to_owned()
}

fn launch() -> Result<(), String> {
    let _ = shell(&["shell", "am", "force-stop", DISCORD]);
    std::thread::sleep(Duration::from_secs(1));
    shell(&[
        "shell",
        "monkey",
        "-p",
        DISCORD,
        "-c",
        "android.intent.category.LAUNCHER",
        "1",
    ])?;
    let deadline = Instant::now() + Duration::from_secs(40);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_secs(2));
        if top_activity().contains(ACTIVITY) {
            std::thread::sleep(Duration::from_secs(15));
            println!("Discord runs on Phoenix");
            return Ok(());
        }
    }
    Err(format!(
        "Discord did not open on Phoenix; it shows {}",
        top_activity().trim()
    ))
}

fn status() -> Result<(), String> {
    if !booted() {
        return Err("the emulator is not running; run astral avd boot".into());
    }
    if let Some(line) = output(&["shell", "dumpsys", "package", DISCORD])
        .unwrap_or_default()
        .lines()
        .find(|line| line.contains("versionName="))
    {
        println!(
            "Discord {}",
            line.trim().trim_start_matches("versionName=").trim()
        );
    }
    let client = root(&format!("cat {PHOENIX_PREFERENCE}")).unwrap_or_default();
    if let Some(captures) = Regex::new(r#"name="launch_client">([a-z]+)<"#)
        .expect("pattern")
        .captures(&client)
    {
        println!("launch client: {}", &captures[1]);
    }
    for line in root(&format!("{VECTOR} modules ls"))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains(MODULE))
    {
        println!(
            "module: {}",
            line.split_whitespace().collect::<Vec<_>>().join(" ")
        );
    }
    println!("screen: {}", top_activity().trim());
    Ok(())
}

struct Node {
    text: String,
    resource: String,
    description: String,
    bounds: String,
}

fn unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#10;", "\n")
        .replace("&amp;", "&")
}

fn nodes() -> Result<Vec<Node>, String> {
    output(&["shell", "uiautomator", "dump", "/sdcard/astral-ui.xml"])?;
    let dump = output(&["shell", "cat", "/sdcard/astral-ui.xml"])?;
    let tag = Regex::new(r"<node\b[^>]*>").expect("pattern");
    let attribute = |node: &str, name: &str| {
        Regex::new(&format!(r#"\b{}="([^"]*)""#, regex::escape(name)))
            .expect("pattern")
            .captures(node)
            .map(|captures| unescape(&captures[1]))
            .unwrap_or_default()
    };
    Ok(tag
        .find_iter(&dump)
        .map(|found| {
            let node = found.as_str();
            Node {
                text: attribute(node, "text"),
                resource: attribute(node, "resource-id"),
                description: attribute(node, "content-desc"),
                bounds: attribute(node, "bounds"),
            }
        })
        .collect())
}

fn find(matches: impl Fn(&Node) -> bool) -> Result<Option<Node>, String> {
    Ok(nodes()?.into_iter().find(|node| matches(node)))
}

fn tap_node(node: &Node) -> Result<(), String> {
    let values: Vec<i64> = Regex::new(r"\d+")
        .expect("pattern")
        .find_iter(&node.bounds)
        .filter_map(|value| value.as_str().parse().ok())
        .collect();
    let [left, top, right, bottom] = values.as_slice() else {
        return Err(format!("the view has no usable bounds: {}", node.bounds));
    };
    if right <= left || bottom <= top {
        return Err(format!("the view has no usable bounds: {}", node.bounds));
    }
    shell(&[
        "shell",
        "input",
        "tap",
        &((left + right) / 2).to_string(),
        &((top + bottom) / 2).to_string(),
    ])?;
    std::thread::sleep(Duration::from_secs(2));
    Ok(())
}

fn tap(target: &str) -> Result<(), String> {
    let node =
        find(|node| node.text == target || node.resource == target || node.description == target)?
            .ok_or_else(|| format!("no view shows {target:?}"))?;
    tap_node(&node)
}

fn settings(row: &str) -> Result<(), String> {
    if !top_activity().contains(ACTIVITY) {
        launch()?;
    }
    for _ in 0..4 {
        if find(|node| node.resource == "settings-phone" || node.text == "Account Settings")?
            .is_some_and(|node| !node.bounds.is_empty())
        {
            break;
        }
        if let Some(profile) = find(|node| node.text == "Settings" && node.resource.is_empty())? {
            tap_node(&profile)?;
            continue;
        }
        if let Some(you) = find(|node| {
            node.resource.starts_with("you-bar") || node.description.contains("User Settings")
        })? {
            tap_node(&you)?;
            continue;
        }
        let _ = shell(&["shell", "input", "keyevent", "KEYCODE_BACK"]);
        std::thread::sleep(Duration::from_secs(2));
    }
    if row.is_empty() {
        Ok(())
    } else {
        tap(&format!("settings.row.astral.{row}"))
    }
}

fn logs(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("avd logs")
        .string("since", "60")
        .parse(arguments)?;
    let seconds: i64 = flags
        .get("since")
        .parse()
        .map_err(|_| "--since takes a number")?;
    let start = repo::output(
        &repo::path(&[]),
        "date",
        &strings(&["-v", &format!("-{seconds}S"), "+%m-%d %H:%M:%S.000"]),
    )?;
    print!(
        "{}",
        output(&["logcat", "-d", "-T", start.trim(), "-s", "astral:*"])?
    );
    Ok(())
}

fn open(link: &str) -> Result<(), String> {
    let link = link.strip_prefix("discord://-/").map_or_else(
        || link.to_owned(),
        |rest| format!("https://discord.com/{rest}"),
    );
    shell(&[
        "shell",
        "am",
        "start",
        "-a",
        "android.intent.action.VIEW",
        "-d",
        &link,
        DISCORD,
    ])?;
    std::thread::sleep(Duration::from_secs(3));
    Ok(())
}

fn deploy(arguments: &[String]) -> Result<(), String> {
    let mut build = strings(&["build"]);
    if !arguments
        .iter()
        .any(|argument| argument.starts_with("--abi"))
    {
        let abi = repo::output(
            &repo::path(&[]),
            "adb",
            &strings(&["shell", "getprop", "ro.product.cpu.abi"]),
        )
        .map(|abi| abi.trim().to_owned())
        .unwrap_or_default();
        if !abi.is_empty() {
            build.extend([String::from("--abi"), abi]);
        }
    }
    build.extend(arguments.iter().cloned());
    android::run(&build)?;
    let module = repo::text(&repo::path(&[
        "packages",
        "android",
        "build",
        "astral",
        "astral-module.apk",
    ]));
    shell(&["install", "-r", &module])?;
    for command in [
        format!("{VECTOR} modules enable {MODULE}"),
        format!("{VECTOR} scope set {MODULE} {DISCORD}/0"),
    ] {
        root(&command)?;
    }
    launch()
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let rest = arguments.get(1..).unwrap_or_default();
    match (arguments.first().map(String::as_str), rest) {
        (Some("boot"), _) => boot(rest),
        (Some("stop"), _) => shell(&["emu", "kill"]),
        (Some("status"), _) => status(),
        (Some("discord"), _) => {
            let inputs = android::apk_inputs(
                &Flags::new("avd discord")
                    .list("apk")
                    .parse(rest)?
                    .all("apk"),
            )?;
            if inputs.is_empty() {
                return Err("pass --apk for the base APK and each split".into());
            }
            let mut install = vec![
                "install-multiple".to_owned(),
                "-r".to_owned(),
                "-d".to_owned(),
            ];
            install.extend(inputs.iter().map(|input| repo::text(input)));
            shell(&install.iter().map(String::as_str).collect::<Vec<_>>())
        }
        (Some("deploy"), _) => deploy(rest),
        (Some("launch"), _) => launch(),
        (Some("screenshot"), [file]) => {
            let data = repo::output_bytes(
                &repo::path(&[]),
                &adb_binary(),
                &adb_arguments(&["exec-out", "screencap", "-p"]),
            )?;
            std::fs::write(file, data).map_err(|error| error.to_string())
        }
        (Some("open"), _) => open(&crate::vphone::discord_link(rest)?),
        (Some("tap"), [target]) => tap(target),
        (Some("settings"), _) => settings(rest.first().map_or("", String::as_str)),
        (Some("logs"), _) => logs(rest),
        _ => Err(USAGE.into()),
    }
}
