//! Venue profiles: what a venue requires, where each requirement came
//! from, and when it was last checked.
//!
//! Profiles are data. Adding a venue is a TOML file, never a code change.
//! Built-in profiles ship in `profiles/`; a user layers their own on top
//! the same way flavors layer, user scope then project scope.
//!
//! The provenance fields are the point of this module. A rule with no
//! `source` is unverified, and a report says so rather than presenting a
//! guess as a requirement. A confident wrong page limit is worse than no
//! page limit at all, because it tells an author they are compliant on
//! their way to a desk rejection.

use crate::date::Date;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Every built-in profile, as (id, TOML text). The id is the file stem and
/// must match the `id` inside the file; `builtin_ids_match_their_contents`
/// holds that.
pub const BUILTIN: &[(&str, &str)] = &[
    ("arxiv", include_str!("../profiles/arxiv.toml")),
    ("fse-2027", include_str!("../profiles/fse-2027.toml")),
    ("iclr-2027", include_str!("../profiles/iclr-2027.toml")),
    ("icml-2026", include_str!("../profiles/icml-2026.toml")),
    ("neurips-2026", include_str!("../profiles/neurips-2026.toml")),
];

/// Which check a rule asks for. The parameter fields a rule sets depend on
/// this.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    /// A page budget for the main text, with an explicit list of what does
    /// not count against it.
    PageLimit,
    /// Double-blind anonymisation.
    Anonymity,
    /// A section the venue requires, matched by heading.
    RequiredSection,
    /// A character or word budget for the abstract.
    AbstractLength,
    /// Title conventions: capitalisation, character set, length.
    TitleFormat,
    /// Figures have captions, are referred to, and meet any stated
    /// accessibility ask.
    FigureCaptions,
    /// Every inline citation key resolves in the bibliography.
    ReferenceKeys,
    /// The required document class or style file.
    Template,
}

/// How loudly a rule fails. `Fail` is the default: a violation that would
/// plausibly get the paper desk-rejected. `Warn` is for a requirement that
/// is conditional, optional, or that a machine cannot settle.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    #[default]
    Fail,
    Warn,
}

/// One venue requirement, with its provenance attached.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// Stable identifier. A higher layer replaces a lower layer's rule of
    /// the same id, wholesale, provenance included.
    pub id: String,
    pub check: CheckKind,
    /// The requirement in the venue's own terms, one sentence.
    pub requirement: String,
    #[serde(default)]
    pub severity: Severity,

    // Provenance. `source` is load bearing: a rule without one is
    // unverified and every finding derived from it says so.
    /// URL of the page this rule was read off.
    pub source: Option<String>,
    /// ISO date the source was read. Falls back to the profile's date.
    pub checked: Option<String>,
    /// The sentence from the source that states the requirement.
    pub quote: Option<String>,
    /// Anything a reader needs to know that the quote does not say. This
    /// is where an unverified rule explains itself.
    pub note: Option<String>,

    // Parameters, by check kind.
    /// `page_limit`: the budget, in pages.
    pub pages: Option<u32>,
    /// `page_limit`: what does not count against it, as section-name
    /// fragments matched case-insensitively.
    #[serde(default)]
    pub excludes: Vec<String>,
    /// `required_section`: heading fragments that satisfy the rule.
    #[serde(default)]
    pub headings: Vec<String>,
    /// `required_section`: a heading fragment this section must follow.
    pub after: Option<String>,
    /// `abstract_length` / `title_format`: character budget.
    pub max_chars: Option<usize>,
    /// `abstract_length`: word budget.
    pub max_words: Option<usize>,
    /// `title_format`: reject an all-uppercase title.
    #[serde(default)]
    pub forbid_all_caps: bool,
    /// `title_format`: reject characters outside ASCII.
    #[serde(default)]
    pub forbid_non_ascii: bool,
    /// `figure_captions`: every figure must be referred to in the prose.
    #[serde(default)]
    pub require_figure_reference: bool,
    /// `figure_captions`: the venue asks for colour-blind-safe figures.
    /// Nothing in the source text can settle this, so it is raised for a
    /// human rather than passed.
    #[serde(default)]
    pub colour_blind_safe: bool,
    /// `template`: the exact document class line to use.
    pub documentclass: Option<String>,
    /// `template`: where the style files live.
    pub style_url: Option<String>,
}

impl Rule {
    /// True when this rule cites a source. A rule that does not is a
    /// guess, and is reported as one.
    pub fn is_verified(&self) -> bool {
        self.source
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
    }
}

/// A submission deadline, with its own provenance.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deadline {
    pub date: String,
    /// Which deadline this is, e.g. "full paper submission".
    pub label: String,
    pub source: Option<String>,
}

/// The page-count model. Not a venue rule and never sourced: it is this
/// tool's arithmetic for turning words into pages when no rendered page
/// count is available. Every estimate prints these numbers alongside it so
/// the author can see what the guess rests on.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Estimate {
    pub words_per_page: u32,
    pub figure_pages: f32,
    pub table_pages: f32,
}

impl Default for Estimate {
    fn default() -> Self {
        Estimate {
            words_per_page: 550,
            figure_pages: 0.3,
            table_pages: 0.25,
        }
    }
}

/// One profile file, before layering. Every scalar is optional so a user
/// layer can set one field without restating the venue.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileLayer {
    pub id: Option<String>,
    pub name: Option<String>,
    pub kind: Option<String>,
    /// ISO date the profile as a whole was last checked against sources.
    pub checked: Option<String>,
    /// Days after `checked` at which this profile is called stale.
    pub stale_after_days: Option<i64>,
    pub deadline: Option<Deadline>,
    pub estimate: Option<Estimate>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl ProfileLayer {
    pub fn parse(text: &str) -> Result<ProfileLayer, ProfileError> {
        toml::from_str(text).map_err(|e| ProfileError::Parse(e.to_string()))
    }

    /// Merge `over` on top of `self`. Scalars set in `over` win. Rules
    /// merge by id: same id replaces, new id appends. Replacement is
    /// wholesale, so an override that omits `source` becomes unverified,
    /// which is the honest outcome for a hand-edited requirement.
    pub fn merge(mut self, over: ProfileLayer) -> ProfileLayer {
        for rule in over.rules {
            match self.rules.iter_mut().find(|r| r.id == rule.id) {
                Some(slot) => *slot = rule,
                None => self.rules.push(rule),
            }
        }
        ProfileLayer {
            id: over.id.or(self.id),
            name: over.name.or(self.name),
            kind: over.kind.or(self.kind),
            checked: over.checked.or(self.checked),
            stale_after_days: over.stale_after_days.or(self.stale_after_days),
            deadline: over.deadline.or(self.deadline),
            estimate: over.estimate.or(self.estimate),
            rules: self.rules,
        }
    }

    /// Turn merged layers into a usable profile, failing on anything a
    /// check would otherwise have to guess at.
    pub fn finish(self, layers: Vec<String>) -> Result<VenueProfile, ProfileError> {
        let id = self.id.ok_or(ProfileError::MissingField("id"))?;
        let name = self.name.ok_or(ProfileError::MissingField("name"))?;
        let checked_text = self.checked.ok_or(ProfileError::MissingField("checked"))?;
        let checked = Date::parse(&checked_text).map_err(ProfileError::BadDate)?;
        if let Some(d) = &self.deadline {
            Date::parse(&d.date).map_err(ProfileError::BadDate)?;
        }
        for rule in &self.rules {
            if let Some(c) = &rule.checked {
                Date::parse(c).map_err(ProfileError::BadDate)?;
            }
            validate_rule(rule)?;
        }
        Ok(VenueProfile {
            id,
            name,
            kind: self.kind.unwrap_or_else(|| "venue".to_string()),
            checked,
            stale_after_days: self.stale_after_days.unwrap_or(180),
            deadline: self.deadline,
            estimate: self.estimate.unwrap_or_default(),
            rules: self.rules,
            layers,
        })
    }
}

/// Reject a rule that cannot do its job. A `page_limit` with no `pages` is
/// worse than no rule: it would report a silent pass.
fn validate_rule(rule: &Rule) -> Result<(), ProfileError> {
    let missing = |field: &str| {
        Err(ProfileError::BadRule(format!(
            "rule '{}' is a {:?} check but sets no {field}",
            rule.id, rule.check
        )))
    };
    match rule.check {
        CheckKind::PageLimit if rule.pages.is_none() => missing("pages"),
        CheckKind::RequiredSection if rule.headings.is_empty() => missing("headings"),
        CheckKind::AbstractLength if rule.max_chars.is_none() && rule.max_words.is_none() => {
            missing("max_chars or max_words")
        }
        CheckKind::Template if rule.documentclass.is_none() && rule.style_url.is_none() => {
            missing("documentclass or style_url")
        }
        _ => Ok(()),
    }
}

/// A venue's requirements, ready to check against.
#[derive(Clone, Debug)]
pub struct VenueProfile {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub checked: Date,
    pub stale_after_days: i64,
    pub deadline: Option<Deadline>,
    pub estimate: Estimate,
    pub rules: Vec<Rule>,
    /// Where this profile came from, low to high precedence. Printed in
    /// the report so a surprising rule can be traced to the file that set
    /// it.
    pub layers: Vec<String>,
}

impl VenueProfile {
    /// How stale this profile is, as of `today`.
    pub fn freshness(&self, today: Date) -> Freshness {
        let age_days = today.days_since(self.checked);
        if age_days > self.stale_after_days {
            Freshness::Stale {
                age_days,
                limit_days: self.stale_after_days,
            }
        } else {
            Freshness::Fresh { age_days }
        }
    }

    /// Whether the cycle this profile describes is still open, as of
    /// `today`.
    pub fn cycle(&self, today: Date) -> CycleState {
        let Some(deadline) = &self.deadline else {
            return CycleState::NoDeadline;
        };
        let Ok(date) = Date::parse(&deadline.date) else {
            return CycleState::NoDeadline;
        };
        let days = date.days_since(today);
        if days < 0 {
            CycleState::Closed {
                days_ago: -days,
                date,
                label: deadline.label.clone(),
            }
        } else {
            CycleState::Open {
                days_left: days,
                date,
                label: deadline.label.clone(),
            }
        }
    }

    /// How many rules carry no source.
    pub fn unverified_rules(&self) -> usize {
        self.rules.iter().filter(|r| !r.is_verified()).count()
    }
}

/// How long ago the profile was checked against its sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    Fresh { age_days: i64 },
    Stale { age_days: i64, limit_days: i64 },
}

/// Whether the submission cycle this profile describes has closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CycleState {
    NoDeadline,
    Open {
        days_left: i64,
        date: Date,
        label: String,
    },
    Closed {
        days_ago: i64,
        date: Date,
        label: String,
    },
}

/// Anything that can go wrong loading a profile.
#[derive(Debug)]
pub enum ProfileError {
    Parse(String),
    MissingField(&'static str),
    BadDate(String),
    BadRule(String),
    UnknownVenue { id: String, known: Vec<String> },
    BadId(String),
    Io(std::io::Error),
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileError::Parse(m) => write!(f, "venue profile does not parse: {m}"),
            ProfileError::MissingField(field) => {
                write!(f, "venue profile is missing required field '{field}'")
            }
            ProfileError::BadDate(m) => write!(f, "venue profile has a bad date: {m}"),
            ProfileError::BadRule(m) => write!(f, "venue profile has an unusable rule: {m}"),
            ProfileError::UnknownVenue { id, known } => write!(
                f,
                "no venue profile '{id}'. Known profiles: {}. Add your own at \
                 ~/.config/zorp/venues/{id}.toml or .zorp/venues/{id}.toml",
                known.join(", ")
            ),
            ProfileError::BadId(id) => write!(
                f,
                "invalid venue id '{id}': must be a single path component, no '/' or '..'"
            ),
            ProfileError::Io(e) => write!(f, "venue profile could not be read: {e}"),
        }
    }
}

impl std::error::Error for ProfileError {}

impl From<std::io::Error> for ProfileError {
    fn from(e: std::io::Error) -> Self {
        ProfileError::Io(e)
    }
}

/// True if `id` is a single normal path component. Same rule flavor names
/// follow, and for the same reason: a venue id becomes a filename.
pub fn is_valid_id(id: &str) -> bool {
    let mut components = Path::new(id).components();
    matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(_)), None)
    )
}

/// The directories holding user venue profiles, low to high precedence.
/// `home` and `cwd` are injected so this is testable.
pub fn layer_dirs(home: &Path, cwd: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".config").join("zorp").join("venues"),
        cwd.join(".zorp").join("venues"),
    ]
}

/// Load a profile by id: the built-in one if there is one, then each user
/// layer merged on top.
pub fn load(home: &Path, cwd: &Path, id: &str) -> Result<VenueProfile, ProfileError> {
    if !is_valid_id(id) {
        return Err(ProfileError::BadId(id.to_string()));
    }
    let mut merged: Option<ProfileLayer> = None;
    let mut layers = Vec::new();

    if let Some((_, text)) = BUILTIN.iter().find(|(name, _)| *name == id) {
        merged = Some(ProfileLayer::parse(text)?);
        layers.push("built-in".to_string());
    }
    for dir in layer_dirs(home, cwd) {
        let path = dir.join(format!("{id}.toml"));
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let layer = ProfileLayer::parse(&text)?;
                merged = Some(match merged {
                    Some(base) => base.merge(layer),
                    None => layer,
                });
                layers.push(path.display().to_string());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(ProfileError::Io(e)),
        }
    }

    match merged {
        Some(layer) => layer.finish(layers),
        None => Err(ProfileError::UnknownVenue {
            id: id.to_string(),
            known: list(home, cwd),
        }),
    }
}

/// Every profile id available here, built-in and user-supplied, sorted.
pub fn list(home: &Path, cwd: &Path) -> Vec<String> {
    let mut ids: BTreeSet<String> = BUILTIN.iter().map(|(id, _)| id.to_string()).collect();
    for dir in layer_dirs(home, cwd) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "toml") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    ids.insert(stem.to_string());
                }
            }
        }
    }
    ids.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs() -> (tempfile::TempDir, tempfile::TempDir) {
        (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap())
    }

    #[test]
    fn every_builtin_profile_parses_and_finishes() {
        for (id, text) in BUILTIN {
            let layer = ProfileLayer::parse(text)
                .unwrap_or_else(|e| panic!("{id} should parse: {e}"));
            let profile = layer
                .finish(vec!["built-in".to_string()])
                .unwrap_or_else(|e| panic!("{id} should finish: {e}"));
            assert_eq!(&profile.id, id, "profile id should match its file name");
            assert!(!profile.rules.is_empty(), "{id} should carry rules");
        }
    }

    #[test]
    fn every_builtin_rule_states_its_requirement_and_names_its_provenance() {
        for (id, text) in BUILTIN {
            let profile = ProfileLayer::parse(text)
                .unwrap()
                .finish(vec!["built-in".to_string()])
                .unwrap();
            for rule in &profile.rules {
                assert!(
                    !rule.requirement.trim().is_empty(),
                    "{id}/{} should state its requirement",
                    rule.id
                );
                // Either it cites a source, or it explains in a note why it
                // cannot. Silence is the one thing not allowed.
                assert!(
                    rule.is_verified() || rule.note.is_some(),
                    "{id}/{} has no source, so it must carry a note saying why",
                    rule.id
                );
                if rule.is_verified() {
                    assert!(
                        rule.quote.is_some(),
                        "{id}/{} cites a source, so it should quote it",
                        rule.id
                    );
                }
            }
        }
    }

    #[test]
    fn unknown_key_in_a_profile_is_rejected() {
        // A typo must not read as "this venue has no page limit".
        assert!(ProfileLayer::parse("id = \"x\"\npage_limit = 9").is_err());
        assert!(ProfileLayer::parse(
            "id = \"x\"\n[[rules]]\nid = \"r\"\ncheck = \"page_limit\"\nrequirement = \"r\"\npagez = 9"
        )
        .is_err());
    }

    #[test]
    fn a_page_limit_rule_without_a_page_count_is_refused() {
        let layer = ProfileLayer::parse(
            "id = \"x\"\nname = \"X\"\nchecked = \"2026-08-18\"\n\
             [[rules]]\nid = \"r\"\ncheck = \"page_limit\"\nrequirement = \"nine pages\"",
        )
        .unwrap();
        let err = layer.finish(vec![]).unwrap_err();
        assert!(
            matches!(err, ProfileError::BadRule(_)),
            "expected BadRule, got {err:?}"
        );
    }

    #[test]
    fn merge_replaces_a_rule_by_id_and_appends_new_ones() {
        let base = ProfileLayer::parse(
            "id = \"v\"\nname = \"V\"\nchecked = \"2026-01-01\"\n\
             [[rules]]\nid = \"pages\"\ncheck = \"page_limit\"\nrequirement = \"nine\"\n\
             pages = 9\nsource = \"https://example.test/cfp\"\nquote = \"nine pages\"",
        )
        .unwrap();
        let over = ProfileLayer::parse(
            "checked = \"2026-08-18\"\n\
             [[rules]]\nid = \"pages\"\ncheck = \"page_limit\"\nrequirement = \"ten\"\npages = 10\n\
             [[rules]]\nid = \"extra\"\ncheck = \"abstract_length\"\nrequirement = \"short\"\n\
             max_words = 200",
        )
        .unwrap();
        let merged = base.merge(over).finish(vec![]).unwrap();
        assert_eq!(merged.name, "V", "unset scalars inherit");
        assert_eq!(merged.checked.to_string(), "2026-08-18", "set scalars win");
        assert_eq!(merged.rules.len(), 2);
        let pages = merged.rules.iter().find(|r| r.id == "pages").unwrap();
        assert_eq!(pages.pages, Some(10));
        assert!(
            !pages.is_verified(),
            "an override that drops the source must become unverified, not \
             inherit the built-in's provenance"
        );
    }

    #[test]
    fn load_layers_user_then_project_over_the_builtin() {
        let (home, cwd) = dirs();
        let user = home.path().join(".config/zorp/venues");
        let project = cwd.path().join(".zorp/venues");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            user.join("arxiv.toml"),
            "[[rules]]\nid = \"abstract-length\"\ncheck = \"abstract_length\"\n\
             requirement = \"user override\"\nmax_chars = 100",
        )
        .unwrap();
        std::fs::write(
            project.join("arxiv.toml"),
            "[[rules]]\nid = \"abstract-length\"\ncheck = \"abstract_length\"\n\
             requirement = \"project override\"\nmax_chars = 50",
        )
        .unwrap();

        let profile = load(home.path(), cwd.path(), "arxiv").unwrap();
        let rule = profile
            .rules
            .iter()
            .find(|r| r.id == "abstract-length")
            .unwrap();
        assert_eq!(rule.max_chars, Some(50), "project layer should win");
        assert_eq!(profile.layers.len(), 3, "three layers should be recorded");
        assert_eq!(profile.layers[0], "built-in");
    }

    #[test]
    fn a_user_only_venue_loads_without_a_builtin() {
        let (home, cwd) = dirs();
        let project = cwd.path().join(".zorp/venues");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("my-workshop.toml"),
            "id = \"my-workshop\"\nname = \"My Workshop\"\nchecked = \"2026-08-01\"\n\
             [[rules]]\nid = \"pages\"\ncheck = \"page_limit\"\nrequirement = \"four pages\"\n\
             pages = 4\nnote = \"read off the workshop web page by hand\"",
        )
        .unwrap();
        let profile = load(home.path(), cwd.path(), "my-workshop").unwrap();
        assert_eq!(profile.name, "My Workshop");
        assert_eq!(profile.unverified_rules(), 1);
        assert!(list(home.path(), cwd.path()).contains(&"my-workshop".to_string()));
    }

    #[test]
    fn an_unknown_venue_names_the_ones_that_exist() {
        let (home, cwd) = dirs();
        let err = load(home.path(), cwd.path(), "nope").unwrap_err();
        let shown = err.to_string();
        assert!(shown.contains("arxiv"), "should list known ids: {shown}");
        assert!(
            shown.contains(".zorp/venues/nope.toml"),
            "should say how to add one: {shown}"
        );
    }

    #[test]
    fn a_venue_id_cannot_escape_its_directory() {
        let (home, cwd) = dirs();
        for id in ["../evil", "/etc/passwd", "a/b", "..", ""] {
            assert!(
                matches!(load(home.path(), cwd.path(), id), Err(ProfileError::BadId(_))),
                "{id} should be refused"
            );
        }
    }

    #[test]
    fn freshness_turns_stale_the_day_after_the_limit() {
        let profile = ProfileLayer::parse(
            "id = \"v\"\nname = \"V\"\nchecked = \"2026-01-01\"\nstale_after_days = 180\n\
             [[rules]]\nid = \"r\"\ncheck = \"anonymity\"\nrequirement = \"blind\"\n\
             note = \"unsourced on purpose\"",
        )
        .unwrap()
        .finish(vec![])
        .unwrap();
        let on_limit = Date::parse("2026-06-30").unwrap();
        assert_eq!(
            profile.freshness(on_limit),
            Freshness::Fresh { age_days: 180 }
        );
        let past = Date::parse("2026-07-01").unwrap();
        assert_eq!(
            profile.freshness(past),
            Freshness::Stale {
                age_days: 181,
                limit_days: 180
            }
        );
    }

    #[test]
    fn a_passed_deadline_reads_as_a_closed_cycle() {
        let profile = ProfileLayer::parse(
            "id = \"v\"\nname = \"V\"\nchecked = \"2026-08-18\"\n\
             [deadline]\ndate = \"2026-05-06\"\nlabel = \"full paper submission\"\n\
             [[rules]]\nid = \"r\"\ncheck = \"anonymity\"\nrequirement = \"blind\"\n\
             note = \"unsourced on purpose\"",
        )
        .unwrap()
        .finish(vec![])
        .unwrap();
        let today = Date::parse("2026-08-18").unwrap();
        match profile.cycle(today) {
            CycleState::Closed { days_ago, .. } => assert_eq!(days_ago, 104),
            other => panic!("expected a closed cycle, got {other:?}"),
        }
        let earlier = Date::parse("2026-04-06").unwrap();
        match profile.cycle(earlier) {
            CycleState::Open { days_left, .. } => assert_eq!(days_left, 30),
            other => panic!("expected an open cycle, got {other:?}"),
        }
    }
}
