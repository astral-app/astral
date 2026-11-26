use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::flags::Flags;
use crate::fuzz;
use crate::ipa_archive::{self as archive, Ipa};
use crate::macho;
use crate::repo::{self, strings};

const USAGE: &str = "usage: astral compat --ipa <decrypted Discord.ipa> [--channel stable] [--baseline compat.json] [--results compat.json] [--report report.md]";
const NO_PHOENIX: &str = "this Discord build ships no Phoenix engine";
const LOAD_UUID: u32 = 0x1b;

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub ops_arrived: u64,
    pub host_call: u64,
    pub property_count: i64,
    pub resolved: i64,
    pub missing: Option<String>,
    #[serde(default)]
    pub properties: BTreeMap<String, u16>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CompatResult {
    pub phoenix: bool,
    pub channel: String,
    pub version: String,
    pub build: String,
    #[serde(rename = "phoenixUUID")]
    pub phoenix_uuid: String,
    pub probe: Probe,
    #[serde(default)]
    pub access_gate: usize,
    pub supported: bool,
}

struct Extracted {
    version: String,
    build: String,
    phoenix: Option<Vec<u8>>,
}

fn extract(ipa_path: &Path) -> Result<Extracted, String> {
    let mut ipa = Ipa::open(ipa_path)?;
    let app = ipa.app()?;
    let info = ipa.info(&app)?;
    Ok(Extracted {
        version: archive::text(&info, "CFBundleShortVersionString"),
        build: archive::text(&info, "CFBundleVersion"),
        phoenix: ipa
            .read(&format!(
                "{app}Frameworks/PhoenixFatClient.framework/PhoenixFatClient"
            ))
            .ok(),
    })
}

pub fn image_uuid(data: &[u8]) -> Result<String, String> {
    let file = macho::parse(data)?;
    let raw = file
        .loads
        .iter()
        .find(|raw| {
            raw.len() >= 24 && u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) == LOAD_UUID
        })
        .ok_or("PhoenixFatClient has no LC_UUID")?;
    let text: String = raw[8..24]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &text[0..8],
        &text[8..12],
        &text[12..16],
        &text[16..20],
        &text[20..32]
    ))
}

fn build_probe(work: &Path) -> Result<String, String> {
    let binary = repo::text(&work.join("compat-probe"));
    let library = fuzz::core_library(false)?;
    repo::run(
        &repo::path(&["packages", "ios"]),
        "cc",
        &strings(&[
            "-O1",
            "-Wall",
            "-Werror",
            "-o",
            &binary,
            "tests/support/compat-probe.c",
            &format!(
                "-I{}",
                repo::text(&repo::path(&["packages", "core", "wire"]))
            ),
            &library,
        ]),
    )?;
    Ok(binary)
}

fn run_probe(probe: &str, phoenix: &Path) -> Result<Probe, String> {
    let output = Command::new(probe)
        .arg(phoenix)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("Failed to run the compat probe: {error}"))?;
    if output.stdout.is_empty() {
        return Err(format!(
            "compat probe produced no output: {}",
            output.status
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
}

fn read_baseline(path: &str) -> Option<CompatResult> {
    if path.is_empty() {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn access_gate_text(evaluators: usize) -> String {
    match evaluators {
        0 => "**not found**: the non-tester unlock would not apply".to_owned(),
        count => format!("{count} evaluators patched"),
    }
}

fn names(properties: &BTreeMap<String, u16>, other: &BTreeMap<String, u16>) -> Vec<String> {
    properties
        .keys()
        .filter(|name| !other.contains_key(*name))
        .map(|name| format!("`{name}`"))
        .collect()
}

pub fn seam_diff(result: &CompatResult, baseline: &CompatResult) -> String {
    let mut changes = Vec::new();
    let added = names(&result.probe.properties, &baseline.probe.properties);
    if !added.is_empty() {
        changes.push(format!("View properties added: {}", added.join(", ")));
    }
    let removed = names(&baseline.probe.properties, &result.probe.properties);
    if !removed.is_empty() {
        changes.push(format!("View properties removed: {}", removed.join(", ")));
    }
    for (seam, before, now) in [
        (
            "`opsArrived`",
            baseline.probe.ops_arrived != 0,
            result.probe.ops_arrived != 0,
        ),
        (
            "Host-call entry",
            baseline.probe.host_call != 0,
            result.probe.host_call != 0,
        ),
    ] {
        if before != now {
            changes.push(format!(
                "{seam} {}",
                if now { "found again" } else { "is gone" }
            ));
        }
    }
    if baseline.access_gate != result.access_gate {
        changes.push(format!(
            "Access gate evaluators {} → {}",
            baseline.access_gate, result.access_gate
        ));
    }
    if changes.is_empty() {
        return String::new();
    }
    format!(
        "\nSeams that changed since Discord {} ({}):\n\n- {}\n",
        baseline.version,
        baseline.build,
        changes.join("\n- ")
    )
}

pub fn report(result: &CompatResult, baseline: Option<&CompatResult>) -> String {
    let status = if result.supported {
        "supported"
    } else {
        "**not supported**"
    };
    let missing = result
        .probe
        .missing
        .as_ref()
        .map_or_else(|| "none".to_owned(), |missing| format!("`{missing}`"));
    let mut report = format!(
        "## astral on Discord {} ({}, {})\n\nPhoenix `{}` is {status}.\n\n| Check | Result |\n| --- | --- |\n| `opsArrived` | `0x{:x}` |\n| Host-call entry | `0x{:x}` |\n| View properties | {} in Phoenix, {} resolved, missing {missing} |\n| Access gate | {} |\n",
        result.version,
        result.build,
        result.channel,
        result.phoenix_uuid,
        result.probe.ops_arrived,
        result.probe.host_call,
        result.probe.property_count,
        result.probe.resolved,
        access_gate_text(result.access_gate),
    );
    let Some(baseline) = baseline else {
        return report;
    };
    report.push_str(&seam_diff(result, baseline));
    let changes: Vec<String> = result
        .probe
        .properties
        .iter()
        .filter_map(|(name, id)| {
            baseline
                .probe
                .properties
                .get(name)
                .filter(|before| *before != id)
                .map(|before| format!("`{name}` {before} → {id}"))
        })
        .collect();
    if !changes.is_empty() {
        report.push_str(&format!(
            "\nProperty IDs that moved since Discord {} ({}), resolved at launch:\n\n- {}\n",
            baseline.version,
            baseline.build,
            changes.join("\n- ")
        ));
    } else if baseline.phoenix_uuid != result.phoenix_uuid {
        report.push_str(&format!(
            "\nNo property IDs moved since Discord {} ({}).\n",
            baseline.version, baseline.build
        ));
    }
    report
}

pub fn write_results<T: Serialize>(
    result: &T,
    results: &str,
    report_path: &str,
    report: &str,
) -> Result<(), String> {
    let mut encoded = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(
        &mut encoded,
        serde_json::ser::PrettyFormatter::with_indent(b"\t"),
    );
    result
        .serialize(&mut serializer)
        .map_err(|error| error.to_string())?;
    encoded.push(b'\n');
    std::fs::write(results, encoded)
        .map_err(|error| format!("Failed to write {results}: {error}"))?;
    if !report_path.is_empty() {
        std::fs::write(report_path, report)
            .map_err(|error| format!("Failed to write {report_path}: {error}"))?;
    }
    print!("{report}");
    Ok(())
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("compat")
        .string("ipa", "")
        .string("channel", "stable")
        .string("baseline", "")
        .string("results", "compat.json")
        .string("report", "")
        .parse(arguments)?;
    let ipa = flags.get("ipa");
    if ipa.is_empty() {
        return Err(USAGE.into());
    }
    let extracted = extract(Path::new(&ipa))?;
    let mut result = CompatResult {
        channel: flags.get("channel"),
        version: extracted.version,
        build: extracted.build,
        ..CompatResult::default()
    };
    let Some(phoenix) = extracted.phoenix else {
        let report = format!(
            "## astral on Discord {} ({}, {})\n\nThis build ships no Phoenix engine, so astral has nothing to run on.\n",
            result.version, result.build, result.channel
        );
        write_results(
            &result,
            &flags.get("results"),
            &flags.get("report"),
            &report,
        )?;
        return Err(NO_PHOENIX.into());
    };
    let work = repo::scratch()?;
    let phoenix_path = work.0.join("PhoenixFatClient");
    std::fs::write(&phoenix_path, &phoenix).map_err(|error| error.to_string())?;
    result.phoenix = true;
    result.phoenix_uuid = image_uuid(&phoenix)?;
    result.probe = run_probe(&build_probe(&work.0)?, &phoenix_path)?;
    result.access_gate = crate::phoenix::access_gate_evaluators(&phoenix);
    result.supported = result.probe.ops_arrived != 0
        && result.probe.host_call != 0
        && result.probe.missing.is_none();
    let report = report(&result, read_baseline(&flags.get("baseline")).as_ref());
    write_results(
        &result,
        &flags.get("results"),
        &flags.get("report"),
        &report,
    )?;
    if !result.supported {
        return Err("this Discord build is not supported".into());
    }
    Ok(())
}
