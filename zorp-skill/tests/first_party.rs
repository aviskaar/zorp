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

const FIRST_PARTY: &[&str] = &[
    "artifact-design",
    "artifact-diagramming",
    "landing-page",
    "react-components",
];

fn first_party_skills() -> Vec<zorp_skill::Skill> {
    let registry = discovered();
    FIRST_PARTY
        .iter()
        .map(|name| {
            registry
                .get(name)
                .unwrap_or_else(|| panic!("{name} is missing"))
                .clone()
        })
        .collect()
}

#[test]
fn the_first_party_skills_are_discoverable() {
    let registry = discovered();
    for name in FIRST_PARTY {
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
    for skill in first_party_skills() {
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
    for skill in first_party_skills() {
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
    for skill in first_party_skills() {
        assert!(
            skill.declared_tools.is_empty(),
            "{} declares allowed-tools, which zorp parses and ignores: {:?}",
            skill.name,
            skill.declared_tools
        );
    }
}

/// The pane serves `.html` and `.svg` under a bare `sandbox` CSP, so
/// scripts do not run. Every skill that writes into the pane has to say
/// so, because a page written against the opposite assumption renders as
/// nothing and says nothing about why.
#[test]
fn the_artifact_skills_say_that_scripts_do_not_run() {
    let registry = discovered();
    for name in FIRST_PARTY {
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

/// A bare `sandbox` does not block an external font, stylesheet or image.
///
/// It stops scripts and form submission. It does not stop a `<link>` to a
/// font service or an `<img>` from a CDN, so a skill that tells a writer
/// the header will keep those out has told them a page is private when it
/// is not: it previews correctly in the pane while reporting every opener
/// to a third party. Keeping them out is a rule the skills state and
/// nothing enforces, and saying which is which is the whole value of
/// saying it at all. This pins the wording against drifting back.
#[test]
fn no_skill_claims_the_sandbox_blocks_external_loads() {
    let registry = discovered();
    for name in FIRST_PARTY {
        let body = registry.get(name).expect("present").body.to_lowercase();
        for claim in [
            "nothing external loads",
            "does not load the cdn",
            "blocks external",
            "no external requests are made",
        ] {
            assert!(
                !body.contains(claim),
                "{name} says {claim:?}, which a bare sandbox does not do"
            );
        }
    }
}

/// The obvious way to preview JSX is a transpiler from a CDN in a
/// `<script type="text/babel">` block, and in this pane it renders nothing
/// and says nothing about why. A skill that talks about JSX at all has to
/// name that trap, or the first thing anybody tries is the thing that does
/// not work.
#[test]
fn the_landing_skill_names_the_babel_in_the_page_trap() {
    let body = discovered()
        .get("landing-page")
        .expect("present")
        .body
        .to_lowercase();
    assert!(body.contains("text/babel"), "the trap is not named");
    assert!(
        body.contains("jsx"),
        "the path to components is not covered"
    );
}

/// A landing page is a deliverable that goes on a real server, where
/// scripts run. Saying only "scripts do not run" would be wrong about the
/// shipped page and would talk somebody out of writing a correct one, so
/// the skill has to separate the preview from the page.
#[test]
fn the_landing_skill_separates_the_preview_from_the_shipped_page() {
    let body = discovered()
        .get("landing-page")
        .expect("present")
        .body
        .to_lowercase();
    assert!(
        body.contains("preview"),
        "the skill never mentions the preview"
    );
    assert!(
        body.contains("real server") || body.contains("real browser"),
        "the skill does not say the shipped page is different"
    );
}

/// The component skill is the one a transpiler in the page is most likely to
/// be tried from, so it names the trap too, and it has to say that the pane
/// cannot preview what it produces and that zorp does not build it. A skill
/// that let a model promise a preview would produce a blank pane and a
/// confused person.
#[test]
fn the_component_skill_names_the_trap_and_what_zorp_cannot_do() {
    let body = discovered()
        .get("react-components")
        .expect("present")
        .body
        .to_lowercase();
    assert!(body.contains("text/babel"), "the trap is not named");
    assert!(
        body.contains("cannot preview"),
        "the skill does not say the pane cannot preview it"
    );
    assert!(
        body.contains("no build step"),
        "the skill does not say zorp does not build it"
    );
}

/// The two authoring skills split rather than duplicate. The component
/// skill builds on `landing-page` and says so, and `landing-page` points at
/// the component skill only conditionally, because a small model is not
/// offered it and must not be told to load something it cannot see.
#[test]
fn the_authoring_skills_refer_to_each_other_without_duplicating() {
    let registry = discovered();
    let react = registry.get("react-components").expect("present");
    assert!(
        react.description.contains("landing-page"),
        "{}",
        react.description
    );
    assert!(react.body.contains("`landing-page`"));
    // The long lists live in one place. The component skill does not carry
    // its own copy of the section order or the generated page tells.
    assert!(!react.body.contains("purple to pink"));
    assert!(!react.body.contains("header    a wordmark"));

    let landing = registry.get("landing-page").expect("present").body.clone();
    assert!(
        landing.contains("`react-components` skill, when it\nis in your list"),
        "landing-page should mention the component skill only as conditional"
    );
}
