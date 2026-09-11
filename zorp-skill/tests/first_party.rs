//! The skills that ship with this repository.
//!
//! `.claude/skills/` here is a real skills directory: `zorp`, `zorp-agent`
//! and `zorp-web` all discover it when they run in this workspace. So the
//! files in it are code in the sense that matters, and a `SKILL.md` that
//! stops parsing takes its skill off every one of those surfaces with
//! nothing but a warning to say so.
//!
//! These tests are about the two things that break silently: frontmatter
//! that no longer parses, and a description that no longer says when to use
//! the skill. Neither is caught by anything else.

use std::path::{Path, PathBuf};

fn skills_dir() -> PathBuf {
    // From `zorp-skill/` up to the workspace root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("zorp-skill has a parent")
        .join(".claude")
        .join("skills")
}

fn discovered() -> zorp_skill::SkillRegistry {
    let (registry, warnings) = zorp_skill::SkillRegistry::discover(&[skills_dir()]);
    assert!(
        warnings.is_empty(),
        "a first-party skill did not parse: {warnings:?}"
    );
    registry
}

#[test]
fn the_first_party_skills_are_discoverable() {
    let registry = discovered();
    for name in ["artifact-design", "artifact-diagramming"] {
        assert!(
            registry.get(name).is_some(),
            "{name} is missing: found {:?}",
            registry.names()
        );
    }
}

/// A description is the whole of what the model sees before it picks a
/// skill, so it has to say when the skill applies rather than what it is
/// about. One that does not is a skill that never gets chosen.
#[test]
fn every_first_party_description_says_when_to_use_it() {
    for skill in discovered().iter() {
        let description = skill.description.to_lowercase();
        assert!(
            description.contains("use ") || description.contains("when "),
            "{} does not say when to use it: {:?}",
            skill.name,
            skill.description
        );
    }
}

/// The directory name is the invocation name. A frontmatter `name` that
/// disagrees with it is a skill somebody will try to load by the wrong one.
#[test]
fn declared_names_agree_with_their_directories() {
    for skill in discovered().iter() {
        if let Some(declared) = &skill.declared_name {
            assert_eq!(
                declared, &skill.name,
                "{} declares a different name",
                skill.name
            );
        }
    }
}

/// A skill grants nothing, and `allowed-tools` is parsed, reported and
/// ignored. A first-party skill asking for tools would be asking for
/// something zorp does not give, and saying so in the repository's own
/// skills would be confusing rather than merely ignored.
#[test]
fn no_first_party_skill_asks_for_tools() {
    for skill in discovered().iter() {
        assert!(
            skill.declared_tools.is_empty(),
            "{} declares allowed-tools, which zorp parses and ignores: {:?}",
            skill.name,
            skill.declared_tools
        );
    }
}

/// The pane serves `.html` and `.svg` under a bare `sandbox` CSP, so
/// scripts do not run and nothing external loads. Both artifact skills have
/// to say so, because a page written against the opposite assumption
/// renders as nothing and says nothing about why.
#[test]
fn the_artifact_skills_say_that_scripts_do_not_run() {
    let registry = discovered();
    for name in ["artifact-design", "artifact-diagramming"] {
        let body = registry.get(name).expect("present").body.to_lowercase();
        assert!(
            body.contains("script"),
            "{name} does not mention scripts at all"
        );
        assert!(
            body.contains("do not run") || body.contains("does not run"),
            "{name} does not say scripts do not run"
        );
    }
}
