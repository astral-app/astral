use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use crate::files;
use crate::flags::environment;
use crate::repo;
use crate::signing;
use crate::vphone_client::Client;
use crate::vphone_ssh;

const USAGE: &str = "usage: astral dev <command>
  pair <code>            save the pairing code shown under Developer mode (the vphone needs none)
  status                 the device's astral version and plugins
  push <plugin folder>   install a plugin and hot reload it
  enable|disable <id>    turn an installed plugin on or off
  watch <plugin folder>  push the plugin every time a file in it changes
  eval <plugin id> <lua> run Lua inside a running plugin and print the result
  logs [-f]              print astral's log, following it with -f
  gateway <json file>    deliver a gateway payload to Discord locally, as if the gateway sent it (needs Staff DevTools)
  restart                soft restart: reload plugins, the theme and icons in place without relaunching Discord
  emoji use <id>         switch emoji pack (discord for Discord's own)
  icons use <id>         switch icon pack (discord for Discord's own) and refresh icons in place
  theme list             the installed themes and the active one
  theme use <id>         switch to a theme; on-screen views recolour at once
  theme import <file>    convert an Unbound theme (JSON) into an astral theme and select it
  backup export [file]   write the device's settings backup (TOML) to a file or stdout
  backup import <file>   restore a settings backup on the device
Needs Developer mode on. DEVICE=android (or android:<serial>) forwards the port with adb; another DEVICE value forwards it over USB; without it, over SSH to the vphone.";

pub struct Dev {
    base: String,
    port: String,
    pairing: String,
    keep: Option<vphone_ssh::Forward>,
}

fn pairing_path() -> PathBuf {
    PathBuf::from(environment("HOME")).join(".config/astral/dev-pairing")
}

fn stored_pairing() -> String {
    let configured = environment("ASTRAL_DEV_PAIRING");
    if !configured.is_empty() {
        return configured.trim().to_uppercase();
    }
    std::fs::read_to_string(pairing_path())
        .unwrap_or_default()
        .trim()
        .to_uppercase()
}

fn save_pairing(code: &str) -> Result<(), String> {
    let code = code.trim().replace('-', "").to_uppercase();
    if code.len() != 8 {
        return Err("a pairing code has 8 characters; it is shown under Developer mode in astral's settings".into());
    }
    std::fs::create_dir_all(pairing_path().parent().unwrap_or(Path::new("/")))
        .map_err(|error| error.to_string())?;
    std::fs::write(pairing_path(), format!("{code}\n")).map_err(|error| error.to_string())
}

fn vphone_pairing() -> Result<String, String> {
    let client = Client::new()?;
    let data = client.discord_data_path()?;
    let output = vphone_ssh::run(
        "vphone",
        &format!(
            "cat {}",
            vphone_ssh::shell_quote(&format!("{data}/Documents/astral/dev-pairing-code"))
        ),
        true,
    )
    .map_err(|error| format!("read the pairing code: {error}"))?;
    let code = output.trim().to_uppercase();
    if code.len() != 8 || !code.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err("the pairing code is not ready yet".into());
    }
    Ok(code)
}

fn android_serial(device: &str) -> Option<String> {
    if device == "android" {
        return Some(String::new());
    }
    device.strip_prefix("android:").map(str::to_owned)
}

fn adb(serial: &str, arguments: &[&str]) -> Command {
    let mut command = Command::new("adb");
    if !serial.is_empty() {
        command.args(["-s", serial]);
    }
    command.args(arguments);
    command
}

fn android_pairing(serial: &str) -> String {
    for arguments in [
        &[
            "shell",
            "su",
            "0",
            "cat",
            "/data/data/com.discord/files/astral/dev-pairing-code",
        ][..],
        &[
            "shell",
            "cat",
            "/data/data/com.discord/files/astral/dev-pairing-code",
        ],
    ] {
        if let Ok(output) = adb(serial, arguments).output() {
            let code = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if code.len() == 8 {
                return code;
            }
        }
    }
    String::new()
}

impl Default for Dev {
    fn default() -> Dev {
        Dev::new()
    }
}

impl Dev {
    pub fn new() -> Dev {
        let port = Some(environment("ASTRAL_DEV_PORT"))
            .filter(|port| !port.is_empty())
            .unwrap_or_else(|| "9324".into());
        Dev {
            base: format!("http://127.0.0.1:{port}"),
            port,
            pairing: stored_pairing(),
            keep: None,
        }
    }

    fn reachable(&self) -> bool {
        match ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(1))
            .build()
            .get(&format!("{}/status", self.base))
            .call()
        {
            Ok(_) => true,
            Err(ureq::Error::Status(code, _)) => code < 500,
            Err(_) => false,
        }
    }

    fn wait_reachable(&self, attempts: usize) -> bool {
        for _ in 0..attempts {
            if self.reachable() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        false
    }

    pub fn forward(&mut self) -> Result<(), String> {
        let device = environment("DEVICE");
        if let Some(serial) = android_serial(&device) {
            if self.pairing.is_empty() {
                self.pairing = android_pairing(&serial);
            }
            if !adb(
                &serial,
                &[
                    "forward",
                    &format!("tcp:{}", self.port),
                    &format!("tcp:{}", self.port),
                ],
            )
            .status()
            .is_ok_and(|status| status.success())
            {
                return Err("adb forward failed".into());
            }
            return if self.reachable() {
                Ok(())
            } else {
                Err("astral's development server is not reachable on Android; turn on Developer mode and relaunch Discord".into())
            };
        }
        if device.is_empty()
            && self.pairing.is_empty()
            && let Ok(code) = vphone_pairing()
        {
            self.pairing = code;
        }
        if self.reachable() {
            return Ok(());
        }
        if device.is_empty() {
            let forward = vphone_ssh::forward("vphone", &self.port).map_err(|error| {
                format!("set DEVICE for a USB device, or boot the vphone: {error}")
            })?;
            self.keep = Some(forward);
            return if self.wait_reachable(20) {
                Ok(())
            } else {
                Err("astral's development server is not reachable on the vphone; turn on Developer mode and relaunch Discord".into())
            };
        }
        Command::new("iproxy")
            .args([&self.port, &self.port, "-u", &device])
            .spawn()
            .map_err(|error| error.to_string())?;
        if self.wait_reachable(10) {
            Ok(())
        } else {
            Err("astral's development server is not reachable; turn on Developer mode".into())
        }
    }

    pub fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<String, String> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(20))
            .build();
        let mut request = agent.request(method, &format!("{}{path}", self.base));
        if !self.pairing.is_empty() {
            request = request.set("X-Astral-Pairing", &self.pairing);
        }
        let result = match body {
            Some(body) => request
                .set("Content-Type", "application/json")
                .send_bytes(&body),
            None => request.call(),
        };
        let response = match result {
            Ok(response) | Err(ureq::Error::Status(_, response)) => response,
            Err(error) => return Err(error.to_string()),
        };
        let mut text = String::new();
        response
            .into_reader()
            .read_to_string(&mut text)
            .map_err(|error| error.to_string())?;
        Ok(text)
    }

    fn json(&self, method: &str, path: &str, body: &Value) -> Result<String, String> {
        self.request(method, path, Some(body.to_string().into_bytes()))
    }

    pub fn push(&self, folder: &Path) -> Result<(), String> {
        let manifest = signing::read_manifest(folder)?;
        let manifest_text = repo::read(&folder.join("manifest.toml"))?;
        let main = repo::read(&folder.join(&manifest.main))?;
        let mut modules = BTreeMap::new();
        for path in signing::plugin_modules(folder)? {
            if path != manifest.main {
                modules.insert(path.clone(), repo::read(&folder.join(&path))?);
            }
        }
        for translation in files::glob(&folder.join("i18n/*.toml")) {
            modules.insert(
                format!(
                    "i18n/{}",
                    translation
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ),
                repo::read(&translation)?,
            );
        }
        let mut assets = BTreeMap::new();
        files::walk(folder, &|_| false, &mut |file| {
            let extension = file
                .extension()
                .map(|extension| extension.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if ["mp3", "m4a", "wav", "caf", "aac", "png", "webp"].contains(&extension.as_str())
                && let (Ok(relative), Ok(data)) = (file.strip_prefix(folder), std::fs::read(file))
            {
                assets.insert(repo::text(relative), STANDARD.encode(data));
            }
        });
        println!("{}", self.json("PUT", "/plugin", &json!({"manifest": manifest_text, "main": main, "modules": modules, "assets": assets}))?);
        Ok(())
    }

    fn watch(&self, folder: &Path) -> Result<(), String> {
        let latest = |folder: &Path| {
            let mut newest = SystemTime::UNIX_EPOCH;
            files::walk(folder, &|_| false, &mut |file| {
                if let Ok(modified) =
                    std::fs::metadata(file).and_then(|metadata| metadata.modified())
                {
                    newest = newest.max(modified);
                }
            });
            newest
        };
        self.push(folder)?;
        let mut seen = latest(folder);
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let changed = latest(folder);
            if changed > seen {
                seen = changed;
                if let Err(error) = self.push(folder) {
                    eprintln!("{error}");
                }
            }
        }
    }

    fn logs(&self, follow: bool) -> Result<(), String> {
        let mut since = 0;
        loop {
            let reply = self.request("GET", &format!("/logs?since={since}"), None)?;
            let lines: Vec<Value> =
                serde_json::from_str(&reply).map_err(|error| error.to_string())?;
            for entry in lines {
                eprintln!(
                    "{}",
                    entry
                        .get("line")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                );
                since = entry.get("id").and_then(Value::as_i64).unwrap_or(since);
            }
            if !follow {
                return Ok(());
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

fn print_indented(reply: &str) {
    match serde_json::from_str::<Value>(reply) {
        Ok(value) => println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        ),
        Err(_) => println!("{reply}"),
    }
}

pub fn reload_in_place() -> Result<String, String> {
    let mut dev = Dev::new();
    dev.forward()?;
    let reply = dev.json("POST", "/reload", &json!({}))?;
    if !reply.contains("\"theme\"") {
        return Err(reply.trim().to_owned());
    }
    Ok(reply.trim().to_owned())
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let Some(command) = arguments.first().map(String::as_str) else {
        return Err(USAGE.into());
    };
    let rest = &arguments[1..];
    if command == "pair" {
        let [code] = rest else {
            return Err(USAGE.into());
        };
        save_pairing(code)?;
        println!("saved the pairing code to {}", pairing_path().display());
        return Ok(());
    }
    let mut dev = Dev::new();
    dev.forward()?;
    let read = |path: &str| std::fs::read(path).map_err(|error| format!("{path}: {error}"));
    match (command, rest) {
        ("status", _) => print_indented(&dev.request("GET", "/status", None)?),
        ("push", [folder, ..]) => dev.push(Path::new(folder))?,
        ("watch", [folder, ..]) => dev.watch(Path::new(folder))?,
        ("enable" | "disable", [id]) => println!(
            "{}",
            dev.json(
                "POST",
                "/plugin/enabled",
                &json!({"id": id, "enabled": command == "enable"})
            )?
            .trim()
        ),
        ("enable" | "disable", _) => {
            return Err(format!("usage: astral dev {command} <plugin id>"));
        }
        ("eval", [plugin, code, ..]) => {
            let reply = dev.json("POST", "/eval", &json!({"plugin": plugin, "code": code}))?;
            let parsed: Value = serde_json::from_str(&reply).map_err(|error| error.to_string())?;
            println!(
                "{}",
                parsed
                    .get("result")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            );
        }
        ("logs", _) => dev.logs(rest.first().is_some_and(|flag| flag == "-f"))?,
        ("emoji", [use_, id]) if use_ == "use" => {
            println!("{}", dev.json("POST", "/emoji-pack", &json!({"id": id}))?)
        }
        ("icons", [use_, id]) if use_ == "use" => {
            println!("{}", dev.json("POST", "/icon-pack", &json!({"id": id}))?)
        }
        ("gateway", [file]) => println!("{}", dev.request("POST", "/gateway", Some(read(file)?))?),
        ("restart", _) => println!("{}", dev.json("POST", "/reload", &json!({}))?),
        ("theme", [list]) if list == "list" => println!("{}", dev.request("GET", "/themes", None)?),
        ("theme", [use_, id]) if use_ == "use" => {
            println!("{}", dev.json("POST", "/theme", &json!({"id": id}))?)
        }
        ("theme", [import, file, ..]) if import == "import" => println!(
            "{}",
            dev.request("POST", "/unbound-theme", Some(read(file)?))?
        ),
        ("backup", [export, rest @ ..]) if export == "export" => {
            let reply = dev.request("GET", "/backup", None)?;
            let backup = serde_json::from_str::<Value>(&reply)
                .map_err(|error| error.to_string())?
                .get("backup")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            match rest.first() {
                Some(file) => std::fs::write(file, backup).map_err(|error| error.to_string())?,
                None => print!("{backup}"),
            }
        }
        ("backup", [import, file]) if import == "import" => {
            let text = String::from_utf8_lossy(&read(file)?).into_owned();
            println!("{}", dev.json("POST", "/backup", &json!({"backup": text}))?);
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}
