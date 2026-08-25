//! Conformance checking end to end: the arithmetic a report has to show,
//! and the honesty machinery around an unverified or stale profile.

use zorp_venue::check::{self, Inputs};
use zorp_venue::profile::{self, ProfileLayer};
use zorp_venue::report::Verdict;
use zorp_venue::{Date, Manuscript, VenueProfile};

fn today() -> Date {
    Date::parse("2026-08-18").unwrap()
}

fn empty_dirs() -> (tempfile::TempDir, tempfile::TempDir) {
    (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap())
}

fn builtin(id: &str) -> VenueProfile {
    let (home, cwd) = empty_dirs();
    profile::load(home.path(), cwd.path(), id).unwrap()
}

/// A draft of roughly `words` words, with no figures and no tables.
fn draft_of(words: usize) -> String {
    let body = std::iter::repeat_n("word", words)
        .collect::<Vec<_>>()
        .join(" ");
    format!("---\ntitle: \"T\"\nauthor: \"Anonymous Author(s)\"\n---\n\n# Body\n\n{body}\n")
}

fn finding<'a>(report: &'a zorp_venue::Report, rule_id: &str) -> &'a zorp_venue::Finding {
    report
        .findings
        .iter()
        .find(|f| f.rule_id == rule_id)
        .unwrap_or_else(|| panic!("no finding for rule {rule_id}"))
}

// ---------------------------------------------------------------------------
// Page limit arithmetic
// ---------------------------------------------------------------------------

#[test]
fn a_page_limit_failure_says_by_how_much_and_how_much_to_cut() {
    // ICML 2026 allows 8 pages and this profile estimates 550 words per
    // page, so 8 pages is a 4400 word budget. 6600 words is 12 pages.
    let profile = builtin("icml-2026");
    let manuscript = Manuscript::parse(&draft_of(6600));
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());
    let f = finding(&report, "main-text-page-limit");

    assert_eq!(f.verdict, Verdict::Fail);
    assert!(
        f.detail.contains("Estimated 12 pages against a limit of 8"),
        "detail should show the estimate and the limit: {}",
        f.detail
    );
    let remedy = f.remedy.join(" ");
    assert!(
        remedy.contains("Over by 4 pages"),
        "should say by how much: {remedy}"
    );
    assert!(
        remedy.contains("2200 words"),
        "should say how many words to cut: {remedy}"
    );
}

#[test]
fn a_measured_page_count_replaces_the_estimate_and_says_so() {
    let profile = builtin("icml-2026");
    let manuscript = Manuscript::parse(&draft_of(6600));
    let inputs = Inputs {
        measured_pages: Some(7),
        ..Inputs::default()
    };
    let report = check::run(&profile, &manuscript, &inputs, today());
    let f = finding(&report, "main-text-page-limit");

    assert_eq!(
        f.verdict,
        Verdict::Pass,
        "a measured 7 pages beats an estimate of 12"
    );
    assert!(
        f.detail.contains("Measured 7 pages against a limit of 8"),
        "detail should say the count was measured: {}",
        f.detail
    );
    assert!(
        !f.detail.contains("Estimated"),
        "a measured count should not also be presented as an estimate: {}",
        f.detail
    );
}

#[test]
fn an_estimate_within_one_page_of_the_limit_is_a_warning_not_a_pass() {
    // 8 pages at 550 words per page is 4400 words. 4000 words estimates to
    // 8 pages, exactly the limit, which is inside this tool's error.
    let profile = builtin("icml-2026");
    let manuscript = Manuscript::parse(&draft_of(4000));
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());
    let f = finding(&report, "main-text-page-limit");

    assert_eq!(f.verdict, Verdict::Warn);
    assert!(
        f.remedy.join(" ").contains("--pages"),
        "should say how to replace the estimate with a measurement: {:?}",
        f.remedy
    );
}

#[test]
fn a_measured_count_exactly_on_the_limit_passes() {
    let profile = builtin("icml-2026");
    let manuscript = Manuscript::parse(&draft_of(4000));
    let inputs = Inputs {
        measured_pages: Some(8),
        ..Inputs::default()
    };
    let report = check::run(&profile, &manuscript, &inputs, today());
    assert_eq!(finding(&report, "main-text-page-limit").verdict, Verdict::Pass);
}

#[test]
fn excluded_sections_do_not_count_and_the_report_names_them() {
    let profile = builtin("icml-2026");
    let long_appendix = std::iter::repeat_n("word", 6000)
        .collect::<Vec<_>>()
        .join(" ");
    let draft = format!(
        "---\ntitle: \"T\"\n---\n\n# Body\n\nshort main text\n\n# Appendix\n\n{long_appendix}\n"
    );
    let manuscript = Manuscript::parse(&draft);
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());
    let f = finding(&report, "main-text-page-limit");

    assert_eq!(
        f.verdict,
        Verdict::Pass,
        "6000 words of appendix should not blow an 8 page main-text limit: {}",
        f.detail
    );
    assert!(
        f.detail.contains("Appendix (line"),
        "should name the excluded section and where it starts: {}",
        f.detail
    );
}

#[test]
fn figures_and_tables_are_counted_into_the_estimate() {
    let profile = builtin("icml-2026");
    let plain = Manuscript::parse(&draft_of(2200));
    let with_art = Manuscript::parse(&format!(
        "{}\n![A figure.](f.png)\n\n| a | b |\n|---|---|\n| 1 | 2 |\n",
        draft_of(2200)
    ));
    let a = check::run(&profile, &plain, &Inputs::default(), today());
    let b = check::run(&profile, &with_art, &Inputs::default(), today());
    let pages = |r: &zorp_venue::Report| {
        finding(r, "main-text-page-limit")
            .detail
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse::<u32>()
            .unwrap()
    };
    assert!(
        pages(&b) > pages(&a),
        "a figure and a table should cost pages: {} vs {}",
        pages(&a),
        pages(&b)
    );
}

// ---------------------------------------------------------------------------
// The honesty machinery
// ---------------------------------------------------------------------------

#[test]
fn an_unverified_rule_is_marked_in_the_report_even_when_it_passes() {
    // ICLR 2027 has published no author guide, so its page limit carries
    // no source. A pass on that rule must not read as compliance.
    let profile = builtin("iclr-2027");
    let manuscript = Manuscript::parse(&draft_of(500));
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());
    let f = finding(&report, "main-text-page-limit");

    assert_eq!(f.verdict, Verdict::Pass);
    assert!(!f.verified, "the rule cites no source, so it is unverified");
    assert!(report.counts().unverified > 0);

    let markdown = report.to_markdown();
    assert!(
        markdown.contains("PASS (UNVERIFIED)"),
        "an unverified pass must be labelled: {markdown}"
    );
    assert!(
        markdown.contains("cite no source"),
        "the report should say what unverified means: {markdown}"
    );
    assert!(
        report
            .caveats()
            .iter()
            .any(|c| c.contains("cannot tell you that you comply")),
        "the caveats should refuse to claim compliance: {:?}",
        report.caveats()
    );
}

#[test]
fn a_verified_rule_carries_its_source_and_quote_into_the_report() {
    let profile = builtin("fse-2027");
    let manuscript = Manuscript::parse(&draft_of(500));
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());
    let f = finding(&report, "main-text-page-limit");

    assert!(f.verified);
    assert_eq!(
        f.source.as_deref(),
        Some("https://conf.researchr.org/track/fse-2027/fse-2027-papers")
    );
    let markdown = report.to_markdown();
    assert!(
        markdown.contains("no more than 18 pages for all text and figures"),
        "the source's own words should be in the report: {markdown}"
    );
    assert!(
        markdown.contains("checked 2026-08-18"),
        "the date the source was read should be in the report: {markdown}"
    );
}

#[test]
fn a_stale_profile_warns_rather_than_pretending_to_be_current() {
    let profile = ProfileLayer::parse(
        "id = \"old\"\nname = \"Old Venue\"\nchecked = \"2025-01-01\"\n\
         stale_after_days = 180\n\
         [[rules]]\nid = \"pages\"\ncheck = \"page_limit\"\nrequirement = \"eight pages\"\n\
         pages = 8\nsource = \"https://example.test/cfp\"\nquote = \"eight pages\"",
    )
    .unwrap()
    .finish(vec!["test".to_string()])
    .unwrap();
    let manuscript = Manuscript::parse(&draft_of(100));
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());

    let caveats = report.caveats().join(" ");
    assert!(
        caveats.contains("594 days ago"),
        "should say exactly how old it is: {caveats}"
    );
    assert!(
        caveats.contains("past its 180 day staleness limit"),
        "should say what the limit was: {caveats}"
    );
    assert!(report.to_markdown().contains("STALE"));
}

#[test]
fn a_closed_cycle_is_reported_as_closed() {
    // NeurIPS 2026's deadline was 2026-05-06, before the date this report
    // is being run on.
    let profile = builtin("neurips-2026");
    let manuscript = Manuscript::parse(&draft_of(100));
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());

    let caveats = report.caveats().join(" ");
    assert!(
        caveats.contains("104 days ago"),
        "should count the days since the deadline: {caveats}"
    );
    assert!(
        caveats.contains("cycle that has closed"),
        "should say the cycle closed: {caveats}"
    );
    assert!(report.to_markdown().contains("PASSED 104 days ago"));
}

#[test]
fn an_open_cycle_says_how_long_is_left() {
    let profile = builtin("fse-2027");
    let manuscript = Manuscript::parse(&draft_of(100));
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());
    assert!(
        report.to_markdown().contains("45 days away"),
        "FSE 2027 closes 2026-10-02, 45 days after 2026-08-18: {}",
        report.to_markdown()
    );
    assert!(
        !report.caveats().iter().any(|c| c.contains("has closed")),
        "an open cycle should not warn about a closed one"
    );
}

#[test]
fn a_check_that_could_not_run_is_not_counted_as_a_pass() {
    let profile = ProfileLayer::parse(
        "id = \"v\"\nname = \"V\"\nchecked = \"2026-08-18\"\n\
         [[rules]]\nid = \"refs\"\ncheck = \"reference_keys\"\n\
         requirement = \"citations must resolve\"\nsource = \"https://example.test\"\n\
         quote = \"citations must resolve\"",
    )
    .unwrap()
    .finish(vec!["test".to_string()])
    .unwrap();
    let manuscript = Manuscript::parse("# Body\n\nAs shown in [@nosuch2020].\n");
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());

    assert_eq!(finding(&report, "refs").verdict, Verdict::NotChecked);
    assert_eq!(report.counts().pass, 0);
    assert!(report
        .caveats()
        .iter()
        .any(|c| c.contains("They are not passes")));
}

#[test]
fn a_dangling_citation_key_is_a_failure_once_a_bibliography_is_supplied() {
    let profile = ProfileLayer::parse(
        "id = \"v\"\nname = \"V\"\nchecked = \"2026-08-18\"\n\
         [[rules]]\nid = \"refs\"\ncheck = \"reference_keys\"\n\
         requirement = \"citations must resolve\"\nsource = \"https://example.test\"\n\
         quote = \"citations must resolve\"",
    )
    .unwrap()
    .finish(vec!["test".to_string()])
    .unwrap();
    let manuscript = Manuscript::parse("# Body\n\nAs in [@known2020] and [@missing2021].\n");
    let inputs = Inputs {
        bibliography: Some("@article{known2020,\n  title = {A},\n}\n".to_string()),
        ..Inputs::default()
    };
    let report = check::run(&profile, &manuscript, &inputs, today());
    let f = finding(&report, "refs");
    assert_eq!(f.verdict, Verdict::Fail);
    assert!(
        f.remedy.join(" ").contains("@missing2021"),
        "should name the key that does not resolve: {:?}",
        f.remedy
    );
}

// ---------------------------------------------------------------------------
// Required sections
// ---------------------------------------------------------------------------

#[test]
fn a_missing_required_section_says_what_to_add_and_where() {
    let profile = builtin("fse-2027");
    let manuscript = Manuscript::parse(&draft_of(200));
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());
    let f = finding(&report, "data-availability");

    assert_eq!(f.verdict, Verdict::Fail);
    let remedy = f.remedy.join(" ");
    assert!(remedy.contains("Data Availability"), "{remedy}");
    assert!(
        remedy.contains("after the \"Conclusion\" section"),
        "should say where it goes: {remedy}"
    );
}

#[test]
fn a_required_section_in_the_wrong_place_fails_on_its_ordering() {
    let profile = builtin("fse-2027");
    let manuscript = Manuscript::parse(
        "---\ntitle: \"T\"\n---\n\n# Data Availability\n\nA package exists.\n\n\
         # Conclusion\n\nIt worked.\n",
    );
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());
    let f = finding(&report, "data-availability");

    assert_eq!(f.verdict, Verdict::Fail);
    assert!(
        f.detail.contains("comes before it"),
        "should say the ordering is wrong: {}",
        f.detail
    );
}

#[test]
fn a_required_section_matches_a_longer_heading_that_contains_it() {
    let profile = builtin("icml-2026");
    let manuscript = Manuscript::parse(
        "---\ntitle: \"T\"\n---\n\n# Body\n\ntext\n\n# Broader Impact and Ethics\n\ntext\n",
    );
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());
    assert_eq!(finding(&report, "impact-statement").verdict, Verdict::Pass);
}

#[test]
fn a_warn_severity_rule_never_escalates_to_a_failure() {
    // ICLR 2027's reproducibility statement is optional as far as anyone
    // here knows, so its absence must not read as a desk rejection.
    let profile = builtin("iclr-2027");
    let manuscript = Manuscript::parse(&draft_of(200));
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());
    assert_eq!(
        finding(&report, "reproducibility-statement").verdict,
        Verdict::Warn
    );
}

// ---------------------------------------------------------------------------
// Abstract and title
// ---------------------------------------------------------------------------

#[test]
fn an_over_long_abstract_says_how_many_characters_to_cut() {
    let profile = builtin("arxiv");
    let long = "x".repeat(2000);
    let manuscript = Manuscript::parse(&format!(
        "---\ntitle: \"T\"\nabstract: |\n  {long}\n---\n\n# Body\n\ntext\n"
    ));
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());
    let f = finding(&report, "abstract-length");
    assert_eq!(f.verdict, Verdict::Fail);
    assert!(
        f.remedy.join(" ").contains("Cut 80 characters"),
        "2000 against 1920 is 80 over: {:?}",
        f.remedy
    );
}

#[test]
fn an_abstract_inside_the_limit_reports_the_headroom() {
    let profile = builtin("arxiv");
    let manuscript = Manuscript::parse(
        "---\ntitle: \"T\"\nabstract: |\n  Twelve characters.\n---\n\n# Body\n\ntext\n",
    );
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());
    let f = finding(&report, "abstract-length");
    assert_eq!(f.verdict, Verdict::Pass);
    assert!(
        f.detail.contains("to spare"),
        "should say how much room is left: {}",
        f.detail
    );
}

#[test]
fn an_all_capitals_title_is_refused_by_arxiv() {
    let profile = builtin("arxiv");
    let manuscript =
        Manuscript::parse("---\ntitle: \"A STUDY OF CACHING\"\n---\n\n# Body\n\ntext\n");
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());
    let f = finding(&report, "title-format");
    assert_eq!(f.verdict, Verdict::Fail);
    assert!(f.remedy.join(" ").contains("all capitals"));
}

#[test]
fn a_non_ascii_title_is_refused_by_arxiv() {
    let profile = builtin("arxiv");
    let manuscript = Manuscript::parse("---\ntitle: \"Caching in Zürich\"\n---\n\n# Body\n\nt\n");
    let report = check::run(&profile, &manuscript, &Inputs::default(), today());
    let f = finding(&report, "title-format");
    assert_eq!(f.verdict, Verdict::Fail);
    assert!(f.remedy.join(" ").contains('ü'));
}

// ---------------------------------------------------------------------------
// The generated manuscript
// ---------------------------------------------------------------------------

#[test]
fn the_generated_manuscript_uses_the_venues_document_class() {
    let profile = builtin("fse-2027");
    let manuscript = Manuscript::parse(
        "---\ntitle: \"A Study\"\nauthor: \"Ada Lovelace\"\nabstract: |\n  Short.\n---\n\n\
         # Introduction\n\nText with a **bold** word and a citation [@smith2020].\n",
    );
    let tex = zorp_venue::latex::emit(&manuscript, &profile);

    assert!(
        tex.contains("\\documentclass[acmsmall,screen,review,anonymous]{acmart}"),
        "should set the venue's class: {tex}"
    );
    assert!(tex.contains("\\title{A Study}"));
    assert!(
        tex.contains("Anonymous Author(s)"),
        "a double-anonymous venue gets an anonymous author block: {tex}"
    );
    assert!(
        !tex.contains("\\author{Ada Lovelace}"),
        "the real author block must not survive: {tex}"
    );
    assert!(tex.contains("\\section{Introduction}"));
    assert!(tex.contains("\\textbf{bold}"));
    assert!(tex.contains("\\citep{smith2020}"));
    assert!(
        tex.contains("\\section{Data Availability}"),
        "a missing required section should be stubbed in: {tex}"
    );
}

#[test]
fn the_generated_manuscript_names_the_source_of_every_rule() {
    let profile = builtin("iclr-2027");
    let manuscript = Manuscript::parse("---\ntitle: \"T\"\n---\n\n# Body\n\ntext\n");
    let tex = zorp_venue::latex::emit(&manuscript, &profile);
    assert!(
        tex.contains("https://iclr.cc/Conferences/2027/CallForPapers"),
        "sourced rules should name their source in the header: {tex}"
    );
    assert!(
        tex.contains("NO SOURCE, this requirement is unverified"),
        "unsourced rules should say so in the header: {tex}"
    );
}

#[test]
fn a_non_blind_venue_keeps_the_real_author_block() {
    let profile = builtin("arxiv");
    let manuscript =
        Manuscript::parse("---\ntitle: \"T\"\nauthor: \"Ada Lovelace\"\n---\n\n# Body\n\nt\n");
    let tex = zorp_venue::latex::emit(&manuscript, &profile);
    assert!(tex.contains("\\author{Ada Lovelace}"), "{tex}");
    assert!(!tex.contains("Anonymous Author(s)"));
}

#[test]
fn the_generated_manuscript_does_not_scrub_identity_out_of_the_body() {
    // Silently deleting the leak would remove the evidence the conformance
    // report is pointing at, and the author would never learn about it.
    let profile = builtin("fse-2027");
    let manuscript = Manuscript::parse(
        "---\ntitle: \"T\"\nauthor: \"Ada Lovelace\"\n---\n\n# Body\n\n\
         See the repo at https://github.com/aviskaar/zorp for details.\n",
    );
    let tex = zorp_venue::latex::emit(&manuscript, &profile);
    assert!(
        tex.contains("github.com/aviskaar/zorp"),
        "the body leak should survive into the tex, so the report stays true: {tex}"
    );
    assert!(
        tex.contains("does not scrub identity"),
        "and the header should say that is deliberate: {tex}"
    );
}
