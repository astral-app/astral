use crate::repo;

const BOX_OPEN: &str = "@(";
const BOOL_CAST: &str = "(BOOL)";
const COMPARISONS: [&str; 6] = ["&&", "||", "==", "!=", "<=", ">="];

#[derive(Debug, PartialEq)]
pub struct Finding {
    pub line: usize,
    pub text: String,
}

fn boxed_expression(source: &str, start: usize) -> Option<&str> {
    let mut depth = 1usize;
    for (offset, character) in source[start..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&source[start..start + offset]);
                }
            }
            _ => {}
        }
    }
    None
}

fn top_level(expression: &str) -> String {
    let mut depth = 0usize;
    expression
        .chars()
        .map(|character| {
            match character {
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                _ => {}
            }
            if depth == 0 { character } else { ' ' }
        })
        .collect()
}

fn is_boolean(expression: &str) -> bool {
    let trimmed = expression.trim();
    if trimmed.starts_with(BOOL_CAST) {
        return false;
    }
    let outer = top_level(trimmed);
    if outer.contains('?') {
        return false;
    }
    COMPARISONS.iter().any(|operator| outer.contains(operator))
        || outer.contains(" < ")
        || outer.contains(" > ")
        || outer.trim_start().starts_with('!')
}

pub fn findings(source: &str) -> Vec<Finding> {
    source
        .match_indices(BOX_OPEN)
        .filter_map(|(index, _)| {
            let expression = boxed_expression(source, index + BOX_OPEN.len())?;
            is_boolean(expression).then(|| Finding {
                line: source[..index].matches('\n').count() + 1,
                text: format!("{BOX_OPEN}{expression})"),
            })
        })
        .collect()
}

pub fn check(files: &[String]) -> Result<(), String> {
    let mut problems = Vec::new();
    for file in files {
        let source = repo::read(&repo::path(&[file]))?;
        for finding in findings(&source) {
            problems.push(format!(
                "{file}:{}: {} boxes an int, so JSON gets 1 or 0; cast it with @((BOOL)(...))",
                finding.line, finding.text
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!("objc boolean boxing:\n{}", problems.join("\n")))
    }
}
