use crate::repo::{self, strings};

pub fn is_workflow_yaml(path: &str) -> bool {
    let extension = repo::extension(path);
    ((extension == "yml" || extension == "yaml") && path.starts_with(".github/"))
        || path == ".yamlfmt.yml"
}

fn yaml_files() -> Result<Vec<String>, String> {
    repo::tracked_files(&[".github/*.yml", ".github/*.yaml"])
}

fn absolute(files: &[String]) -> Vec<String> {
    files
        .iter()
        .map(|file| repo::text(&repo::path(&[file])))
        .collect()
}

fn yamlfmt(lint: bool, files: &[String]) -> Result<(), String> {
    let mut arguments = strings(&["-conf", &repo::text(&repo::path(&[".yamlfmt.yml"]))]);
    if lint {
        arguments.push("-lint".into());
    }
    arguments.extend(absolute(files));
    repo::run(&repo::path(&[]), "yamlfmt", &arguments)
}

pub fn lint(files: &[String]) -> Result<(), String> {
    let workflows: Vec<String> = files
        .iter()
        .filter(|file| file.starts_with(".github/workflows/"))
        .cloned()
        .collect();
    if !workflows.is_empty() {
        repo::run(&repo::path(&[]), "actionlint", &absolute(&workflows))?;
        repo::run(
            &repo::path(&[]),
            "zizmor",
            &strings(&["--offline", "--quiet", ".github/workflows"]),
        )?;
    }
    yamlfmt(true, files)?;
    space(files, true)
}

pub fn lint_all() -> Result<(), String> {
    let files = yaml_files()?;
    if files.is_empty() {
        Ok(())
    } else {
        lint(&files)
    }
}

pub fn format_all() -> Result<(), String> {
    let files = yaml_files()?;
    if files.is_empty() {
        return Ok(());
    }
    yamlfmt(false, &files)?;
    space(&files, false)
}

struct Block {
    child_indent: usize,
    seen: bool,
    item: bool,
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn is_block_child(line: &str, block: &Block) -> bool {
    let trimmed = line.trim();
    if indent_of(line) != block.child_indent || trimmed.starts_with('#') {
        return false;
    }
    if block.item {
        trimmed.starts_with("- ")
    } else {
        trimmed.ends_with(':') || trimmed.contains(": ")
    }
}

fn with_blank_before_comments(out: &mut Vec<String>) {
    let mut at = out.len();
    while at > 0 && out[at - 1].trim().starts_with('#') {
        at -= 1;
    }
    if at == 0 || out[at - 1].trim().is_empty() {
        return;
    }
    out.insert(at, String::new());
}

pub fn space_workflow(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut blocks: Vec<Block> = Vec::new();
    let mut pending: Option<(usize, bool)> = None;
    for line in text.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            out.push(line.to_owned());
            continue;
        }
        let indent = indent_of(line);
        if let Some((opener, item)) = pending.take()
            && indent > opener
        {
            blocks.push(Block {
                child_indent: indent,
                seen: false,
                item,
            });
        }
        while blocks
            .last()
            .is_some_and(|block| indent < block.child_indent)
        {
            blocks.pop();
        }
        if let Some(block) = blocks.last_mut()
            && is_block_child(line, block)
        {
            if block.seen {
                with_blank_before_comments(&mut out);
            }
            block.seen = true;
        }
        match trimmed {
            "steps:" => pending = Some((indent, true)),
            "jobs:" => pending = Some((indent, false)),
            _ => {}
        }
        out.push(line.to_owned());
    }
    out.join("\n")
}

fn space(files: &[String], check: bool) -> Result<(), String> {
    for file in files
        .iter()
        .filter(|file| file.starts_with(".github/workflows/"))
    {
        let path = repo::path(&[file]);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("failed to read workflow {file}: {error}"))?;
        let spaced = space_workflow(&text);
        if spaced == text {
            continue;
        }
        if check {
            return Err(format!(
                "{file}: separate each job and step with a blank line (run astral format)"
            ));
        }
        std::fs::write(&path, spaced)
            .map_err(|error| format!("failed to write workflow {file}: {error}"))?;
    }
    Ok(())
}
