use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command, Stdio};

use serde_json::Value;

use crate::flags::{Flags, environment};

const USER: &str = "mobile";
const PASSWORD: &str = "alpine";

pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

pub fn address(name: &str) -> Result<(String, String), String> {
    let configured = environment("VPHONE_SSH");
    if !configured.is_empty() {
        let (host, port) = configured.rsplit_once(':').unwrap_or((&configured, "22"));
        return Ok((host.to_owned(), port.to_owned()));
    }
    let raw = Command::new("vphone-cli")
        .args(["vm", "leases", "--json"])
        .output()
        .map_err(|error| format!("vphone-cli vm leases: {error}"))?;
    let leases: Vec<Value> =
        serde_json::from_slice(&raw.stdout).map_err(|error| error.to_string())?;
    leases
        .iter()
        .find(|lease| {
            lease.get("machine").and_then(Value::as_str) == Some(name)
                && lease
                    .get("address")
                    .and_then(Value::as_str)
                    .is_some_and(|address| !address.is_empty())
        })
        .and_then(|lease| lease.get("address").and_then(Value::as_str))
        .map(|address| (address.to_owned(), "22".to_owned()))
        .ok_or_else(|| {
            format!("{name} has no DHCP lease yet; is it booted? (or set VPHONE_SSH=host:port)")
        })
}

fn askpass() -> Result<std::path::PathBuf, String> {
    let path = std::env::temp_dir().join(format!("astral-askpass-{}", std::process::id()));
    if !path.exists() {
        let mut file = std::fs::File::create(&path).map_err(|error| error.to_string())?;
        writeln!(file, "#!/bin/sh\necho {PASSWORD}").map_err(|error| error.to_string())?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    Ok(path)
}

fn ssh(name: &str) -> Result<Command, String> {
    let (host, port) = address(name)?;
    let mut command = Command::new("ssh");
    command
        .env("SSH_ASKPASS", askpass()?)
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("DISPLAY", ":0")
        .stdin(Stdio::null())
        .args([
            "-p",
            &port,
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "LogLevel=ERROR",
        ])
        .args([
            "-o",
            "PubkeyAuthentication=no",
            "-o",
            "PreferredAuthentications=password,keyboard-interactive",
            "-o",
            "ConnectTimeout=10",
        ])
        .arg(format!("{USER}@{host}"));
    Ok(command)
}

pub fn run(name: &str, command: &str, root: bool) -> Result<String, String> {
    let command = if root {
        format!(
            "echo {PASSWORD} | sudo -S -p '' sh -c {}",
            shell_quote(command)
        )
    } else {
        command.to_owned()
    };
    let mut last = String::new();
    for attempt in 0..10 {
        let result = ssh(name)?
            .arg(&command)
            .output()
            .map_err(|error| error.to_string())?;
        let output = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        if result.status.code() == Some(255) && attempt < 9 {
            last = output;
            std::thread::sleep(std::time::Duration::from_secs(2));
            continue;
        }
        return if result.status.success() {
            Ok(output)
        } else {
            Err(format!("{}: {}", result.status, output.trim()))
        };
    }
    Err(format!(
        "ssh {USER}@vphone: {} (is owngoal-bootstrap-vphone installed?)",
        last.trim()
    ))
}

pub struct Forward(Child);

impl Drop for Forward {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn forward(name: &str, port: &str) -> Result<Forward, String> {
    let child = ssh(name)?
        .args([
            "-N",
            "-o",
            "ExitOnForwardFailure=yes",
            "-L",
            &format!("127.0.0.1:{port}:127.0.0.1:{port}"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(Forward(child))
}

pub fn command(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("vphone ssh")
        .string("name", "vphone")
        .boolean("root")
        .parse(arguments)?;
    if flags.rest.is_empty() {
        return Err("usage: astral vphone ssh [--root] [--name vphone] <command>".into());
    }
    let result = run(
        &flags.get("name"),
        &flags.rest.join(" "),
        flags.flag("root"),
    );
    match &result {
        Ok(output) => print!("{output}"),
        Err(error) => print!("{error}"),
    }
    result.map(|_| ())
}
