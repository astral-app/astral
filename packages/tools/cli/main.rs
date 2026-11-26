use std::process::ExitCode;

use astral_tools::{
    android, android_compat, avd, check, compat, create, dev, device, doctor, fuzz, ipa, lint,
    palette, readme, sideload, signing, stage, test, theos, vphone,
};

type Run = fn(&[String]) -> Result<(), String>;

const COMMANDS: &[(&str, &str, Run)] = &[
    (
        "android",
        "android <build|deploy|download> ...",
        android::run,
    ),
    (
        "android-compat",
        "android-compat (--channel <stable|beta|alpha> | --apk <Discord.apk> [--apk <split.apk> ...]) [--results android-compat.json] [--report android-compat.md]",
        android_compat::run,
    ),
    (
        "android-core",
        "android-core [--ndk <directory>] [--output <directory>]",
        android::core,
    ),
    (
        "avd",
        "avd <boot|stop|status|discord|deploy|launch|screenshot|open|tap|settings|logs> ...",
        avd::run,
    ),
    ("bench", "bench [captured batch ...]", fuzz::bench),
    (
        "build-ipa",
        "build-ipa --ipa <decrypted Discord.ipa> [--deb <astral.deb>] [--output <astral.ipa>]",
        ipa::run,
    ),
    ("check", "check [--staged] [file ...]", check::run),
    (
        "compat",
        "compat --ipa <decrypted Discord.ipa> [--channel stable] [--baseline compat.json] [--results compat.json] [--report report.md]",
        compat::run,
    ),
    (
        "create",
        "create [--name <name>] [--author <name>] [--dir <directory>] <id>",
        create::run,
    ),
    ("deploy", "deploy [--device <udid>]", device::deploy),
    (
        "dev",
        "dev <status|push|watch|eval|logs|theme|backup> ...",
        dev::run,
    ),
    ("doctor", "doctor", doctor::run),
    ("format", "format", lint::format),
    (
        "fuzz",
        "fuzz [--seconds 60] [--corpus <directory>]",
        fuzz::fuzz,
    ),
    ("lint", "lint", lint::run),
    (
        "open-settings",
        "open-settings [--device <udid>] [row]",
        device::open_settings,
    ),
    (
        "palette",
        "palette semantic --source <generated-definitions.tsx> --origin <description> [--output <toml>]",
        palette::run,
    ),
    (
        "prepare-theos",
        "prepare-theos [--theos <directory>]",
        theos::run,
    ),
    ("readme", "readme [--check]", readme::run),
    ("replay", "replay <plugin>... <capture>", fuzz::replay),
    ("sdk", "sdk install", create::sdk),
    (
        "sideload",
        "sideload --ipa <IPA> --bundle-id <id> [--device <udid>] [--profile <mobileprovision>]",
        sideload::run,
    ),
    ("sign", "sign [--key <pem>]", signing::run),
    ("stage", "stage <directory>", stage::run),
    ("test", "test [--phoenix <PhoenixFatClient>]", test::run),
    (
        "vphone",
        "vphone <boot|status|install|launch|shell|push|pull|deploy> ...",
        vphone::run,
    ),
    (
        "wire-dump",
        "wire-dump [--device <udid>] <output directory>",
        device::wire_dump,
    ),
];

fn usage() -> ExitCode {
    let mut lines = vec!["usage: astral <command>".to_owned()];
    lines.extend(COMMANDS.iter().map(|(_, usage, _)| format!("  {usage}")));
    eprintln!("{}", lines.join("\n"));
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(name) = arguments.first() else {
        return usage();
    };
    let Some((_, _, run)) = COMMANDS.iter().find(|(command, _, _)| command == name) else {
        return usage();
    };
    match run(&arguments[1..]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("astral {name}: {error}");
            ExitCode::FAILURE
        }
    }
}
