use std::path::{Path, PathBuf};
use std::time::Duration;

use regex::Regex;
use serde_json::Value;

use crate::files;
use crate::flags::{Flags, environment};
use crate::repo::{self, strings};

struct Pilot {
    binary: String,
    device: String,
}

fn device_flags(name: &str, arguments: &[String]) -> Result<Flags, String> {
    Flags::new(name)
        .string("device", &environment("DEVICE"))
        .parse(arguments)
}

impl Pilot {
    fn new(device: &str) -> Result<Pilot, String> {
        if device.is_empty() {
            return Err("set --device or DEVICE to the test device UDID".into());
        }
        let binary = Some(environment("JB_PILOT"))
            .filter(|binary| !binary.is_empty())
            .unwrap_or_else(|| format!("{}/go/bin/jb-p1lot", environment("HOME")));
        Ok(Pilot {
            binary,
            device: device.to_owned(),
        })
    }

    fn call(&self, arguments: &[&str]) -> Result<String, String> {
        let mut all = strings(&[arguments[0], "--device", &self.device]);
        all.extend(strings(&arguments[1..]));
        repo::output(&repo::path(&[]), &self.binary, &all)
    }

    fn shell(&self, command: &str) -> Result<String, String> {
        let raw = self.call(&["shell_exec", "--command", command])?;
        let result: Value = serde_json::from_str(&raw)
            .map_err(|_| format!("unexpected jb-p1lot output: {}", raw.trim()))?;
        Ok(result
            .get("output")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned())
    }

    fn push(&self, local: &Path, remote: &str) -> Result<(), String> {
        self.call(&[
            "file_transfer",
            "--direction",
            "host_to_device",
            "--source",
            &repo::text(local),
            "--destination",
            remote,
        ])
        .map(|_| ())
    }

    fn pull(&self, remote: &str, local: &Path) -> Result<(), String> {
        self.call(&[
            "file_transfer",
            "--direction",
            "device_to_host",
            "--source",
            remote,
            "--destination",
            &repo::text(local),
        ])
        .map(|_| ())
    }

    fn tap(&self, x: i64, y: i64, settle: Duration) {
        let _ = self.call(&[
            "ui_action",
            "--action",
            "tap",
            "--x",
            &x.to_string(),
            "--y",
            &y.to_string(),
        ]);
        std::thread::sleep(settle);
    }

    fn labels(&self) -> Result<Vec<(String, i64, i64)>, String> {
        let raw = self.call(&["ui_snapshot"])?;
        let snapshot: Value = serde_json::from_str(&raw).map_err(|error| error.to_string())?;
        Ok(snapshot
            .get("nodes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|node| {
                let point = node.get("activationPoint");
                let coordinate = |axis: &str| {
                    point
                        .and_then(|point| point.get(axis))
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0) as i64
                };
                (
                    node.get("label")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    coordinate("x"),
                    coordinate("y"),
                )
            })
            .collect())
    }

    fn state_directory(&self) -> Result<String, String> {
        let directory = self
            .shell("ls -d /var/mobile/Containers/Data/Application/*/Documents/astral | head -1")?
            .trim()
            .to_owned();
        if directory.is_empty() {
            Err("no astral state directory on the device".into())
        } else {
            Ok(directory)
        }
    }
}

pub fn build_package() -> Result<(), String> {
    let theos = Some(environment("THEOS"))
        .filter(|theos| !theos.is_empty())
        .unwrap_or_else(|| format!("{}/theos", environment("HOME")));
    let path = format!(
        "/opt/homebrew/opt/make/libexec/gnubin:/opt/homebrew/bin:{}",
        environment("PATH")
    );
    repo::run_with_env(
        &repo::path(&["packages", "ios"]),
        &[("THEOS", &theos), ("PATH", &path)],
        "make",
        &strings(&["package", "FINALPACKAGE=1"]),
    )
}

pub fn newest_deb() -> Result<PathBuf, String> {
    files::newest(&repo::path(&["packages", "ios", "packages", "*.deb"]))
        .ok_or_else(|| "no astral .deb; build packages/ios first".into())
}

pub fn deploy(arguments: &[String]) -> Result<(), String> {
    let pilot = Pilot::new(&device_flags("deploy", arguments)?.get("device"))?;
    build_package()?;
    let deb = newest_deb()?;
    pilot.push(&deb, "/var/mobile/Documents/astral.deb")?;
    pilot.shell("dpkg -i /var/mobile/Documents/astral.deb >/dev/null 2>&1; killall -9 Discord 2>/dev/null; sleep 1; /var/jb/usr/bin/uiopen --bundleid com.hammerandchisel.discord")?;
    println!(
        "installed {}",
        deb.file_name().unwrap_or_default().to_string_lossy()
    );
    Ok(())
}

pub fn open_settings(arguments: &[String]) -> Result<(), String> {
    let flags = device_flags("open-settings", arguments)?;
    let target = if flags.rest.is_empty() {
        "General".to_owned()
    } else {
        flags.rest.join(" ")
    };
    let pilot = Pilot::new(&flags.get("device"))?;
    let status = Regex::new(r", (Idle|Online|Do Not Disturb|Invisible)$").expect("pattern");
    let find = |nodes: &[(String, i64, i64)], label: &str| {
        nodes.iter().rev().find(|node| node.0 == label).cloned()
    };
    for _ in 0..6 {
        let nodes = pilot.labels()?;
        if find(&nodes, "Account Settings").is_some()
            && let Some(row) = find(&nodes, &target)
        {
            pilot.tap(187, row.2, Duration::from_secs(3));
            for (label, x, y) in pilot.labels()? {
                println!("{} {x} {y}", label.replace(' ', "_"));
            }
            return Ok(());
        }
        if let Some(button) = find(&nodes, "Settings") {
            pilot.tap(button.1, button.2, Duration::from_secs(3));
            continue;
        }
        if let Some(node) = nodes.iter().find(|node| status.is_match(&node.0)) {
            pilot.tap(node.1, node.2, Duration::from_secs(3));
            continue;
        }
        pilot.tap(28, 48, Duration::from_secs(2));
    }
    Err(format!("could not reach the {target} row"))
}

pub fn wire_dump(arguments: &[String]) -> Result<(), String> {
    let flags = device_flags("wire-dump", arguments)?;
    let [destination] = flags.rest.as_slice() else {
        return Err("usage: astral wire-dump [--device <udid>] <output directory>".into());
    };
    let destination = PathBuf::from(destination);
    let pilot = Pilot::new(&flags.get("device"))?;
    std::fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    let tool = crate::vphone::wire_dump_tool(&destination)?;
    let state = pilot.state_directory()?;
    let mut batches = Vec::new();
    for name in pilot.shell(&format!("ls {state}/wire"))?.split_whitespace() {
        let local = destination.join(name);
        pilot.pull(&format!("{state}/wire/{name}"), &local)?;
        batches.push(repo::text(&local));
    }
    if batches.is_empty() {
        return Err(
            "no captured batches; create capture-wire in astral's state directory and relaunch"
                .into(),
        );
    }
    let dump = repo::output(&repo::path(&[]), &repo::text(&tool), &batches)?;
    let jsonl = destination.join("wire.jsonl");
    std::fs::write(&jsonl, &dump).map_err(|error| error.to_string())?;
    println!(
        "{} batches in {}",
        dump.matches('\n').count(),
        jsonl.display()
    );
    Ok(())
}
