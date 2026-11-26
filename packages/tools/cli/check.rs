use std::path::Path;

use crate::flags::Flags;
use crate::lint;
use crate::repo::{self, strings, under};
use crate::test;
use crate::workflows;

type Affects = fn(&str) -> bool;
type Step = fn(&[String], &Path) -> Result<(), String>;

pub struct Rule {
    pub name: &'static str,
    pub affects: Affects,
    pub run: Step,
}

const NATIVE: [&str; 5] = ["c", "h", "m", "mm", "cpp"];

fn native(path: &str) -> bool {
    NATIVE.contains(&repo::extension(path))
}

fn rust(path: &str) -> bool {
    under(
        path,
        &[
            "packages/core/",
            "packages/tools/",
            "packages/android/native/",
            "packages/ios/native/",
        ],
    ) && (repo::extension(path) == "rs"
        || path.ends_with("/Cargo.toml")
        || path.ends_with("/Cargo.lock"))
}

fn lua(path: &str) -> bool {
    repo::extension(path) == "lua" && under(path, &["packages/"])
}

fn with_files(command: &str, base: &[&str], files: &[String]) -> Result<(), String> {
    let mut arguments = strings(base);
    arguments.extend(files.iter().cloned());
    repo::run(&repo::path(&[]), command, &arguments)
}

pub fn rules() -> Vec<Rule> {
    vec![
        Rule {
            name: "clang-format",
            affects: |path| native(path) && under(path, &["packages/"]),
            run: |files, _| with_files("clang-format", &["--dry-run", "-Werror"], files),
        },
        Rule {
            name: "rust",
            affects: rust,
            run: |_, _| {
                lint::lint_rust()?;
                lint::test_rust()
            },
        },
        Rule {
            name: "stylua",
            affects: lua,
            run: |files, _| with_files("stylua", &["--check"], files),
        },
        Rule {
            name: "luacheck",
            affects: |path| {
                lua(path)
                    && under(
                        path,
                        &[
                            "packages/addons/plugins/",
                            "packages/core/lua/",
                            "packages/core/tests/",
                        ],
                    )
            },
            run: |files, _| with_files("luacheck", &["--quiet"], files),
        },
        Rule {
            name: "lua casing",
            affects: |path| {
                lua(path)
                    && under(
                        path,
                        &[
                            "packages/addons/plugins/",
                            "packages/core/lua/",
                            "packages/sdk/",
                        ],
                    )
            },
            run: |files, _| crate::lua_casing::check(files),
        },
        Rule {
            name: "rust magic numbers",
            affects: |path| {
                repo::extension(path) == "rs"
                    && under(path, &["packages/core/", "packages/tools/"])
                    && !path.contains("/tests/")
            },
            run: |files, _| crate::rust_magic::check(files),
        },
        Rule {
            name: "lua enums",
            affects: |path| lua(path) && under(path, &["packages/addons/plugins/"]),
            run: |files, _| crate::lua_enums::check(files),
        },
        Rule {
            name: "sdk enums",
            affects: |path| {
                path == "packages/core/lua/enums.rs" || path == "packages/sdk/library/astral.lua"
            },
            run: |_, _| crate::lua_enums::sync_sdk(true),
        },
        Rule {
            name: "lua types",
            affects: |path| lua(path) || path == ".luarc.json",
            run: |_, _| lint::typecheck_lua(),
        },
        Rule {
            name: "taplo",
            affects: |path| repo::extension(path) == "toml",
            run: |files, _| {
                let mut arguments = strings(&["lint"]);
                arguments.extend(files.iter().cloned());
                lint::taplo(&arguments)?;
                let mut arguments = strings(&["format", "--check"]);
                arguments.extend(files.iter().cloned());
                lint::taplo(&arguments)
            },
        },
        Rule {
            name: "workflows",
            affects: workflows::is_workflow_yaml,
            run: |files, _| {
                let existing: Vec<String> = files
                    .iter()
                    .filter(|file| {
                        *file != ".yamlfmt.yml" && repo::file_exists(&repo::path(&[file]))
                    })
                    .cloned()
                    .collect();
                if existing.is_empty() {
                    workflows::lint_all()
                } else {
                    workflows::lint(&existing)
                }
            },
        },
        Rule {
            name: "wire tests",
            affects: |path| under(path, &["packages/core/wire/", "packages/ios/tests/"]),
            run: |_, work| test::test_wire(work),
        },
        Rule {
            name: "core tests",
            affects: |path| {
                (native(path) || repo::extension(path) == "rs")
                    && under(path, &["packages/core/"])
                    && !under(path, &["packages/core/wire/"])
            },
            run: |_, work| test::test_core(work),
        },
        Rule {
            name: "lua tests",
            affects: |path| {
                under(
                    path,
                    &[
                        "packages/core/lua/",
                        "packages/core/tests/",
                        "packages/addons/plugins/",
                    ],
                ) && !native(path)
            },
            run: |_, work| test::test_lua(work),
        },
        Rule {
            name: "signatures",
            affects: |path| {
                under(
                    path,
                    &["packages/addons/plugins/", "packages/catalog/", "keys/"],
                )
            },
            run: |_, work| test::test_signatures(work),
        },
    ]
}

fn file_scoped(name: &str) -> bool {
    matches!(
        name,
        "clang-format"
            | "stylua"
            | "luacheck"
            | "lua casing"
            | "lua enums"
            | "rust magic numbers"
            | "taplo"
    )
}

pub fn plan<'a>(
    rules: &'a [Rule],
    changed: &[String],
    exists: impl Fn(&str) -> bool,
) -> Vec<(&'a Rule, Vec<String>)> {
    let mut plan = Vec::new();
    for rule in rules {
        let affected: Vec<&String> = changed.iter().filter(|path| (rule.affects)(path)).collect();
        if affected.is_empty() {
            continue;
        }
        let files: Vec<String> = affected
            .into_iter()
            .filter(|path| exists(path))
            .cloned()
            .collect();
        if !files.is_empty() || !file_scoped(rule.name) {
            plan.push((rule, files));
        }
    }
    plan
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("check").boolean("staged").parse(arguments)?;
    let mut changed = flags.rest.clone();
    if flags.flag("staged") {
        changed = repo::output(
            &repo::path(&[]),
            "git",
            &strings(&["diff", "--cached", "--name-only", "--diff-filter=ACMRD"]),
        )?
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    }
    let rules = rules();
    let plan = plan(&rules, &changed, |path| {
        repo::file_exists(&repo::path(&[path]))
    });
    if plan.is_empty() {
        println!("check: nothing to check");
        return Ok(());
    }
    let work = repo::scratch()?;
    for (rule, files) in plan {
        println!("check: {}", rule.name);
        (rule.run)(&files, &work.0).map_err(|error| format!("{}: {error}", rule.name))?;
    }
    Ok(())
}
