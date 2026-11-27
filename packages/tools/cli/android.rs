use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::files;
use crate::flags::{Flags, environment};
use crate::repo::{self, strings};

const API_LEVEL: &str = "28";
const NATIVE_LIBRARY: &str = "libastral.so";
const ABIS: [(&str, &str); 2] = [
    ("arm64-v8a", "aarch64-linux-android"),
    ("x86_64", "x86_64-linux-android"),
];
const USAGE: &str = "usage: astral android <command>
  build [--java-home <JDK>] [--output <directory>] [--patcher <LSPatch.jar> <Discord APK>...]
  deploy --device <adb serial> --mode <module|sideload> <APK>...
  download [--channel alpha] [--output <directory>]
  version [--channel alpha]";
const TRACKER: &str = "https://tracker.vendetta.rocks/tracker";
const DISCORD_CERTIFICATE: &str =
    "3c39d23cf9367849a5c699395647fe0e5bfea5a1f1f40d8c717ddc70f8bfa113";
const MAXIMUM_APK: u64 = 512 << 20;
pub const SCAN_SPLITS: [&str; 2] = ["base", "config.arm64_v8a"];
const INSTALL_SPLITS: [&str; 4] = ["base", "config.arm64_v8a", "config.xxhdpi", "config.en"];

pub fn ndk_toolchain(explicit: &str) -> Result<PathBuf, String> {
    let host = if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else {
        "linux-x86_64"
    };
    let mut candidates = vec![
        explicit.to_owned(),
        environment("ANDROID_NDK_HOME"),
        environment("ANDROID_NDK_LATEST_HOME"),
    ];
    let mut installed =
        files::glob(&PathBuf::from(environment("HOME")).join("Library/Android/sdk/ndk/*"));
    installed.reverse();
    candidates.extend(installed.iter().map(|path| repo::text(path)));
    candidates
        .into_iter()
        .filter(|candidate| !candidate.is_empty())
        .map(|root| {
            PathBuf::from(root)
                .join("toolchains/llvm/prebuilt")
                .join(host)
                .join("bin")
        })
        .find(|toolchain| toolchain.join("llvm-ar").exists())
        .ok_or_else(|| "no Android NDK; pass --ndk or set ANDROID_NDK_HOME".into())
}

fn commit() -> String {
    repo::output(
        &repo::path(&[]),
        "git",
        &strings(&["rev-parse", "--short=9", "HEAD"]),
    )
    .map(|commit| commit.trim().to_owned())
    .ok()
    .filter(|commit| !commit.is_empty())
    .unwrap_or_else(|| "unknown".to_owned())
}

fn linker_variable(triple: &str) -> String {
    format!(
        "CARGO_TARGET_{}_LINKER",
        triple.to_ascii_uppercase().replace('-', "_")
    )
}

pub fn build_native(toolchain: &Path, triple: &str, directory: &Path) -> Result<PathBuf, String> {
    let compiler = repo::text(&toolchain.join(format!("{triple}{API_LEVEL}-clang")));
    let archiver = repo::text(&toolchain.join("llvm-ar"));
    let version = repo::read(&repo::path(&["version.txt"]))?.trim().to_owned();
    let commit = commit();
    let linker = linker_variable(triple);
    repo::run_with_env(
        &repo::path(&[]),
        &[
            ("CC", compiler.as_str()),
            ("AR", archiver.as_str()),
            (linker.as_str(), compiler.as_str()),
            ("ASTRAL_VERSION", version.as_str()),
            ("ASTRAL_COMMIT", commit.as_str()),
        ],
        "cargo",
        &strings(&[
            "build",
            "--profile",
            &repo::rust_profile(),
            "--quiet",
            "--target",
            triple,
            "-p",
            "astral-android",
        ]),
    )?;
    let built = repo::path(&["target", triple, &repo::rust_profile(), NATIVE_LIBRARY]);
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let library = directory.join(NATIVE_LIBRARY);
    files::copy(&built, &library)?;
    Ok(library)
}

pub fn core(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("android-core")
        .string("ndk", "")
        .string("abi", "")
        .string(
            "output",
            &repo::text(&repo::path(&["packages", "android", "build", "native"])),
        )
        .parse(arguments)?;
    let toolchain = ndk_toolchain(&flags.get("ndk"))?;
    let only = flags.get("abi");
    for (abi, triple) in ABIS
        .iter()
        .filter(|(abi, _)| only.is_empty() || only.split(',').any(|wanted| wanted == *abi))
    {
        let library = build_native(
            &toolchain,
            triple,
            &PathBuf::from(flags.get("output")).join(abi),
        )
        .map_err(|error| format!("{abi}: {error}"))?;
        println!("{}", library.display());
    }
    Ok(())
}

pub fn java_home(explicit: &str) -> Result<PathBuf, String> {
    let mut candidates = vec![explicit.to_owned(), environment("JAVA_HOME")];
    if explicit.is_empty() {
        let mut cached = files::glob(
            &PathBuf::from(environment("HOME")).join("Library/Caches/unbound/jdk/*/Contents/Home"),
        );
        cached.reverse();
        candidates.extend(cached.iter().map(|path| repo::text(path)));
    }
    for candidate in candidates {
        if !candidate.is_empty() && Path::new(&candidate).join("bin/java").exists() {
            return std::fs::canonicalize(&candidate).map_err(|error| error.to_string());
        }
        if !explicit.is_empty() {
            break;
        }
    }
    Err("no Java runtime; pass --java-home or set JAVA_HOME to a JDK 21 installation".into())
}

pub fn apk_inputs(paths: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut inputs: Vec<PathBuf> = Vec::new();
    for input in paths {
        let absolute = std::fs::canonicalize(input).map_err(|error| format!("{input}: {error}"))?;
        let apk = absolute
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("apk"));
        if !absolute.is_file() || !apk || inputs.contains(&absolute) {
            return Err(format!("expected distinct APK files: {input}"));
        }
        inputs.push(absolute);
    }
    Ok(inputs)
}

fn build(arguments: &[String]) -> Result<(), String> {
    let default = repo::text(&repo::path(&["packages", "android", "build", "astral"]));
    let flags = Flags::new("android build")
        .string("java-home", "")
        .string("abi", "")
        .string("output", &default)
        .string("patcher", "")
        .parse(arguments)?;
    let inputs = apk_inputs(&flags.rest)?;
    if flags.get("patcher").is_empty() != inputs.is_empty() {
        return Err("pass --patcher and the Discord base APK with its splits together".into());
    }
    let java = java_home(&flags.get("java-home"))?;
    let java_text = repo::text(&java);
    let sdk = PathBuf::from(environment("HOME")).join("Library/Android/sdk");
    let sdk_text = repo::text(&sdk);
    let mut variables = vec![("JAVA_HOME", java_text.as_str())];
    if environment("ANDROID_HOME").is_empty() && sdk.exists() {
        variables.push(("ANDROID_HOME", sdk_text.as_str()));
    }
    let mut gradle = strings(&[":module:assembleDebug"]);
    if !flags.get("abi").is_empty() {
        gradle.push(format!("-Pastral.abis={}", flags.get("abi")));
    }
    repo::run_with_env(
        &repo::path(&["packages", "android"]),
        &variables,
        "./gradlew",
        &gradle,
    )?;
    let module = repo::path(&[
        "packages",
        "android",
        "module",
        "build",
        "outputs",
        "apk",
        "debug",
        "module-debug.apk",
    ]);
    std::fs::create_dir_all(flags.get("output")).map_err(|error| error.to_string())?;
    let output = std::fs::canonicalize(flags.get("output")).map_err(|error| error.to_string())?;
    files::copy(&module, &output.join("astral-module.apk"))?;
    if inputs.is_empty() {
        println!("{}", output.join("astral-module.apk").display());
        return Ok(());
    }
    let jar = std::fs::canonicalize(flags.get("patcher")).map_err(|error| error.to_string())?;
    let work = repo::scratch()?;
    let patched = work.0.join("patched");
    let mut patch = strings(&["-jar", &repo::text(&jar)]);
    patch.extend(inputs.iter().map(|input| repo::text(input)));
    patch.extend(strings(&[
        "-m",
        &repo::text(&module),
        "-l",
        "2",
        "-o",
        &repo::text(&patched),
    ]));
    repo::run(&work.0, &repo::text(&java.join("bin/java")), &patch)?;
    let outputs = files::glob(&patched.join("*.apk"));
    if outputs.len() != inputs.len() {
        return Err(format!(
            "patcher produced {} APKs for {} inputs",
            outputs.len(),
            inputs.len()
        ));
    }
    for file in outputs {
        open_phoenix_access(&file, &jar, &work.0)?;
        register_link_scheme(&file, &jar, &work.0)?;
        strip_split_copies(&file, &jar, &work.0)?;
        let target = output
            .join("sideload")
            .join(file.file_name().unwrap_or_default());
        files::copy(&file, &target)?;
        println!("{}", target.display());
    }
    Ok(())
}

fn adb(serial: &str, arguments: &[&str]) -> Result<(), String> {
    let mut all = strings(&["-s", serial]);
    all.extend(strings(arguments));
    repo::run(&repo::path(&[]), "adb", &all)
}

fn deploy(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("android deploy")
        .string("device", "")
        .string("mode", "")
        .parse(arguments)?;
    let (serial, mode) = (flags.get("device"), flags.get("mode"));
    if serial.is_empty() || (mode != "module" && mode != "sideload") || flags.rest.is_empty() {
        return Err(USAGE.into());
    }
    let inputs = apk_inputs(&flags.rest)?;
    if mode == "module" && inputs.len() != 1 {
        return Err("module deployment takes exactly one astral module APK".into());
    }
    let mut install = vec!["install-multiple".to_owned(), "-r".to_owned()];
    install.extend(inputs.iter().map(|input| repo::text(input)));
    let install: Vec<&str> = install.iter().map(String::as_str).collect();
    adb(&serial, &install)?;
    if mode == "module" {
        for command in [
            "sh /data/adb/modules/zygisk_vector/cli modules enable app.astral.android",
            "sh /data/adb/modules/zygisk_vector/cli scope set app.astral.android com.discord/0",
        ] {
            adb(&serial, &["shell", "su", "-c", command])?;
        }
    }
    adb(&serial, &["shell", "am", "force-stop", "com.discord"])?;
    adb(
        &serial,
        &[
            "shell",
            "monkey",
            "-p",
            "com.discord",
            "-c",
            "android.intent.category.LAUNCHER",
            "1",
        ],
    )
}

fn tracker_get(url: &str) -> Result<ureq::Response, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(600))
        .build();
    let mut last = String::new();
    for attempt in 1..=4u64 {
        match agent.get(url).call() {
            Ok(response) => return Ok(response),
            Err(ureq::Error::Status(code, response)) if code < 500 => {
                return Err(format!(
                    "HTTP {code}: {}",
                    response.into_string().unwrap_or_default()
                ));
            }
            Err(error) => last = error.to_string(),
        }
        if attempt < 4 {
            std::thread::sleep(Duration::from_secs(attempt * attempt * 5));
        }
    }
    Err(last)
}

fn version_code(channel: &str) -> Result<i64, String> {
    let index: Value = tracker_get(&format!("{TRACKER}/index"))
        .map_err(|error| format!("read the Discord APK index: {error}"))?
        .into_string()
        .map_err(|error| error.to_string())
        .and_then(|text| serde_json::from_str(&text).map_err(|error| error.to_string()))
        .map_err(|error| format!("parse the Discord APK index: {error}"))?;
    let latest = index
        .get("latest")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    latest
        .get(channel)
        .and_then(Value::as_i64)
        .filter(|code| *code > 0)
        .ok_or_else(|| {
            let channels: Vec<&String> = latest.keys().collect();
            format!(
                "no Discord {channel:?} channel; the tracker lists {}",
                channels
                    .iter()
                    .map(|name| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn download_split(code: i64, split: &str, directory: &Path) -> Result<PathBuf, String> {
    let response = tracker_get(&format!("{TRACKER}/download/{code}/{split}"))
        .map_err(|error| format!("download Discord {code} {split}: {error}"))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAXIMUM_APK + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAXIMUM_APK {
        return Err(format!(
            "Discord {code} {split} is larger than {} MiB",
            MAXIMUM_APK >> 20
        ));
    }
    let destination = directory.join(format!("{split}.apk"));
    std::fs::write(&destination, bytes).map_err(|error| error.to_string())?;
    Ok(destination)
}

fn build_tool(name: &str) -> Result<String, String> {
    if let Ok(path) = repo::output(&repo::path(&[]), "which", &strings(&[name])) {
        return Ok(path.trim().to_owned());
    }
    for root in [
        environment("ANDROID_HOME"),
        environment("ANDROID_SDK_ROOT"),
        format!("{}/Library/Android/sdk", environment("HOME")),
    ] {
        if !root.is_empty()
            && let Some(found) =
                files::glob(&PathBuf::from(&root).join("build-tools/*").join(name)).pop()
        {
            return Ok(repo::text(&found));
        }
    }
    Err(format!(
        "{name} was not found; install Android build-tools or put {name} on PATH"
    ))
}

const CERTIFICATE_DIGEST: &str = "certificate SHA-256 digest:";
const SOURCE_STAMP: &str = "Source Stamp";

fn verify_apk(signer: &str, apk: &Path) -> Result<(), String> {
    let name = apk
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let output = std::process::Command::new(signer)
        .args(["verify", "--print-certs", &repo::text(apk)])
        .output()
        .map_err(|error| error.to_string())?;
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(format!("{name} does not verify: {printed}"));
    }
    let digests: Vec<&str> = printed
        .lines()
        .filter(|line| !line.starts_with(SOURCE_STAMP) && line.contains(CERTIFICATE_DIGEST))
        .filter_map(|line| {
            line.split_once(CERTIFICATE_DIGEST)
                .map(|(_, digest)| digest.trim())
        })
        .collect();
    if digests.is_empty() {
        return Err(format!("{name} has no signer certificate: {printed}"));
    }
    if digests.contains(&DISCORD_CERTIFICATE) {
        Ok(())
    } else {
        Err(format!(
            "{name} is not signed by Discord: {}",
            digests.join(", ")
        ))
    }
}

pub fn download_apks(
    channel: &str,
    splits: &[&str],
    directory: &Path,
) -> Result<(i64, Vec<PathBuf>), String> {
    let code = version_code(channel)?;
    let signer = build_tool("apksigner")?;
    let mut apks = Vec::new();
    for split in splits {
        let apk = download_split(code, split, directory)?;
        verify_apk(&signer, &apk)?;
        apks.push(apk);
    }
    Ok((code, apks))
}

fn download(arguments: &[String]) -> Result<(), String> {
    let default = repo::text(&repo::path(&["packages", "android", "build", "discord"]));
    let flags = Flags::new("android download")
        .string("channel", "alpha")
        .string("output", &default)
        .parse(arguments)?;
    let output = PathBuf::from(flags.get("output"));
    std::fs::create_dir_all(&output).map_err(|error| error.to_string())?;
    let (code, apks) = download_apks(&flags.get("channel"), &INSTALL_SPLITS, &output)?;
    println!(
        "Discord {} {code}, signed by Discord:",
        flags.get("channel")
    );
    for apk in apks {
        println!("{}", apk.display());
    }
    Ok(())
}

fn version(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("android version")
        .string("channel", "alpha")
        .parse(arguments)?;
    println!("{}", version_code(&flags.get("channel"))?);
    Ok(())
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    match arguments.first().map(String::as_str) {
        Some("version") => version(&arguments[1..]),
        Some("build") => build(&arguments[1..]),
        Some("deploy") => deploy(&arguments[1..]),
        Some("download") => download(&arguments[1..]),
        _ => Err(USAGE.into()),
    }
}

const PHOENIX_SO: &str = "lib/arm64-v8a/liblibdiscord_wasm_host.so";
const LSPATCH_KEYSTORE: &str = "assets/keystore";
const LSPATCH_ALIAS: &str = "key0";
const LSPATCH_PASSWORD: &str = "123456";

fn open_phoenix_access(
    apk: &std::path::Path,
    patcher: &std::path::Path,
    work: &std::path::Path,
) -> Result<(), String> {
    let archive = crate::ipa_archive::Ipa::open(apk)?;
    let Some(entry) = archive
        .names()
        .into_iter()
        .find(|name| name.ends_with(PHOENIX_SO))
    else {
        return Ok(());
    };
    crate::ipa_archive::rewrite(
        apk,
        "phoenix-access",
        |name| {
            if name == entry {
                crate::ipa_archive::Action::Patch(Box::new(
                    crate::phoenix::patch_android_access_gate,
                ))
            } else {
                crate::ipa_archive::Action::Keep
            }
        },
        Vec::new(),
    )?;
    resign(apk, patcher, work)?;
    println!("opened Phoenix's access gate in {}", apk.display());
    Ok(())
}

const MANIFEST: &str = "AndroidManifest.xml";
const LINK_ACTIVITY: &str = "com.discord.main.MainActivity";
const LINK_SCHEME: &str = "astral";

fn register_link_scheme(
    apk: &std::path::Path,
    patcher: &std::path::Path,
    work: &std::path::Path,
) -> Result<(), String> {
    let name = apk
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if is_config_split(name) {
        return Ok(());
    }
    let manifest = crate::ipa_archive::Ipa::open(apk)?.read(MANIFEST)?;
    let Some(patched) = crate::axml::add_scheme_filter(&manifest, LINK_ACTIVITY, LINK_SCHEME)?
    else {
        return Ok(());
    };
    crate::ipa_archive::rewrite(
        apk,
        "link-scheme",
        |entry| {
            if entry == MANIFEST {
                let patched = patched.clone();
                crate::ipa_archive::Action::Patch(Box::new(move |_| Ok(patched)))
            } else {
                crate::ipa_archive::Action::Keep
            }
        },
        Vec::new(),
    )?;
    resign(apk, patcher, work)?;
    println!("registered {LINK_SCHEME}:// links in {}", apk.display());
    Ok(())
}

const SPLIT_REDUNDANT: [&str; 2] = ["assets/lspatch/origin.apk", "assets/lspatch/modules/"];
const CONFIG_SPLIT_PREFIX: &str = "config.";

pub fn redundant_in_split(name: &str) -> bool {
    SPLIT_REDUNDANT
        .iter()
        .any(|prefix| name == *prefix || (prefix.ends_with('/') && name.starts_with(prefix)))
}

pub fn is_config_split(file_name: &str) -> bool {
    file_name.starts_with(CONFIG_SPLIT_PREFIX)
}

fn strip_split_copies(
    apk: &std::path::Path,
    patcher: &std::path::Path,
    work: &std::path::Path,
) -> Result<(), String> {
    let file_name = apk
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let names = crate::ipa_archive::Ipa::open(apk)?.names();
    if !is_config_split(&file_name) || !names.iter().any(|name| redundant_in_split(name)) {
        return Ok(());
    }
    crate::ipa_archive::rewrite(
        apk,
        "split-copies",
        |name| {
            if redundant_in_split(name) {
                crate::ipa_archive::Action::Drop
            } else {
                crate::ipa_archive::Action::Keep
            }
        },
        Vec::new(),
    )?;
    resign(apk, patcher, work)?;
    println!("removed LSPatch's duplicate copies from {}", apk.display());
    Ok(())
}

fn resign(
    apk: &std::path::Path,
    patcher: &std::path::Path,
    work: &std::path::Path,
) -> Result<(), String> {
    let keystore = work.join("lspatch.keystore");
    std::fs::write(
        &keystore,
        crate::ipa_archive::Ipa::open(patcher)?.read(LSPATCH_KEYSTORE)?,
    )
    .map_err(|error| error.to_string())?;
    let aligned = work.join("resign-aligned.apk");
    repo::run(
        work,
        &build_tool("zipalign")?,
        &strings(&[
            "-f",
            "-P",
            "16",
            "4",
            &repo::text(apk),
            &repo::text(&aligned),
        ]),
    )?;
    let password = format!("pass:{LSPATCH_PASSWORD}");
    repo::run(
        work,
        &build_tool("apksigner")?,
        &strings(&[
            "sign",
            "--ks",
            &repo::text(&keystore),
            "--ks-pass",
            &password,
            "--ks-key-alias",
            LSPATCH_ALIAS,
            "--key-pass",
            &password,
            "--out",
            &repo::text(apk),
            &repo::text(&aligned),
        ]),
    )?;
    Ok(())
}
