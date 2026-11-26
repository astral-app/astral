use std::path::PathBuf;

use crate::flags::environment;
use crate::vphone_client::Client;
use crate::vphone_ssh::{self, shell_quote};

const FILES: [(&str, &str, &str); 4] = [
    (
        "keychain.plist",
        "Documents/astral/session.plist",
        "Documents/astral/session-import.plist",
    ),
    (
        "preferences.plist",
        "Library/Preferences/com.hammerandchisel.discord.plist",
        "",
    ),
    (
        "wasm_storage.plist",
        "Library/Preferences/wasm_storage.plist",
        "",
    ),
    (
        "wasm_device.plist",
        "Library/Preferences/wasm_device.plist",
        "",
    ),
];

pub fn directory() -> PathBuf {
    PathBuf::from(environment("HOME")).join(".config/astral/vphone-session")
}

pub fn save(client: &Client) -> Result<usize, String> {
    let data = client.discord_data_path()?;
    let directory = directory();
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let mut saved = 0;
    for (local, remote, _) in FILES {
        let staging = directory.join(format!(".{local}"));
        if client.pull(&format!("{data}/{remote}"), &staging).is_err() {
            let _ = std::fs::remove_file(&staging);
            continue;
        }
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o600));
        std::fs::rename(&staging, directory.join(local)).map_err(|error| error.to_string())?;
        saved += 1;
    }
    if !directory.join("keychain.plist").exists() {
        return Err("no login saved yet; open Discord on the vphone once with this astral build, then send it to the background".into());
    }
    Ok(saved)
}

pub fn restore(client: &Client, name: &str) -> Result<usize, String> {
    let directory = directory();
    if !directory.join("keychain.plist").exists() {
        return Err(format!(
            "no saved login in {}; run astral vphone session save first",
            directory.display()
        ));
    }
    let data = client.discord_data_path()?;
    let mut restored = 0;
    let mut written = Vec::new();
    for (local, remote, push_to) in FILES {
        let source = directory.join(local);
        if !source.exists() {
            continue;
        }
        let target = format!(
            "{data}/{}",
            if push_to.is_empty() { remote } else { push_to }
        );
        client
            .push(&source, &target)
            .map_err(|error| format!("restore {local}: {error}"))?;
        let parent = target
            .rsplit_once('/')
            .map(|(parent, _)| parent.to_owned())
            .unwrap_or_default();
        written.push(shell_quote(&target));
        written.push(shell_quote(&parent));
        restored += 1;
    }
    vphone_ssh::run(
        name,
        &format!("chown mobile:mobile {}", written.join(" ")),
        true,
    )
    .map_err(|error| format!("hand the restored files to mobile: {error}"))?;
    Ok(restored)
}

pub fn command(client: &Client, arguments: &[String]) -> Result<(), String> {
    match arguments.first().map(String::as_str) {
        Some("save") if arguments.len() == 1 => {
            let saved = save(client)?;
            println!("saved {saved} session files to {}", directory().display());
            Ok(())
        }
        Some("restore") if arguments.len() == 1 => {
            let restored = restore(client, "vphone")?;
            println!(
                "restored {restored} session files; astral loads the login on Discord's next launch"
            );
            Ok(())
        }
        _ => Err("usage: astral vphone session <save|restore>".into()),
    }
}
