use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::pkcs8::DecodePrivateKey;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::flags::{Flags, environment};
use crate::repo;

const SIGNATURE_VERSION: &str = "astral-plugin-v2";

#[derive(Deserialize, Default)]
pub struct Manifest {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub main: String,
}

#[derive(Deserialize, Default)]
pub struct PluginSignature {
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

pub fn read_manifest(folder: &Path) -> Result<Manifest, String> {
    let text = repo::read(&folder.join("manifest.toml"))?;
    let mut manifest: Manifest = toml::from_str(&text)
        .map_err(|error| format!("{}/manifest.toml: {error}", name_of(folder)))?;
    if manifest.main.is_empty() {
        manifest.main = "main.lua".into();
    }
    Ok(manifest)
}

pub fn name_of(folder: &Path) -> String {
    folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn plugin_modules(folder: &Path) -> Result<Vec<String>, String> {
    fn walk(root: &Path, directory: &Path, modules: &mut Vec<String>) -> Result<(), String> {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(directory)
            .map_err(|error| error.to_string())?
            .flatten()
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for entry in entries {
            let relative = entry
                .strip_prefix(root)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if entry.is_dir() {
                if relative != "tests" {
                    walk(root, &entry, modules)?;
                }
            } else if entry
                .extension()
                .is_some_and(|extension| extension == "lua")
            {
                modules.push(relative);
            }
        }
        Ok(())
    }
    let mut modules = Vec::new();
    walk(folder, folder, &mut modules)?;
    Ok(modules)
}

pub fn bytecode_path(source: &str) -> String {
    format!("{}.luac", source.strip_suffix(".lua").unwrap_or(source))
}

pub fn signed_paths(folder: &Path) -> Result<Vec<String>, String> {
    let manifest = read_manifest(folder)?;
    let mut paths =
        std::collections::BTreeSet::from(["manifest.toml".to_owned(), manifest.main.clone()]);
    if let Ok(entries) = std::fs::read_dir(folder.join("i18n")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".toml") {
                paths.insert(format!("i18n/{name}"));
            }
        }
    }
    for module in plugin_modules(folder)? {
        paths.insert(bytecode_path(&module));
        paths.insert(module);
    }
    Ok(paths.into_iter().collect())
}

fn digest(path: &Path) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(Sha256::digest(&data)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub fn signature_message(files: &BTreeMap<String, String>) -> Vec<u8> {
    let mut message = format!("{SIGNATURE_VERSION}\n");
    for (path, sum) in files {
        message.push_str(&format!("{path} {sum}\n"));
    }
    message.into_bytes()
}

pub fn public_key() -> Result<VerifyingKey, String> {
    let text = repo::read(&repo::path(&["keys", "plugin-signing.pub"]))?;
    let bytes = STANDARD
        .decode(text.trim())
        .map_err(|_| "keys/plugin-signing.pub is not a base64 Ed25519 public key")?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "keys/plugin-signing.pub is not a base64 Ed25519 public key")?;
    VerifyingKey::from_bytes(&bytes)
        .map_err(|_| "keys/plugin-signing.pub is not a base64 Ed25519 public key".into())
}

fn private_key(path: &str) -> Result<SigningKey, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("signing key not found: {error}"))?;
    SigningKey::from_pkcs8_pem(&text)
        .map_err(|error| format!("signing key is not an Ed25519 PKCS#8 PEM: {error}"))
}

fn plugin_files(folder: &Path) -> Result<BTreeMap<String, String>, String> {
    signed_paths(folder)?
        .into_iter()
        .map(|path| Ok((path.clone(), digest(&folder.join(&path))?)))
        .collect()
}

unsafe extern "C" fn collect(
    _: *mut astral_core::lua_api::LuaState,
    data: *const core::ffi::c_void,
    size: usize,
    into: *mut core::ffi::c_void,
) -> core::ffi::c_int {
    let into = unsafe { &mut *into.cast::<Vec<u8>>() };
    into.extend_from_slice(unsafe { core::slice::from_raw_parts(data.cast::<u8>(), size) });
    0
}

pub fn compile_lua(chunk: &str, source: &[u8]) -> Result<Vec<u8>, String> {
    use astral_core::lua_api as lua;
    let state = unsafe { lua::luaL_newstate() };
    if state.is_null() {
        return Err("out of memory".into());
    }
    let chunk = std::ffi::CString::new(chunk).map_err(|error| error.to_string())?;
    let mut bytecode = Vec::new();
    let status = unsafe {
        lua::luaL_loadbufferx(
            state,
            source.as_ptr().cast(),
            source.len(),
            chunk.as_ptr(),
            c"t".as_ptr(),
        )
    };
    let result = if status == lua::OK {
        unsafe { lua::lua_dump(state, collect, (&mut bytecode as *mut Vec<u8>).cast(), 0) };
        Ok(bytecode)
    } else {
        Err(
            String::from_utf8_lossy(unsafe { lua::to_bytes(state, -1) }.unwrap_or_default())
                .into_owned(),
        )
    };
    unsafe { lua::lua_close(state) };
    result
}

fn module_chunk(identifier: &str, main: &str, module: &str) -> String {
    if module == main {
        return format!("={identifier}");
    }
    format!(
        "={identifier}/{}",
        module
            .strip_suffix(".lua")
            .unwrap_or(module)
            .replace('/', ".")
    )
}

pub fn compile_plugin(folder: &Path) -> Result<(), String> {
    let manifest = read_manifest(folder)?;
    let mut stale = Vec::new();
    for directory in std::iter::once(folder.to_path_buf()).chain(
        std::fs::read_dir(folder)
            .map_err(|error| error.to_string())?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir()),
    ) {
        for entry in std::fs::read_dir(&directory)
            .map_err(|error| error.to_string())?
            .flatten()
        {
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "luac")
            {
                stale.push(entry.path());
            }
        }
    }
    for path in stale {
        std::fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    for module in plugin_modules(folder)? {
        let source = folder.join(&module);
        let bytes = std::fs::read(&source).map_err(|error| format!("{module}: {error}"))?;
        let bytecode = compile_lua(&module_chunk(&manifest.id, &manifest.main, &module), &bytes)
            .map_err(|error| format!("{module}: {error}"))?;
        std::fs::write(bytecode_path(&repo::text(&source)), bytecode)
            .map_err(|error| format!("{module}: {error}"))?;
    }
    Ok(())
}

pub fn write_plugin_signature(folder: &Path, key: &SigningKey) -> Result<(), String> {
    compile_plugin(folder)?;
    let files = plugin_files(folder)?;
    let signature = STANDARD.encode(key.sign(&signature_message(&files)).to_bytes());
    let mut document = format!("signature = \"{signature}\"\n\n[files]\n");
    for (path, sum) in &files {
        document.push_str(&format!("\"{path}\" = \"{sum}\"\n"));
    }
    std::fs::write(folder.join("signature"), document).map_err(|error| error.to_string())
}

pub fn read_plugin_signature(folder: &Path) -> Result<PluginSignature, String> {
    toml::from_str(&repo::read(&folder.join("signature"))?).map_err(|error| error.to_string())
}

fn verify(key: &VerifyingKey, message: &[u8], encoded: &str) -> bool {
    let Ok(raw) = STANDARD.decode(encoded.trim()) else {
        return false;
    };
    let Ok(signature) = Signature::from_slice(&raw) else {
        return false;
    };
    key.verify(message, &signature).is_ok()
}

pub fn verify_plugin(folder: &Path, key: &VerifyingKey) -> Result<(), String> {
    let name = name_of(folder);
    let signature = read_plugin_signature(folder).map_err(|error| format!("{name}: {error}"))?;
    let current = plugin_files(folder).map_err(|error| format!("{name}: {error}"))?;
    if current.len() != signature.files.len() {
        return Err(format!(
            "{name}: signature lists different files from the folder (run astral sign)"
        ));
    }
    for (path, sum) in &current {
        if signature.files.get(path) != Some(sum) {
            return Err(format!(
                "{name}: {path} changed since it was signed (run astral sign)"
            ));
        }
    }
    if !verify(
        key,
        &signature_message(&signature.files),
        &signature.signature,
    ) {
        return Err(format!("{name}: invalid signature (run astral sign)"));
    }
    Ok(())
}

pub fn verify_catalog(key: &VerifyingKey) -> Result<(), String> {
    let index = std::fs::read(repo::path(&["packages", "catalog", "index.toml"]))
        .map_err(|error| error.to_string())?;
    let signature = repo::read(&repo::path(&["packages", "catalog", "index.toml.sig"]))?;
    if verify(key, &index, &signature) {
        Ok(())
    } else {
        Err("stale or invalid catalog signature (run astral sign)".into())
    }
}

fn default_signing_key() -> String {
    let configured = environment("ASTRAL_SIGNING_KEY_FILE");
    if !configured.is_empty() {
        return configured;
    }
    format!(
        "{}/Library/Mobile Documents/com~apple~CloudDocs/astral-plugin-signing/astral-plugin-signing.pem",
        environment("HOME")
    )
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("sign")
        .string("key", &default_signing_key())
        .parse(arguments)?;
    let key = private_key(&flags.get("key"))?;
    for folder in repo::plugin_folders()? {
        write_plugin_signature(&folder, &key)
            .map_err(|error| format!("{}: {error}", name_of(&folder)))?;
    }
    let index = std::fs::read(repo::path(&["packages", "catalog", "index.toml"]))
        .map_err(|error| error.to_string())?;
    let signature = STANDARD.encode(key.sign(&index).to_bytes());
    std::fs::write(
        repo::path(&["packages", "catalog", "index.toml.sig"]),
        signature,
    )
    .map_err(|error| error.to_string())
}
