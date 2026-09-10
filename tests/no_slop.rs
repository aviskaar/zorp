//! The prose in this repository is written by people, and reads like it.
//!
//! An em dash or an en dash is the single most reliable tell that a
//! sentence came out of a model, and this repository is largely written
//! with one. So the living text holds neither, and this test is what keeps
//! that true after the next hundred commits rather than for a week.
//!
//! Living text means anything a person or a model reads today: source
//! comments, the strings the CLI prints, the instruction files, and the
//! documents that describe how zorp works now.
//!
//! Four things are exempt, and each for its own reason rather than for
//! convenience:
//!
//! - `docs/upstream-quecto/` and `docs/UPSTREAM_QUECTO_README.md` are the
//!   inherited record of the project this one was forked from. Editing
//!   somebody else's document to change how it reads makes it a worse
//!   record of what they wrote.
//! - `docs/DECISIONS.md` says in its own opening that entries are never
//!   rewritten, so that a reader can see what was believed at the time.
//!   That rule does not have a punctuation exception.
//! - `docs/superpowers/` and `docs/uat/` are dated plans, specs and run
//!   reports. Same reason: they say what was planned or what happened on a
//!   day, and a document that is quietly edited afterwards stops being
//!   evidence of either.
//! - Eval fixtures under `evals/` and `zorp-eval/evals/` are inputs to a
//!   measurement. Changing a prompt changes what the eval measures, which
//!   is a different thing from tidying prose.
//!
//! Everywhere else, use a comma, a full stop, a colon or a pair of
//! parentheses. There is always one that reads better.

use std::path::{Path, PathBuf};

/// Directories and files whose text is a record rather than living prose.
const EXEMPT: &[&str] = &[
    "docs/upstream-quecto",
    "docs/UPSTREAM_QUECTO_README.md",
    "docs/DECISIONS.md",
    "docs/superpowers",
    "docs/uat",
    "docs/assets",
    "evals",
    "zorp-eval/evals",
];

/// Never walked: build output, dependencies, and git's own storage.
const SKIP_DIRS: &[&str] = &["target", "node_modules", ".git", "dist", "dist-site"];

/// Extensions worth checking. A binary or a lockfile has no prose in it.
const TEXT: &[&str] = &[
    "rs", "ts", "js", "md", "toml", "css", "html", "sh", "py", "yml", "yaml",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn is_exempt(relative: &Path) -> bool {
    let text = relative.to_string_lossy().replace('\\', "/");
    EXEMPT
        .iter()
        .any(|e| text == *e || text.starts_with(&format!("{e}/")))
}

fn walk(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            let relative = path.strip_prefix(root).unwrap_or(&path);
            if is_exempt(relative) {
                continue;
            }
            walk(&path, root, out);
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        if is_exempt(&relative) {
            continue;
        }
        let checkable = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| TEXT.contains(&e))
            || name == ".gitignore";
        if checkable {
            out.push(path);
        }
    }
}

#[test]
fn the_living_text_holds_no_em_or_en_dashes() {
    let root = repo_root();
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    assert!(
        files.len() > 50,
        "the walk found almost nothing, so it is not checking what it thinks it is"
    );

    let mut offences = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            if line.contains('\u{2014}') || line.contains('\u{2013}') {
                let shown = line.trim();
                let shown: String = shown.chars().take(100).collect();
                offences.push(format!(
                    "{}:{}: {shown}",
                    path.strip_prefix(&root).unwrap_or(path).display(),
                    number + 1
                ));
            }
        }
    }

    assert!(
        offences.is_empty(),
        "an em dash or en dash reached the living text. Use a comma, a full \
         stop, a colon or parentheses instead:\n{}",
        offences.join("\n")
    );
}

/// The exemptions have to name things that exist, or this test slowly stops
/// covering the repository while continuing to pass.
#[test]
fn every_exemption_still_points_at_something() {
    let root = repo_root();
    for exempt in EXEMPT {
        assert!(
            root.join(exempt).exists(),
            "{exempt} is exempted and is not there. Remove the exemption \
             rather than leaving a hole in the check."
        );
    }
}
