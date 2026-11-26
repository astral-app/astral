use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::device;
use crate::files;
use crate::flags::environment;
use crate::repo;
use crate::vphone_client::{Client, DISCORD_BUNDLE_ID};
use crate::vphone_session;

const CONTAINERS: &str = "/var/containers/Bundle/Application";

pub fn jailbreak_root(client: &Client) -> Result<String, String> {
    client
        .names(CONTAINERS)
        .into_iter()
        .find(|name| name.starts_with(".jbroot-"))
        .map(|name| format!("{CONTAINERS}/{name}"))
        .ok_or_else(|| {
            "no roothide jailbreak root on the vphone; is the Irisin bootstrap installed?".into()
        })
}

fn staged() -> PathBuf {
    repo::path(&["packages", "ios", ".theos", "_", "var", "jb"])
}

fn tweak_files(tweaks: &str, resources: &str) -> Vec<(PathBuf, String)> {
    let libraries = staged().join("Library/MobileSubstrate/DynamicLibraries");
    let mut files = vec![
        (
            libraries.join("astral.dylib"),
            format!("{tweaks}/astral.dylib"),
        ),
        (
            libraries.join("astral.plist"),
            format!("{tweaks}/astral.plist"),
        ),
    ];
    let local = staged().join("usr/lib/astral");
    files::walk(&local, &|_| false, &mut |file| {
        let relative = file
            .strip_prefix(&local)
            .map(repo::text)
            .unwrap_or_default();
        files.push((file.to_path_buf(), format!("{resources}/{relative}")));
    });
    files
}

fn cache_path() -> PathBuf {
    PathBuf::from(environment("HOME")).join(".cache/astral/vphone-deploy.json")
}

fn hash(file: &Path) -> Result<String, String> {
    let data = std::fs::read(file).map_err(|error| format!("{}: {error}", file.display()))?;
    Ok(Sha256::digest(&data)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn strip_private(path: &str) -> String {
    path.strip_prefix("/private").unwrap_or(path).to_owned()
}

fn list_remote(client: &Client, directory: &str, depth: usize, present: &mut BTreeSet<String>) {
    if depth > 6 {
        return;
    }
    for entry in client.list(directory).unwrap_or_default() {
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let full = format!("{directory}/{name}");
        match entry
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "directory" | "dir" => list_remote(client, &full, depth + 1, present),
            "link" => {}
            _ => {
                present.insert(strip_private(&full));
            }
        }
    }
}

fn sync(
    client: &Client,
    files: &[(PathBuf, String)],
    resources: &str,
    tweaks: &str,
) -> Result<(usize, usize, bool), String> {
    let mut cache: BTreeMap<String, String> = std::fs::read(cache_path())
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default();
    let mut present = BTreeSet::new();
    list_remote(client, resources, 0, &mut present);
    for name in client.names(tweaks) {
        present.insert(strip_private(&format!("{tweaks}/{name}")));
    }
    let mut wanted = BTreeSet::new();
    let (mut pushed, mut binary_changed) = (0, false);
    for (local, remote) in files {
        wanted.insert(remote.clone());
        let digest = hash(local)?;
        if cache.get(remote) == Some(&digest) && present.contains(&strip_private(remote)) {
            continue;
        }
        if remote.ends_with(".dylib") {
            client.try_call("files.remove", json!({"path": remote}));
        }
        client.push(local, remote)?;
        cache.insert(remote.clone(), digest);
        pushed += 1;
        if remote.starts_with(&format!("{tweaks}/")) {
            binary_changed = true;
        }
    }
    let mut removed = 0;
    let prefix = format!("{}/", strip_private(resources));
    for remote in present {
        if remote.starts_with(&prefix)
            && !wanted.contains(&remote)
            && client.call("files.remove", json!({"path": remote})).is_ok()
        {
            cache.remove(&remote);
            removed += 1;
        }
    }
    if let Some(parent) = cache_path().parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(cache_path(), serde_json::to_vec(&cache).unwrap_or_default())
        .map_err(|error| error.to_string())?;
    Ok((pushed, removed, binary_changed))
}

pub fn deploy(client: &Client) -> Result<(), String> {
    let started = Instant::now();
    let root = jailbreak_root(client)?;
    let tweaks = format!("{root}/usr/lib/TweakInject");
    let resources = format!("{root}/usr/lib/astral");
    if !client.exists(&format!("{root}/usr/lib"), "TweakLoader.dylib") {
        return Err(
            "ElleKit is not installed on the vphone; see the vphone section of the vphone skill"
                .into(),
        );
    }
    let bundle = client
        .app_info(DISCORD_BUNDLE_ID)
        .map_err(|error| format!("Discord is not installed on the vphone; run astral vphone discord --ipa <decrypted Discord.ipa>: {error}"))?
        .get("bundle_path")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if client.exists(&format!("{bundle}/Frameworks"), "astral.dylib") {
        return Err("the installed Discord has astral built in; reinstall it stock with astral vphone discord --ipa <decrypted Discord.ipa>".into());
    }
    device::build_package()?;
    let files = tweak_files(&tweaks, &resources);
    let (pushed, removed, binary_changed) = sync(client, &files, &resources, &tweaks)?;
    let seconds = || started.elapsed().as_secs_f64();
    if pushed + removed > 0
        && !binary_changed
        && client.discord_running()
        && let Ok(reply) = crate::dev::reload_in_place()
    {
        println!(
            "deployed {} changed resource files and reloaded the theme and plugins in place ({reply}) in {:.1}s",
            pushed + removed,
            seconds()
        );
        return Ok(());
    }
    if pushed + removed == 0 && client.discord_running() {
        println!("nothing changed; Discord keeps running ({:.1}s)", seconds());
        return Ok(());
    }
    if vphone_session::save(client).is_ok() {
        println!("refreshed the saved login");
    }
    client.try_call("apps.terminate", json!({"bundle_id": DISCORD_BUNDLE_ID}));
    client.launch_when_stopped()?;
    println!(
        "deployed astral as an ElleKit tweak ({pushed} of {} files changed, {removed} removed) and relaunched Discord in {:.1}s",
        files.len(),
        seconds()
    );
    Ok(())
}
