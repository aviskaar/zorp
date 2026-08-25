//! The conformance checks.
//!
//! Every check is a pure function of a parsed draft and one rule. Nothing
//! here reads the network, calls a model, or looks at the clock except
//! through the `today` a caller passes in, so every check can be tested
//! directly and adversarially.
//!
//! Two things are deliberate. First, a finding always shows its
//! arithmetic: "too long" without a number tells an author nothing they
//! can act on. Second, a finding carries the provenance of the rule that
//! produced it, so a pass on an unsourced rule reads as a pass on an
//! unsourced rule and not as compliance.

use crate::date::Date;
use crate::manuscript::{Located, Manuscript};
use crate::profile::{CheckKind, Rule, Severity, VenueProfile};
use crate::report::{Finding, Report, Verdict};

/// Everything a check may look at besides the profile.
#[derive(Clone, Debug, Default)]
pub struct Inputs {
    /// Extra terms that identify the authors: names, lab or group names,
    /// product names only they use, handles.
    pub identity: Vec<String>,
    /// A page count measured from a rendered PDF. When present it replaces
    /// the estimate, and the report says the count was measured.
    pub measured_pages: Option<u32>,
    /// Bibliography text, for checking citation keys resolve.
    pub bibliography: Option<String>,
}

/// Run every rule in `profile` against `manuscript`.
pub fn run(
    profile: &VenueProfile,
    manuscript: &Manuscript,
    inputs: &Inputs,
    today: Date,
) -> Report {
    let findings = profile
        .rules
        .iter()
        .map(|rule| {
            let mut finding = check(rule, profile, manuscript, inputs);
            // A rule may be conditional or unsettleable, in which case a
            // hard failure overstates what is known.
            if rule.severity == Severity::Warn && finding.verdict == Verdict::Fail {
                finding.verdict = Verdict::Warn;
            }
            finding
        })
        .collect();
    Report::new(profile, manuscript, findings, today)
}

fn check(
    rule: &Rule,
    profile: &VenueProfile,
    manuscript: &Manuscript,
    inputs: &Inputs,
) -> Finding {
    match rule.check {
        CheckKind::PageLimit => page_limit(rule, profile, manuscript, inputs),
        CheckKind::Anonymity => anonymity(rule, manuscript, inputs),
        CheckKind::RequiredSection => required_section(rule, manuscript),
        CheckKind::AbstractLength => abstract_length(rule, manuscript),
        CheckKind::TitleFormat => title_format(rule, manuscript),
        CheckKind::FigureCaptions => figure_captions(rule, manuscript),
        CheckKind::ReferenceKeys => reference_keys(rule, manuscript, inputs),
        CheckKind::Template => template(rule),
    }
}

// ---------------------------------------------------------------------------
// Page and length limits
// ---------------------------------------------------------------------------

fn page_limit(
    rule: &Rule,
    profile: &VenueProfile,
    manuscript: &Manuscript,
    inputs: &Inputs,
) -> Finding {
    let limit = rule.pages.unwrap_or(0);
    let words = manuscript.counted_words(&rule.excludes);
    let figures = manuscript.figures.len();
    let tables = manuscript.table_count();
    let est = &profile.estimate;

    let mut detail = Vec::new();
    let pages;
    let measured;
    match inputs.measured_pages {
        Some(m) => {
            pages = m;
            measured = true;
            detail.push(format!("Measured {m} pages against a limit of {limit}."));
        }
        None => {
            let from_words = words as f32 / est.words_per_page.max(1) as f32;
            let from_figures = figures as f32 * est.figure_pages;
            let from_tables = tables as f32 * est.table_pages;
            let total = from_words + from_figures + from_tables;
            pages = total.ceil().max(0.0) as u32;
            measured = false;
            detail.push(format!(
                "Estimated {pages} pages against a limit of {limit}."
            ));
            detail.push(format!(
                "{words} words at {} words per page is {from_words:.1} pages, \
                 plus {figures} figures at {:.2} and {tables} tables at {:.2}, \
                 giving {total:.1} pages, rounded up.",
                est.words_per_page, est.figure_pages, est.table_pages
            ));
            detail.push(
                "This is an estimate. It does not know the venue's column width, \
                 font, or how your figures actually place. Render the paper and \
                 re-run with --pages to replace it with a measured count."
                    .to_string(),
            );
        }
    }

    if rule.excludes.is_empty() {
        detail.push("Everything in the draft counts against this limit.".to_string());
    } else {
        let excluded = manuscript.excluded_sections(&rule.excludes);
        let named = if excluded.is_empty() {
            "none found in this draft".to_string()
        } else {
            excluded
                .iter()
                .map(|s| format!("{} (line {})", s.title, s.line))
                .collect::<Vec<_>>()
                .join(", ")
        };
        detail.push(format!(
            "Does not count against the limit: {}. Excluded from this draft: {named}.",
            rule.excludes.join(", ")
        ));
    }

    let mut remedy = Vec::new();
    let verdict = if pages > limit {
        let over = pages - limit;
        let budget = limit * est.words_per_page;
        let word_overage = words.saturating_sub(budget as usize);
        remedy.push(format!(
            "Over by {over} page{}.",
            if over == 1 { "" } else { "s" }
        ));
        if !measured && word_overage > 0 {
            remedy.push(format!(
                "Cutting about {word_overage} words would bring the main text \
                 inside {limit} pages at this estimate's {} words per page.",
                est.words_per_page
            ));
        }
        if !rule.excludes.is_empty() {
            remedy.push(format!(
                "Material moved into any of these does not count: {}.",
                rule.excludes.join(", ")
            ));
        }
        Verdict::Fail
    } else if !measured && limit.saturating_sub(pages) <= 1 {
        remedy.push(format!(
            "Within one page of the limit on an estimate, which is inside this \
             tool's error. Render the paper and re-run with --pages before \
             trusting the {pages} against {limit}."
        ));
        Verdict::Warn
    } else {
        remedy.push(format!(
            "{} pages of headroom.",
            limit.saturating_sub(pages)
        ));
        Verdict::Pass
    };

    Finding::new(rule, verdict, detail.join(" "), remedy)
}

fn abstract_length(rule: &Rule, manuscript: &Manuscript) -> Finding {
    let Some(abstract_text) = &manuscript.front_matter.abstract_text else {
        return Finding::new(
            rule,
            Verdict::Warn,
            "No `abstract:` key in the draft's front matter, so the abstract \
             could not be measured."
                .to_string(),
            vec![
                "Put the abstract in front matter as `abstract: |` so it can be \
                 checked and so the generated manuscript can place it."
                    .to_string(),
            ],
        );
    };
    let chars = abstract_text.value.chars().count();
    let words = crate::manuscript::word_count(&abstract_text.value);
    let mut detail = vec![format!(
        "Abstract at line {} is {chars} characters and {words} words.",
        abstract_text.line
    )];
    let mut remedy = Vec::new();
    let mut verdict = Verdict::Pass;

    if let Some(max) = rule.max_chars {
        if chars > max {
            verdict = Verdict::Fail;
            remedy.push(format!(
                "Cut {} characters: {chars} against a limit of {max}.",
                chars - max
            ));
        } else {
            detail.push(format!(
                "{chars} of {max} characters, {} to spare.",
                max - chars
            ));
        }
    }
    if let Some(max) = rule.max_words {
        if words > max {
            verdict = Verdict::Fail;
            remedy.push(format!(
                "Cut {} words: {words} against a limit of {max}.",
                words - max
            ));
        } else {
            detail.push(format!("{words} of {max} words, {} to spare.", max - words));
        }
    }
    Finding::new(rule, verdict, detail.join(" "), remedy)
}

// ---------------------------------------------------------------------------
// Anonymisation
// ---------------------------------------------------------------------------

/// Hosts whose URLs name an account or an organisation in the first path
/// segment, so linking one from a double-blind submission names you.
const OWNER_NAMING_HOSTS: &[&str] = &[
    "github.com",
    "www.github.com",
    "gist.github.com",
    "gitlab.com",
    "bitbucket.org",
    "codeberg.org",
    "huggingface.co",
    "hf.co",
    "sourceforge.net",
    "colab.research.google.com",
    "drive.google.com",
    "docs.google.com",
    "dropbox.com",
    "www.dropbox.com",
    "kaggle.com",
    "www.kaggle.com",
];

/// Hosts that carry no author identity: archives, DOI resolvers, review
/// systems, venue sites.
const NEUTRAL_HOSTS: &[&str] = &[
    "anonymous.4open.science",
    "doi.org",
    "dx.doi.org",
    "arxiv.org",
    "openreview.net",
    "dl.acm.org",
    "ieeexplore.ieee.org",
    "aclanthology.org",
    "proceedings.mlr.press",
    "papers.nips.cc",
    "neurips.cc",
    "iclr.cc",
    "icml.cc",
    "conf.researchr.org",
    "en.wikipedia.org",
    "creativecommons.org",
];

/// First-person phrasings of a self-citation. The venues that ask for
/// third person say so in these words, and this is the leak that survives
/// stripping the author block.
const FIRST_PERSON_SELF_CITE: &[&str] = &[
    "our previous work",
    "our prior work",
    "our earlier work",
    "our own previous",
    "our own prior",
    "our previous paper",
    "our earlier paper",
    "our prior paper",
    "our recent work",
    "our recent paper",
    "our earlier study",
    "our previous study",
    "in our previous",
    "in our earlier",
    "in our prior",
    "we previously showed",
    "we previously demonstrated",
    "we previously proposed",
    "we previously introduced",
    "we previously reported",
    "we previously found",
    "we previously argued",
    "as we showed in",
    "as we argued in",
    "as we reported in",
    "as we described in",
    "building on our previous",
    "building on our earlier",
    "extending our previous",
    "extending our earlier",
];

/// Phrasings that mark an acknowledgement, which most double-blind venues
/// ask authors to leave out of the submission entirely.
const ACKNOWLEDGEMENT_PHRASES: &[&str] = &[
    "we thank",
    "the authors thank",
    "we are grateful to",
    "we would like to thank",
    "this work was supported by",
    "this work was funded by",
    "this research was supported by",
    "this research was funded by",
    "supported in part by",
    "under grant",
    "grant no",
    "grant number",
];

/// Front matter keys that carry author identity even when the body does
/// not.
const IDENTIFYING_KEYS: &[&str] = &[
    "author",
    "authors",
    "affiliation",
    "affiliations",
    "institute",
    "institution",
    "email",
    "thanks",
    "acknowledgements",
    "acknowledgments",
    "funding",
    "orcid",
];

fn anonymity(rule: &Rule, manuscript: &Manuscript, inputs: &Inputs) -> Finding {
    let terms = identity_terms(manuscript, &inputs.identity);
    let mut hard: Vec<String> = Vec::new();
    let mut soft: Vec<String> = Vec::new();

    // 1. Author identity declared in the front matter.
    for key in IDENTIFYING_KEYS {
        if let Some(value) = manuscript.front_matter.keys.get(*key) {
            if is_anonymous_placeholder(&value.value) {
                continue;
            }
            hard.push(format!(
                "line {}: front matter `{key}:` still names the authors ({:?}). \
                 Replace it with the venue's anonymous author block.",
                value.line,
                truncate(&value.value, 60)
            ));
        }
    }

    // 2. An acknowledgements section, or acknowledgement prose anywhere.
    for section in &manuscript.sections {
        if section.heading_matches("acknowledg") {
            hard.push(format!(
                "line {}: an \"{}\" section. Most double-blind venues ask for \
                 acknowledgements to be left out until camera-ready. Delete it \
                 and restore it after acceptance.",
                section.line, section.title
            ));
        }
    }
    for (line_no, line) in manuscript.body_lines() {
        let lowered = line.to_lowercase();
        for phrase in ACKNOWLEDGEMENT_PHRASES {
            if lowered.contains(phrase) {
                hard.push(format!(
                    "line {line_no}: acknowledgement or funding text (\"{phrase}\"). \
                     Funders and collaborators identify a group as reliably as a \
                     name does. Remove it until camera-ready."
                ));
                break;
            }
        }
    }

    // 3. Self-citation in the first person. A first-person phrase next to a
    //    citation is the leak the venues name outright; the same phrase with
    //    no citation may just be prose, so it is raised rather than failed.
    for (line_no, line) in manuscript.body_lines() {
        let lowered = line.to_lowercase();
        for phrase in FIRST_PERSON_SELF_CITE {
            let Some(at) = lowered.find(phrase) else {
                continue;
            };
            let window = &lowered[at..lowered.len().min(at + phrase.len() + 120)];
            let note = format!(
                "line {line_no}: \"{}\" reads as a self-citation in the first \
                 person. Rewrite it in the third person, as in \"the previous \
                 work of Smith et al. [1]\".",
                truncate(line.trim(), 90)
            );
            if has_citation_marker(window) {
                hard.push(note);
            } else {
                soft.push(note);
            }
            break;
        }
    }

    // 4. URLs. A repository link is the classic double-blind leak.
    let mut unknown_hosts: Vec<String> = Vec::new();
    for url in manuscript.urls() {
        let host = host_of(&url.value);
        let lowered = url.value.to_lowercase();
        let matched_term = terms.iter().find(|t| contains_term(&lowered, t));
        if let Some(term) = matched_term {
            hard.push(format!(
                "line {}: {} contains \"{term}\", which identifies you. Replace \
                 it with an anonymised mirror such as anonymous.4open.science.",
                url.line, url.value
            ));
            continue;
        }
        if NEUTRAL_HOSTS.iter().any(|h| *h == host) {
            continue;
        }
        if OWNER_NAMING_HOSTS.iter().any(|h| *h == host) && has_owner_segment(&url.value) {
            hard.push(format!(
                "line {}: {} names its owner in the URL. A repository or account \
                 link identifies the group even when the name is nowhere else in \
                 the paper. Replace it with an anonymised mirror.",
                url.line, url.value
            ));
            continue;
        }
        if host.ends_with(".github.io") || host.ends_with(".gitlab.io") {
            hard.push(format!(
                "line {}: {} is a personal or organisation pages site; the \
                 subdomain is the account name.",
                url.line, url.value
            ));
            continue;
        }
        unknown_hosts.push(format!("line {}: {}", url.line, url.value));
    }
    if !unknown_hosts.is_empty() {
        soft.push(format!(
            "URLs on hosts this check does not recognise. Each one is worth a \
             look, because a project or lab domain identifies a group: {}",
            unknown_hosts.join("; ")
        ));
    }

    // 5. Email addresses.
    for (line_no, line) in manuscript.body_lines() {
        if let Some(address) = find_email(line) {
            hard.push(format!(
                "line {line_no}: the email address {address} is in the body."
            ));
        }
    }

    // 6. Any identity term, anywhere, including inside code blocks, file
    //    paths, and figure targets. A checked-in path is as identifying as
    //    a byline and survives every other pass over the paper.
    for (line_no, line) in manuscript.body_lines() {
        let lowered = line.to_lowercase();
        for term in &terms {
            if contains_term(&lowered, term) {
                hard.push(format!(
                    "line {line_no}: \"{term}\" appears in the body: {}",
                    truncate(line.trim(), 90)
                ));
                break;
            }
        }
    }

    let mut detail = if terms.is_empty() {
        vec![
            "No identity terms were supplied and the front matter names no \
             author, so the term scan had nothing to look for. Pass --identity \
             with your name, group, and lab to make it useful."
                .to_string(),
        ]
    } else {
        vec![format!(
            "Scanned for {} identity term(s): {}.",
            terms.len(),
            terms.join(", ")
        )]
    };
    detail.push(format!(
        "{} definite leak(s), {} to check by hand.",
        hard.len(),
        soft.len()
    ));

    let verdict = if !hard.is_empty() {
        Verdict::Fail
    } else if !soft.is_empty() {
        Verdict::Warn
    } else {
        Verdict::Pass
    };
    let mut remedy = hard;
    remedy.extend(soft);
    Finding::new(rule, verdict, detail.join(" "), remedy)
}

/// The terms that identify these authors: whatever the caller supplied,
/// plus whatever the draft's own front matter declares. Multi-word names
/// also contribute their long words, so "Ada Lovelace" is caught as
/// "Lovelace et al." too.
fn identity_terms(manuscript: &Manuscript, extra: &[String]) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    let mut push = |value: &str| {
        let v = value.trim();
        if v.is_empty() || is_anonymous_placeholder(v) {
            return;
        }
        let lowered = v.to_lowercase();
        if !terms.contains(&lowered) {
            terms.push(lowered);
        }
        for word in v.split(|c: char| !c.is_alphanumeric()) {
            let w = word.to_lowercase();
            if w.chars().count() >= 4 && !terms.contains(&w) {
                terms.push(w);
            }
        }
    };
    for author in &manuscript.front_matter.authors {
        push(&author.value);
    }
    for affiliation in &manuscript.front_matter.affiliations {
        push(&affiliation.value);
    }
    for term in extra {
        push(term);
    }
    terms
}

fn is_anonymous_placeholder(value: &str) -> bool {
    let v = value.to_lowercase();
    v.contains("anonymous") || v.contains("under review") || v.contains("redacted")
}

/// Whether `haystack` (already lowercased) holds `term` on its own rather
/// than inside a longer word.
fn contains_term(haystack: &str, term: &str) -> bool {
    if term.is_empty() {
        return false;
    }
    let chars: Vec<char> = haystack.chars().collect();
    let needle: Vec<char> = term.chars().collect();
    if needle.len() > chars.len() {
        return false;
    }
    for start in 0..=(chars.len() - needle.len()) {
        if chars[start..start + needle.len()] != needle[..] {
            continue;
        }
        let before_ok = start == 0 || !chars[start - 1].is_alphanumeric();
        let after = start + needle.len();
        let after_ok = after >= chars.len() || !chars[after].is_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

fn has_citation_marker(window: &str) -> bool {
    if window.contains("[@") || window.contains("\\cite") || window.contains("et al") {
        return true;
    }
    // A numeric citation such as [12] or [3, 4].
    let chars: Vec<char> = window.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if *c != '[' {
            continue;
        }
        let mut j = i + 1;
        let mut digits = false;
        while j < chars.len() && chars[j] != ']' {
            if chars[j].is_ascii_digit() {
                digits = true;
            } else if !matches!(chars[j], ',' | ' ' | '-' | ';') {
                digits = false;
                break;
            }
            j += 1;
        }
        if digits && j < chars.len() {
            return true;
        }
    }
    false
}

fn host_of(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    rest.split('/')
        .next()
        .unwrap_or("")
        .split('@')
        .next_back()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_lowercase()
}

/// True when the URL has a non-empty first path segment, which on these
/// hosts is the account or organisation.
fn has_owner_segment(url: &str) -> bool {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    rest.split_once('/')
        .is_some_and(|(_, path)| !path.trim_matches('/').is_empty())
}

fn find_email(line: &str) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if *c != '@' || i == 0 {
            continue;
        }
        if !chars[i - 1].is_ascii_alphanumeric() {
            continue;
        }
        let mut start = i;
        while start > 0 && is_email_local(chars[start - 1]) {
            start -= 1;
        }
        let mut end = i + 1;
        while end < chars.len() && is_email_domain(chars[end]) {
            end += 1;
        }
        let domain: String = chars[i + 1..end].iter().collect();
        if domain.contains('.') && !domain.ends_with('.') && start < i {
            return Some(chars[start..end].iter().collect());
        }
    }
    None
}

fn is_email_local(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+')
}

fn is_email_domain(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '-')
}

fn truncate(text: &str, max: usize) -> String {
    let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.chars().count() <= max {
        return cleaned;
    }
    let head: String = cleaned.chars().take(max).collect();
    format!("{head}...")
}

// ---------------------------------------------------------------------------
// Structure
// ---------------------------------------------------------------------------

fn required_section(rule: &Rule, manuscript: &Manuscript) -> Finding {
    let accepted = rule.headings.join("\", \"");
    let Some(section) = manuscript.find_section(&rule.headings) else {
        return Finding::new(
            rule,
            Verdict::Fail,
            format!("No section heading matches \"{accepted}\"."),
            vec![format!(
                "Add a section whose heading contains one of: \"{accepted}\".{}",
                rule.after
                    .as_ref()
                    .map(|a| format!(" It has to come after the \"{a}\" section."))
                    .unwrap_or_default()
            )],
        );
    };
    let detail = format!(
        "\"{}\" at line {} satisfies this.",
        section.title, section.line
    );
    let Some(after) = &rule.after else {
        return Finding::new(rule, Verdict::Pass, detail, vec![]);
    };
    match manuscript.find_section(std::slice::from_ref(after)) {
        Some(anchor) if section.line < anchor.line => Finding::new(
            rule,
            Verdict::Fail,
            format!(
                "{detail} It is at line {} but \"{}\" is at line {}, so it comes \
                 before it.",
                section.line, anchor.title, anchor.line
            ),
            vec![format!(
                "Move \"{}\" to after \"{}\".",
                section.title, anchor.title
            )],
        ),
        Some(anchor) => Finding::new(
            rule,
            Verdict::Pass,
            format!("{detail} It follows \"{}\" as required.", anchor.title),
            vec![],
        ),
        None => Finding::new(
            rule,
            Verdict::Warn,
            format!("{detail} There is no \"{after}\" section to place it after."),
            vec![format!(
                "The venue asks for this section after \"{after}\", and the draft \
                 has no such section. Check the ordering by hand."
            )],
        ),
    }
}

fn title_format(rule: &Rule, manuscript: &Manuscript) -> Finding {
    let Some(title) = &manuscript.front_matter.title else {
        return Finding::new(
            rule,
            Verdict::Warn,
            "No `title:` key in the draft's front matter.".to_string(),
            vec!["Add `title:` to the front matter so it can be checked.".to_string()],
        );
    };
    let text = &title.value;
    let mut remedy = Vec::new();
    let mut verdict = Verdict::Pass;

    if rule.forbid_all_caps {
        let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
        if !letters.is_empty() && letters.iter().all(|c| c.is_uppercase()) {
            verdict = Verdict::Fail;
            remedy.push(
                "The title is in all capitals. Write it in normal case.".to_string(),
            );
        }
    }
    if rule.forbid_non_ascii {
        let odd: String = text.chars().filter(|c| !c.is_ascii()).collect();
        if !odd.is_empty() {
            verdict = Verdict::Fail;
            remedy.push(format!(
                "The title has non-ASCII characters ({odd}). Spell them out or \
                 use the venue's accepted escapes."
            ));
        }
    }
    if let Some(max) = rule.max_chars {
        let chars = text.chars().count();
        if chars > max {
            verdict = Verdict::Fail;
            remedy.push(format!(
                "The title is {chars} characters against a limit of {max}: cut {}.",
                chars - max
            ));
        }
    }
    Finding::new(
        rule,
        verdict,
        format!(
            "Title at line {}: {:?} ({} characters).",
            title.line,
            text,
            text.chars().count()
        ),
        remedy,
    )
}

fn figure_captions(rule: &Rule, manuscript: &Manuscript) -> Finding {
    if manuscript.figures.is_empty() {
        return Finding::new(
            rule,
            Verdict::Pass,
            "The draft has no figures.".to_string(),
            vec![],
        );
    }
    let mut remedy = Vec::new();
    let mut verdict = Verdict::Pass;

    for figure in &manuscript.figures {
        if figure.caption.trim().is_empty() {
            verdict = Verdict::Fail;
            remedy.push(format!(
                "line {}: the figure {} has no caption.",
                figure.line, figure.target
            ));
        }
    }
    if rule.require_figure_reference {
        let body = manuscript.body().to_lowercase();
        let unreferenced: Vec<usize> = (1..=manuscript.figures.len())
            .filter(|n| {
                !body.contains(&format!("figure {n}")) && !body.contains(&format!("fig. {n}"))
            })
            .collect();
        if !unreferenced.is_empty() {
            verdict = verdict.worse_of(Verdict::Warn);
            remedy.push(format!(
                "No prose refers to figure(s) {}. Numbering here is positional, \
                 so check by hand that every figure is discussed in the text.",
                unreferenced
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if rule.colour_blind_safe {
        verdict = verdict.worse_of(Verdict::Warn);
        remedy.push(
            "This venue asks for colour-blind-safe figures. Nothing in the source \
             text can settle that: check the rendered figures, or the script that \
             generates them, by hand."
                .to_string(),
        );
    }
    Finding::new(
        rule,
        verdict,
        format!("{} figure(s) found.", manuscript.figures.len()),
        remedy,
    )
}

fn reference_keys(rule: &Rule, manuscript: &Manuscript, inputs: &Inputs) -> Finding {
    let mut keys: Vec<Located<String>> = manuscript.citation_keys();
    keys.dedup_by(|a, b| a.value == b.value);
    let Some(bib) = &inputs.bibliography else {
        return Finding::new(
            rule,
            Verdict::NotChecked,
            format!(
                "{} citation key(s) in the draft, and no bibliography to check \
                 them against.",
                keys.len()
            ),
            vec!["Pass --bib <file> to check that every citation key resolves."
                .to_string()],
        );
    };
    let mut missing = Vec::new();
    for key in &keys {
        if !bib_has_key(bib, &key.value) {
            missing.push(format!("line {}: @{}", key.line, key.value));
        }
    }
    if missing.is_empty() {
        Finding::new(
            rule,
            Verdict::Pass,
            format!("All {} citation key(s) resolve in the bibliography.", keys.len()),
            vec![],
        )
    } else {
        Finding::new(
            rule,
            Verdict::Fail,
            format!(
                "{} of {} citation key(s) do not resolve in the bibliography.",
                missing.len(),
                keys.len()
            ),
            missing,
        )
    }
}

fn bib_has_key(bib: &str, key: &str) -> bool {
    for line in bib.lines() {
        let t = line.trim();
        if !t.starts_with('@') {
            continue;
        }
        let Some(open) = t.find('{') else { continue };
        let entry = t[open + 1..].trim().trim_end_matches(',').trim();
        if entry == key {
            return true;
        }
    }
    false
}

fn template(rule: &Rule) -> Finding {
    let mut remedy = Vec::new();
    if let Some(class) = &rule.documentclass {
        remedy.push(format!("Use exactly: {class}"));
    }
    if let Some(url) = &rule.style_url {
        remedy.push(format!("Style files: {url}"));
    }
    remedy.push(
        "Nothing in a markdown draft can show which document class the PDF was \
         built with. `zorp-agent conform --emit-tex` writes a manuscript with \
         this class already set."
            .to_string(),
    );
    Finding::new(
        rule,
        Verdict::NotChecked,
        "The document class cannot be read off a markdown draft.".to_string(),
        remedy,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn term_matching_respects_word_boundaries() {
        assert!(contains_term("github.com/aviskaar/zorp", "aviskaar"));
        assert!(contains_term("aviskaar's lab", "aviskaar"));
        assert!(!contains_term("aviskaarium research", "aviskaar"));
        assert!(!contains_term("preaviskaar", "aviskaar"));
    }

    #[test]
    fn citation_markers_are_recognised_in_several_styles() {
        assert!(has_citation_marker("our previous work [@zorp2026] showed"));
        assert!(has_citation_marker("our previous work [12] showed"));
        assert!(has_citation_marker("our previous work [3, 4] showed"));
        assert!(has_citation_marker("our previous work \\cite{x}"));
        assert!(has_citation_marker("our previous work (Smith et al. 2020)"));
        assert!(!has_citation_marker("our previous work showed that caching helps"));
    }

    #[test]
    fn emails_are_found_but_citation_keys_are_not() {
        assert_eq!(
            find_email("write to ada@example.ac.uk today"),
            Some("ada@example.ac.uk".to_string())
        );
        assert_eq!(find_email("as shown in [@smith2020]"), None);
        assert_eq!(find_email("no address here"), None);
    }

    #[test]
    fn host_and_owner_segment_are_read_off_a_url() {
        assert_eq!(host_of("https://github.com/acme/thing"), "github.com");
        assert!(has_owner_segment("https://github.com/acme/thing"));
        assert!(!has_owner_segment("https://github.com/"));
        assert!(!has_owner_segment("https://github.com"));
    }
}
