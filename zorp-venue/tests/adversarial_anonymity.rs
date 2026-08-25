//! Attempts to sneak author identity past the anonymisation check.
//!
//! Every case here hides the identity somewhere other than the author
//! block, because stripping the author block is the part everybody
//! remembers. These are the leaks that survive it.

use zorp_venue::check::{self, Inputs};
use zorp_venue::profile::ProfileLayer;
use zorp_venue::report::Verdict;
use zorp_venue::{Date, Manuscript, VenueProfile};

fn blind_venue() -> VenueProfile {
    ProfileLayer::parse(
        r#"
id      = "test-blind"
name    = "Test Blind Venue"
checked = "2026-08-18"

[[rules]]
id          = "double-blind-anonymity"
check       = "anonymity"
requirement = "The submission must not reveal author identity."
source      = "https://example.test/cfp"
quote       = "Submissions are double blind."
"#,
    )
    .unwrap()
    .finish(vec!["test".to_string()])
    .unwrap()
}

fn today() -> Date {
    Date::parse("2026-08-18").unwrap()
}

/// Run the anonymity rule and return (verdict, every remedy line joined).
fn scan(draft: &str, identity: &[&str]) -> (Verdict, String) {
    let manuscript = Manuscript::parse(draft);
    let inputs = Inputs {
        identity: identity.iter().map(|s| s.to_string()).collect(),
        ..Inputs::default()
    };
    let report = check::run(&blind_venue(), &manuscript, &inputs, today());
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "double-blind-anonymity")
        .expect("the anonymity rule should have produced a finding");
    (finding.verdict, finding.remedy.join("\n"))
}

const CLEAN: &str = r#"---
title: "A Study of Caching"
author: "Anonymous Author(s)"
abstract: |
  We measured a cache.
---

# Introduction

The previous work of Smith et al. [@smith2020] measured a different cache.
Code is at <https://anonymous.4open.science/r/cache-1234>.

# Conclusion

Caching helped.
"#;

#[test]
fn a_properly_anonymised_draft_passes() {
    let (verdict, remedy) = scan(CLEAN, &["Ada Lovelace", "Aviskaar"]);
    assert_eq!(
        verdict,
        Verdict::Pass,
        "the clean draft should pass, but got: {remedy}"
    );
}

#[test]
fn a_name_only_in_an_acknowledgement_is_caught() {
    // The author block is anonymous. The name is one line, in a section
    // most authors forget to strip until camera-ready.
    let draft = CLEAN.replace(
        "# Conclusion",
        "# Acknowledgements\n\nWe thank Ada Lovelace for the analytical engine.\n\n# Conclusion",
    );
    let (verdict, remedy) = scan(&draft, &["Ada Lovelace"]);
    assert_eq!(verdict, Verdict::Fail, "remedy was: {remedy}");
    assert!(
        remedy.to_lowercase().contains("acknowledg"),
        "should name the acknowledgement section: {remedy}"
    );
    assert!(
        remedy.contains("ada lovelace") || remedy.contains("Ada Lovelace"),
        "should point at the name itself: {remedy}"
    );
}

#[test]
fn an_acknowledgement_with_no_names_is_still_caught() {
    // Funders identify a group as reliably as a name does.
    let draft = CLEAN.replace(
        "Caching helped.",
        "Caching helped.\n\nThis work was supported by grant no. 12345.",
    );
    let (verdict, remedy) = scan(&draft, &[]);
    assert_eq!(verdict, Verdict::Fail, "remedy was: {remedy}");
    assert!(
        remedy.contains("funding") || remedy.contains("acknowledgement"),
        "should say why funding text is a leak: {remedy}"
    );
}

#[test]
fn a_self_citation_in_the_first_person_is_caught() {
    // No name anywhere. The giveaway is the pronoun next to the citation.
    let draft = CLEAN.replace(
        "The previous work of Smith et al. [@smith2020] measured a different cache.",
        "In our previous work [@smith2020] we measured a different cache.",
    );
    let (verdict, remedy) = scan(&draft, &[]);
    assert_eq!(verdict, Verdict::Fail, "remedy was: {remedy}");
    assert!(
        remedy.contains("third person"),
        "should say to rewrite it in the third person: {remedy}"
    );
}

#[test]
fn a_first_person_phrase_with_no_citation_is_raised_not_failed() {
    // Same pronoun, no citation. Might be ordinary prose, so it is a
    // warning: over-failing this would make authors stop reading.
    let draft = CLEAN.replace(
        "Caching helped.",
        "Caching helped. We previously showed that eviction dominates.",
    );
    let (verdict, remedy) = scan(&draft, &[]);
    assert_eq!(verdict, Verdict::Warn, "remedy was: {remedy}");
    assert!(remedy.contains("third person"), "remedy was: {remedy}");
}

#[test]
fn a_third_person_self_citation_is_left_alone() {
    let (verdict, remedy) = scan(CLEAN, &[]);
    assert_eq!(
        verdict,
        Verdict::Pass,
        "third person phrasing is what the venues ask for: {remedy}"
    );
}

#[test]
fn a_repository_url_that_names_the_group_is_caught() {
    let draft = CLEAN.replace(
        "https://anonymous.4open.science/r/cache-1234",
        "https://github.com/aviskaar/zorp",
    );
    let (verdict, remedy) = scan(&draft, &["Aviskaar"]);
    assert_eq!(verdict, Verdict::Fail, "remedy was: {remedy}");
    assert!(
        remedy.contains("anonymous.4open.science"),
        "should suggest an anonymised mirror: {remedy}"
    );
}

#[test]
fn a_repository_url_is_caught_even_when_the_owner_is_not_a_known_identity_term() {
    // The author never said "quantum-lab" identifies them. The URL still
    // names an owner, and that is enough at a double-blind venue.
    let draft = CLEAN.replace(
        "https://anonymous.4open.science/r/cache-1234",
        "https://gitlab.com/quantum-lab/cachebench",
    );
    let (verdict, remedy) = scan(&draft, &[]);
    assert_eq!(verdict, Verdict::Fail, "remedy was: {remedy}");
    assert!(
        remedy.contains("names its owner"),
        "should say why an owner segment leaks: {remedy}"
    );
}

#[test]
fn a_personal_pages_site_is_caught_through_its_subdomain() {
    let draft = CLEAN.replace(
        "https://anonymous.4open.science/r/cache-1234",
        "https://alovelace.github.io/cachebench",
    );
    let (verdict, remedy) = scan(&draft, &[]);
    assert_eq!(verdict, Verdict::Fail, "remedy was: {remedy}");
    assert!(
        remedy.contains("subdomain"),
        "should say the subdomain is the account: {remedy}"
    );
}

#[test]
fn a_filename_in_a_code_listing_is_caught() {
    // A path pasted out of a terminal. Nothing else in the paper names the
    // group, and a scanner that skips fenced blocks misses it entirely.
    let draft = CLEAN.replace(
        "Caching helped.",
        "Caching helped.\n\n```text\n$ ls /home/build/aviskaar/zorp/target\n```\n",
    );
    let (verdict, remedy) = scan(&draft, &["Aviskaar"]);
    assert_eq!(verdict, Verdict::Fail, "remedy was: {remedy}");
    assert!(
        remedy.contains("aviskaar"),
        "should quote the offending line: {remedy}"
    );
}

#[test]
fn a_figure_path_that_names_the_group_is_caught() {
    let draft = CLEAN.replace(
        "Caching helped.",
        "Caching helped.\n\n![The cache.](figures/aviskaar-cache.png)\n",
    );
    let (verdict, remedy) = scan(&draft, &["Aviskaar"]);
    assert_eq!(verdict, Verdict::Fail, "remedy was: {remedy}");
}

#[test]
fn an_email_address_in_the_body_is_caught() {
    let draft = CLEAN.replace(
        "Caching helped.",
        "Caching helped. Write to cachebench@example.ac.uk for the dataset.",
    );
    let (verdict, remedy) = scan(&draft, &[]);
    assert_eq!(verdict, Verdict::Fail, "remedy was: {remedy}");
    assert!(
        remedy.contains("cachebench@example.ac.uk"),
        "should quote the address: {remedy}"
    );
}

#[test]
fn a_citation_key_is_not_mistaken_for_an_email_address() {
    // `[@smith2020]` has an at-sign in it and must not read as a leak.
    let (verdict, remedy) = scan(CLEAN, &[]);
    assert_eq!(verdict, Verdict::Pass, "remedy was: {remedy}");
}

#[test]
fn a_surname_alone_is_caught_when_the_full_name_was_supplied() {
    let draft = CLEAN.replace(
        "Caching helped.",
        "Caching helped, as the Lovelace group has argued for years.",
    );
    let (verdict, remedy) = scan(&draft, &["Ada Lovelace"]);
    assert_eq!(verdict, Verdict::Fail, "remedy was: {remedy}");
}

#[test]
fn an_identity_term_inside_a_longer_word_is_not_a_false_positive() {
    let draft = CLEAN.replace(
        "Caching helped.",
        "Caching helped, and so did the lovelacement heuristic.",
    );
    let (verdict, remedy) = scan(&draft, &["Lovelace"]);
    assert_eq!(
        verdict,
        Verdict::Pass,
        "a term inside another word should not fire: {remedy}"
    );
}

#[test]
fn an_author_block_that_is_still_filled_in_is_caught() {
    let draft = CLEAN.replace("author: \"Anonymous Author(s)\"", "author: \"Ada Lovelace\"");
    let (verdict, remedy) = scan(&draft, &[]);
    assert_eq!(verdict, Verdict::Fail, "remedy was: {remedy}");
    assert!(
        remedy.contains("front matter"),
        "should point at the front matter: {remedy}"
    );
}

#[test]
fn an_affiliation_left_in_the_front_matter_is_caught() {
    let draft = CLEAN.replace(
        "author: \"Anonymous Author(s)\"",
        "author: \"Anonymous Author(s)\"\naffiliation: \"Analytical Engine Lab\"",
    );
    let (verdict, remedy) = scan(&draft, &[]);
    assert_eq!(verdict, Verdict::Fail, "remedy was: {remedy}");
    assert!(
        remedy.contains("affiliation"),
        "should name the offending key: {remedy}"
    );
}

#[test]
fn a_url_on_an_unrecognised_host_is_raised_for_a_human() {
    // A project domain may or may not identify a group. Silence would be
    // the wrong answer; so would a hard failure.
    let draft = CLEAN.replace(
        "https://anonymous.4open.science/r/cache-1234",
        "https://cachebench.dev/downloads",
    );
    let (verdict, remedy) = scan(&draft, &[]);
    assert_eq!(verdict, Verdict::Warn, "remedy was: {remedy}");
    assert!(
        remedy.contains("cachebench.dev"),
        "should list the URL to look at: {remedy}"
    );
}

#[test]
fn every_leak_is_reported_with_the_line_it_is_on() {
    let draft = CLEAN.replace(
        "Caching helped.",
        "Caching helped.\n\nWe thank Ada Lovelace.\n\nSee https://github.com/aviskaar/zorp.",
    );
    let (verdict, remedy) = scan(&draft, &["Ada Lovelace", "Aviskaar"]);
    assert_eq!(verdict, Verdict::Fail);
    for line in remedy.lines().filter(|l| !l.starts_with("URLs on hosts")) {
        assert!(
            line.contains("line "),
            "every leak needs a line number, this one had none: {line}"
        );
    }
}
