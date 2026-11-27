use crate::flags::Flags;
use crate::repo;

const ALPHA_TAGS: &str = "v*-alpha.*";
const FALLBACK_COMMITS: &str = "30";
const SECTIONS: [(&str, &str); 3] = [("feat", "Added"), ("fix", "Fixed"), ("perf", "Faster")];

pub struct Change<'a> {
    pub kind: &'a str,
    pub scope: Option<&'a str>,
    pub summary: &'a str,
}

pub fn parse(subject: &str) -> Option<Change<'_>> {
    let (head, summary) = subject.split_once(": ")?;
    let head = head.trim_end_matches('!');
    let (kind, scope) = match head.split_once('(') {
        Some((kind, rest)) => (kind, Some(rest.trim_end_matches(')'))),
        None => (head, None),
    };
    let valid = !kind.is_empty() && kind.chars().all(|character| character.is_ascii_lowercase());
    valid.then_some(Change {
        kind,
        scope,
        summary: summary.trim(),
    })
}

fn line(change: &Change) -> String {
    match change.scope {
        Some(scope) => format!("- {scope}: {}", change.summary),
        None => format!("- {}", change.summary),
    }
}

pub fn notes(subjects: &[String]) -> String {
    let changes: Vec<Change> = subjects
        .iter()
        .filter_map(|subject| parse(subject))
        .collect();
    let mut out = Vec::new();
    for (kind, title) in SECTIONS {
        let lines: Vec<String> = changes
            .iter()
            .filter(|change| change.kind == kind)
            .map(line)
            .collect();
        if !lines.is_empty() {
            out.push(format!("### {title}\n\n{}", lines.join("\n")));
        }
    }
    let internal = changes
        .iter()
        .filter(|change| SECTIONS.iter().all(|(kind, _)| *kind != change.kind))
        .count();
    if internal > 0 {
        out.push(format!(
            "{internal} internal changes (refactors, builds, CI, docs)."
        ));
    }
    if out.is_empty() {
        return "No user-facing changes since the last alpha.".to_owned();
    }
    out.join("\n\n")
}

fn previous_alpha() -> Option<String> {
    let tag = repo::output(
        &repo::path(&[]),
        "git",
        &[
            "describe",
            "--tags",
            "--abbrev=0",
            "--match",
            ALPHA_TAGS,
            "HEAD^",
        ]
        .map(str::to_owned),
    )
    .ok()?;
    let tag = tag.trim();
    (!tag.is_empty()).then(|| tag.to_owned())
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("release-notes")
        .string("since", "")
        .parse(arguments)?;
    let since = Some(flags.get("since"))
        .filter(|since| !since.is_empty())
        .or_else(previous_alpha);
    let mut log = vec![
        "log".to_owned(),
        "--no-merges".to_owned(),
        "--format=%s".to_owned(),
    ];
    match since {
        Some(since) => log.push(format!("{since}..HEAD")),
        None => log.extend(["-n".to_owned(), FALLBACK_COMMITS.to_owned()]),
    }
    let subjects: Vec<String> = repo::output(&repo::path(&[]), "git", &log)?
        .lines()
        .map(str::to_owned)
        .collect();
    println!("{}", notes(&subjects));
    Ok(())
}
