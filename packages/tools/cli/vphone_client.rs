use std::io::Read;
use std::time::Duration;

use serde_json::{Value, json};
use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, connect};

use crate::flags::environment;

pub const DISCORD_BUNDLE_ID: &str = "com.hammerandchisel.discord";

pub struct Client {
    pub address: String,
    pub token: String,
}

pub fn token_path() -> std::path::PathBuf {
    std::path::PathBuf::from(environment("HOME")).join(".config/astral/vphone-token")
}

impl Client {
    pub fn new() -> Result<Client, String> {
        let address = Some(environment("VPHONE_API"))
            .filter(|address| !address.is_empty())
            .unwrap_or_else(|| "127.0.0.1:8765".into());
        let mut token = environment("VPHONE_API_TOKEN");
        if token.is_empty() {
            token = std::fs::read_to_string(token_path())
                .map_err(|_| "no vphone API token; run astral vphone boot")?
                .trim()
                .to_owned();
        }
        Ok(Client { address, token })
    }

    pub fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let mut request = format!("ws://{}/v1/events", self.address)
            .into_client_request()
            .map_err(|error| error.to_string())?;
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", self.token)
                .parse()
                .map_err(|_| "invalid token")?,
        );
        let (mut socket, _) =
            connect(request).map_err(|error| format!("vphone API at {}: {error}", self.address))?;
        let params = if params.is_null() { json!({}) } else { params };
        socket
            .send(Message::text(
                json!({"type": "request", "id": 1, "method": method, "params": params}).to_string(),
            ))
            .map_err(|error| error.to_string())?;
        loop {
            let message = socket.read().map_err(|error| error.to_string())?;
            let Message::Text(text) = message else {
                continue;
            };
            let Ok(response) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if response.get("type").and_then(Value::as_str) != Some("response")
                || response.get("id").and_then(Value::as_i64) != Some(1)
            {
                continue;
            }
            let _ = socket.close(None);
            if let Some(error) = response.get("error").filter(|error| !error.is_null()) {
                let mut message = format!(
                    "{method}: {}",
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                );
                if let Some(hint) = error
                    .get("hint")
                    .and_then(Value::as_str)
                    .filter(|hint| !hint.is_empty())
                {
                    message.push_str(&format!(" ({hint})"));
                }
                return Err(message);
            }
            return Ok(response.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    pub fn try_call(&self, method: &str, params: Value) {
        let _ = self.call(method, params);
    }

    fn content_url(&self, path: &str) -> String {
        let encoded: String = path
            .bytes()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                    (byte as char).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect();
        format!("http://{}/v1/files/content?path={encoded}", self.address)
    }

    fn agent(&self) -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(1800))
            .build()
    }

    pub fn push(&self, local: &std::path::Path, remote: &str) -> Result<(), String> {
        let file =
            std::fs::File::open(local).map_err(|error| format!("{}: {error}", local.display()))?;
        match self
            .agent()
            .put(&self.content_url(remote))
            .set("Authorization", &format!("Bearer {}", self.token))
            .send(file)
        {
            Ok(_) => Ok(()),
            Err(ureq::Error::Status(_, response)) => Err(format!(
                "upload {remote}: {}",
                response.into_string().unwrap_or_default().trim()
            )),
            Err(error) => Err(format!("upload {remote}: {error}")),
        }
    }

    pub fn pull(&self, remote: &str, local: &std::path::Path) -> Result<(), String> {
        match self
            .agent()
            .get(&self.content_url(remote))
            .set("Authorization", &format!("Bearer {}", self.token))
            .call()
        {
            Ok(response) => {
                let mut bytes = Vec::new();
                response
                    .into_reader()
                    .read_to_end(&mut bytes)
                    .map_err(|error| error.to_string())?;
                std::fs::write(local, bytes).map_err(|error| error.to_string())
            }
            Err(ureq::Error::Status(_, response)) => Err(format!(
                "download {remote}: {}",
                response.into_string().unwrap_or_default().trim()
            )),
            Err(error) => Err(format!("download {remote}: {error}")),
        }
    }

    pub fn install(&self, ipa: &std::path::Path) -> Result<(), String> {
        let remote = "/var/root/Library/Caches/astral-install.ipa";
        self.push(ipa, remote)?;
        let result = self.call("apps.install", json!({"path": remote}));
        self.try_call("files.remove", json!({"path": remote}));
        result.map(|_| ())
    }

    pub fn list(&self, directory: &str) -> Result<Vec<Value>, String> {
        let listing = self.call("files.list", json!({"path": directory, "limit": 1000}))?;
        Ok(listing
            .get("entries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    pub fn names(&self, directory: &str) -> Vec<String> {
        self.list(directory)
            .unwrap_or_default()
            .iter()
            .filter_map(|entry| entry.get("name").and_then(Value::as_str).map(str::to_owned))
            .collect()
    }

    pub fn exists(&self, directory: &str, name: &str) -> bool {
        self.names(directory).iter().any(|entry| entry == name)
    }

    pub fn app_info(&self, bundle: &str) -> Result<Value, String> {
        self.call("apps.info", json!({"bundle_id": bundle}))
    }

    pub fn discord_data_path(&self) -> Result<String, String> {
        let data = self
            .app_info(DISCORD_BUNDLE_ID)?
            .get("data_path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if data.is_empty() {
            Err("Discord has no data container on the vphone".into())
        } else {
            Ok(data)
        }
    }

    pub fn discord_running(&self) -> bool {
        self.call("processes.list", json!({"filter": "Discord"}))
            .ok()
            .and_then(|listing| listing.get("processes").and_then(Value::as_array).cloned())
            .is_some_and(|processes| {
                processes.iter().any(|process| {
                    process.get("bundle_id").and_then(Value::as_str) == Some(DISCORD_BUNDLE_ID)
                })
            })
    }

    pub fn launch_when_stopped(&self) -> Result<(), String> {
        for _ in 0..20 {
            if !self.discord_running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        self.try_call("screen.unlock", json!({"timeout_ms": 10000}));
        let mut last = Ok(Value::Null);
        for _ in 0..10 {
            last = self.call("apps.launch", json!({"bundle_id": DISCORD_BUNDLE_ID}));
            std::thread::sleep(Duration::from_millis(700));
            if last.is_ok() && self.discord_running() {
                return Ok(());
            }
        }
        last?;
        Err("Discord did not stay running; read the newest Discord report in /var/mobile/Library/Logs/CrashReporter".into())
    }

    pub fn restart_discord(&self) -> Result<(), String> {
        self.try_call("apps.terminate", json!({"bundle_id": DISCORD_BUNDLE_ID}));
        self.launch_when_stopped()
    }
}
