use std::process::ExitCode;

use astral_tools::{check, dev, device, lint, readme, repo, signing, stage, test, vphone};

type Run = fn(&[String]) -> Result<(), String>;

const COMMANDS: &[(&str, &str, Run)] = &[
    ("check", "check [--staged] [file ...]", check::run),
    ("deploy", "deploy [--device <udid>]", device::deploy),
    (
        "dev",
        "dev <status|push|watch|eval|logs|theme|backup> ...",
        dev::run,
    ),
    ("format", "format", lint::format),
    ("lint", "lint", lint::run),
    (
        "open-settings",
        "open-settings [--device <udid>] [row]",
        device::open_settings,
    ),
    ("readme", "readme [--check]", readme::run),
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

fn fall_back_to_go(arguments: &[String]) -> ExitCode {
    let Ok(root) = repo::root() else {
        return ExitCode::FAILURE;
    };
    let status = std::process::Command::new("go")
        .args(["run", "./packages/tools/cmd/astral"])
        .args(arguments)
        .current_dir(&root)
        .status();
    match status {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(name) = arguments.first() else {
        let mut lines = vec!["usage: astral <command>".to_owned()];
        lines.extend(COMMANDS.iter().map(|(_, usage, _)| format!("  {usage}")));
        eprintln!("{}", lines.join("\n"));
        return ExitCode::from(2);
    };
    let Some((_, _, run)) = COMMANDS.iter().find(|(command, _, _)| command == name) else {
        return fall_back_to_go(&arguments);
    };
    match run(&arguments[1..]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("astral {name}: {error}");
            ExitCode::FAILURE
        }
    }
}
