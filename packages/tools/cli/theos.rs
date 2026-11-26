use std::path::Path;

use crate::files;
use crate::flags::{Flags, environment};
use crate::repo::{self, strings};

fn remote_head(repository: &str) -> Result<String, String> {
    repo::output(
        &repo::path(&[]),
        "git",
        &strings(&["ls-remote", repository, "HEAD"]),
    )?
    .split_whitespace()
    .next()
    .map(str::to_owned)
    .ok_or_else(|| format!("no HEAD for {repository}"))
}

fn update_theos(theos: &Path, head: &str) -> Result<(), String> {
    if !theos.exists() {
        return repo::run(
            &repo::path(&[]),
            "git",
            &strings(&[
                "clone",
                "--recursive",
                "https://github.com/theos/theos",
                &repo::text(theos),
            ]),
        );
    }
    if repo::output(theos, "git", &strings(&["rev-parse", "HEAD"]))?.trim() == head {
        return Ok(());
    }
    repo::run(
        theos,
        "git",
        &strings(&["pull", "--ff-only", "--recurse-submodules"]),
    )?;
    repo::run(
        theos,
        "git",
        &strings(&["submodule", "update", "--init", "--recursive"]),
    )
}

fn update_sdks(theos: &Path, head: &str) -> Result<(), String> {
    let revision = theos.join("sdks/.revision");
    if std::fs::read_to_string(&revision).is_ok_and(|text| text.trim() == head) {
        return Ok(());
    }
    let work = repo::scratch()?;
    let clone = work.0.join("sdks");
    repo::run(
        &work.0,
        "git",
        &strings(&[
            "clone",
            "--depth",
            "1",
            "https://github.com/theos/sdks",
            &repo::text(&clone),
        ]),
    )?;
    for sdk in files::glob(&theos.join("sdks/*.sdk")) {
        std::fs::remove_dir_all(&sdk).map_err(|error| error.to_string())?;
    }
    for sdk in files::glob(&clone.join("*.sdk")) {
        std::fs::rename(
            &sdk,
            theos.join("sdks").join(sdk.file_name().unwrap_or_default()),
        )
        .map_err(|error| error.to_string())?;
    }
    std::fs::write(revision, format!("{head}\n")).map_err(|error| error.to_string())
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("prepare-theos")
        .string("theos", &environment("THEOS"))
        .parse(arguments)?;
    let theos = flags.get("theos");
    if theos.is_empty() {
        return Err("set --theos or THEOS to the Theos checkout".into());
    }
    let theos_head = remote_head("https://github.com/theos/theos")?;
    let sdks_head = remote_head("https://github.com/theos/sdks")?;
    repo::run(
        &repo::path(&[]),
        "brew",
        &strings(&["install", "ldid", "make"]),
    )?;
    update_theos(Path::new(&theos), &theos_head)?;
    update_sdks(Path::new(&theos), &sdks_head)
}
