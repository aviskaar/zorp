//! The review dimensions, as data.
//!
//! The orchestrator iterates this table. It branches on no dimension by
//! name, so adding a dimension is an entry here and nothing else. What a
//! dimension needs in order to run is declared here too, so a dimension
//! that cannot be answered from the inputs at hand is dropped up front
//! and named in the report rather than run against nothing.

/// What kind of question a dimension asks. The report groups by this and
/// keeps the groups apart on purpose: a paper can be highly shareable and
/// wrong, and a finding about reach must never be read as a finding about
/// correctness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    /// Is it right.
    Technical,
    /// Can it be read and understood.
    Communication,
    /// Will it be read and by whom.
    Distribution,
    /// What it means for someone deciding whether to fund or build on it.
    Executive,
}

impl Category {
    pub fn label(&self) -> &'static str {
        match self {
            Category::Technical => "Technical",
            Category::Communication => "Communication",
            Category::Distribution => "Distribution",
            Category::Executive => "Executive",
        }
    }

    /// The line the report prints under a non-technical group. Findings
    /// in these groups say nothing about whether the paper is correct.
    pub fn caveat(&self) -> Option<&'static str> {
        match self {
            Category::Technical => None,
            Category::Communication => Some(
                "These are findings about how the paper reads. None of them says anything \
                 about whether it is correct.",
            ),
            Category::Distribution => Some(
                "These are findings about whether the paper will be read and shared. \
                 Shareable is not the same as correct, and nothing here is evidence \
                 either way.",
            ),
            Category::Executive => Some(
                "These are findings about what the work means for someone deciding \
                 whether to fund or build on it. They are judgements about consequence, \
                 not about whether the paper is correct.",
            ),
        }
    }
}

/// What a dimension needs before it can be run at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Needs {
    /// The paper text, which every dimension gets.
    Nothing,
    /// The track's recorded evidence: the validation verdict and every
    /// metric investigate recorded.
    EvidenceRecord,
    /// A venue shortlist to check the paper against.
    VenueList,
}

#[derive(Debug)]
pub struct Dimension {
    pub key: &'static str,
    pub title: &'static str,
    pub category: Category,
    pub needs: Needs,
    /// Whether this dimension runs unless the caller asks otherwise.
    /// See `docs/superpowers/specs/2026-08-18-zorp-review-design.md` for
    /// why the default set is smaller than the full one.
    pub default_on: bool,
    /// What the reviewer is told to look for. Written as an instruction
    /// to go and check something specific, never as a topic: "assess the
    /// statistics" produces an essay, "find every number reported
    /// without a spread" produces findings.
    pub brief: &'static str,
}

pub const DIMENSIONS: &[Dimension] = &[
    Dimension {
        key: "citation-integrity",
        title: "Citation integrity",
        category: Category::Technical,
        needs: Needs::Nothing,
        default_on: true,
        brief: "For each cited work, check whether it says what the paper claims it says. \
                Go to the cited work where you can. Flag citations that do not exist, that \
                are attributed to the wrong authors or year, that are cited for a claim \
                they do not make, and that are cited for a stronger claim than they make.",
    },
    Dimension {
        key: "claim-evidence-traceability",
        title: "Claim to evidence traceability",
        category: Category::Technical,
        needs: Needs::EvidenceRecord,
        default_on: true,
        brief: "Take every quantitative or factual claim in the paper and trace it to the \
                recorded evidence you have been given. Flag any claim with no recorded \
                metric or citation behind it, any figure in the paper that does not match \
                the recorded value, and any recorded result the paper leaves out.",
    },
    Dimension {
        key: "statistical-validity",
        title: "Statistical validity",
        category: Category::Technical,
        needs: Needs::Nothing,
        default_on: true,
        brief: "Find every number presented as a result. Flag the ones reported without a \
                spread, sample size, or number of runs. Check whether significance is \
                claimed and on what test, whether the sample supports it, and whether \
                several comparisons are made without any correction.",
    },
    Dimension {
        key: "reproducibility",
        title: "Reproducibility",
        category: Category::Technical,
        needs: Needs::Nothing,
        default_on: true,
        brief: "Work out what a stranger would need to re-run this and list what is \
                missing: seeds, software and model versions, hyperparameters, hardware, \
                data availability, and the exact commands. Flag each missing item against \
                the specific result it would be needed to reproduce.",
    },
    Dimension {
        key: "technical-correctness",
        title: "Technical correctness",
        category: Category::Technical,
        needs: Needs::Nothing,
        default_on: true,
        brief: "Check the reasoning, definitions, algorithms, and arithmetic. Flag steps \
                that do not follow, terms used inconsistently, and results that contradict \
                each other or the method described.",
    },
    Dimension {
        key: "benchmarking-validity",
        title: "Benchmarking validity",
        category: Category::Technical,
        needs: Needs::Nothing,
        default_on: true,
        brief: "Check whether the comparison is fair. Flag baselines that are weaker than \
                the state of the art, tuning spent on one side and not the other, metrics \
                chosen after seeing results, test sets that overlap training data, and \
                benchmarks that do not measure what the claim needs.",
    },
    Dimension {
        key: "data-correctness",
        title: "Data correctness",
        category: Category::Technical,
        needs: Needs::Nothing,
        default_on: true,
        brief: "Check the data described: where it came from, how it was split and \
                filtered, how much of it there is, and whether the numbers in the text, \
                tables, and figures agree with each other. Flag totals that do not add up \
                and quantities that change between sections.",
    },
    Dimension {
        key: "threats-to-validity",
        title: "Threats to validity",
        category: Category::Technical,
        needs: Needs::Nothing,
        default_on: true,
        brief: "Read what the paper says could be wrong with it. Judge whether that list \
                is honest and complete, and name the threats it leaves out. A paper that \
                states no threats at all is itself the finding.",
    },
    Dimension {
        key: "novelty-prior-art",
        title: "Novelty and prior art",
        category: Category::Technical,
        needs: Needs::Nothing,
        default_on: true,
        brief: "Check each novelty claim against work that already exists. Flag claims to \
                be first where prior work does the same thing, and contributions that are \
                a known technique renamed.",
    },
    Dimension {
        key: "figures-tables",
        title: "Figures and tables",
        category: Category::Technical,
        needs: Needs::Nothing,
        default_on: true,
        brief: "For each figure and table, check that it supports the claim the text makes \
                about it. Flag axes that mislead, missing units or error bars, captions \
                that overstate what is plotted, and figures the text never refers to.",
    },
    Dimension {
        key: "related-work-coverage",
        title: "Related work coverage",
        category: Category::Technical,
        needs: Needs::Nothing,
        default_on: false,
        brief: "Name specific prior work the paper should have engaged with and does not. \
                Give the actual work, not an area. Flag omissions that would change how a \
                reader reads the contribution.",
    },
    Dimension {
        key: "architecture-validation",
        title: "Architecture validation",
        category: Category::Technical,
        needs: Needs::Nothing,
        default_on: false,
        brief: "Check the described design against what it claims to achieve. Flag \
                components that are not needed for any stated claim, claims the design \
                cannot support, and failure modes the design admits but the paper does not \
                discuss.",
    },
    Dimension {
        key: "completeness",
        title: "Completeness",
        category: Category::Technical,
        needs: Needs::Nothing,
        default_on: false,
        brief: "Find what a reader needs and cannot get: a described method with a missing \
                step, a promised appendix, a result mentioned and never reported, a \
                forward reference that goes nowhere.",
    },
    Dimension {
        key: "content-quality",
        title: "Content quality",
        category: Category::Communication,
        needs: Needs::Nothing,
        default_on: false,
        brief: "Flag passages where the argument is present but the writing loses it: \
                a claim buried in a subclause, a section whose purpose is never stated, a \
                paragraph that repeats an earlier one. Quote the passage. Do not give \
                general writing advice.",
    },
    Dimension {
        key: "readability",
        title: "Readability",
        category: Category::Communication,
        needs: Needs::Nothing,
        default_on: false,
        brief: "Flag specific sentences a competent reader in the field would have to read \
                twice: undefined notation, an acronym used before it is expanded, a \
                sentence with three nested clauses. Quote each one. Do not comment on \
                style in general.",
    },
    Dimension {
        key: "virality-reach",
        title: "Virality and reach",
        category: Category::Distribution,
        needs: Needs::Nothing,
        default_on: false,
        brief: "Judge whether this gets read and passed on. Is the title legible to someone \
                one field over? Is there a single quotable claim, and can you state it? Is \
                the contribution in the first paragraph or on page four? Is there a hook: a \
                surprising number, a named artifact, a result that contradicts what people \
                assume? This is a distribution judgement and says nothing about whether the \
                paper is correct.",
    },
    Dimension {
        key: "venue-fit",
        title: "Venue fit",
        category: Category::Distribution,
        needs: Needs::VenueList,
        default_on: false,
        brief: "Check the paper against the venue shortlist you have been given. For each \
                venue, flag what this paper would be desk-rejected or criticised for there: \
                scope, expected evaluation, length, and the kind of contribution that venue \
                publishes.",
    },
    Dimension {
        key: "problem-validation",
        title: "Problem validation",
        category: Category::Executive,
        needs: Needs::Nothing,
        default_on: false,
        brief: "Check that the problem is real and stated as more than an assumption. Flag \
                motivation asserted with no evidence, a problem already solved well enough \
                in practice, and a gap between the problem described and the one the work \
                actually addresses.",
    },
    Dimension {
        key: "business-roi",
        title: "Business case and return",
        category: Category::Executive,
        needs: Needs::Nothing,
        default_on: false,
        brief: "Work out what this is worth to someone paying for it. Flag benefits stated \
                without a magnitude, costs left out, and comparisons against doing nothing \
                that the paper never makes.",
    },
    Dimension {
        key: "exec-ceo",
        title: "Executive review: CEO",
        category: Category::Executive,
        needs: Needs::Nothing,
        default_on: false,
        brief: "Read this as a chief executive. Does the work fit where the organisation is \
                going, and is the timing right: too early to matter, or late to something \
                already settled? Flag strategic claims the paper makes and does not \
                support, and the question a board would ask that this does not answer.",
    },
    Dimension {
        key: "exec-cto",
        title: "Executive review: CTO",
        category: Category::Executive,
        needs: Needs::Nothing,
        default_on: false,
        brief: "Read this as a chief technology officer. What does adopting this commit the \
                organisation to building and maintaining? Flag technical risk the paper \
                understates, dependencies it takes on, the migration it implies and does \
                not describe, and what happens when the approach fails in production.",
    },
    Dimension {
        key: "exec-cfo",
        title: "Executive review: CFO",
        category: Category::Executive,
        needs: Needs::Nothing,
        default_on: false,
        brief: "Read this as a chief financial officer. What does the return rest on? Take \
                each number offered as a benefit and trace what it is computed from. Flag \
                costs the paper does not count, savings claimed from a single measurement, \
                and payback periods asserted rather than derived.",
    },
];

pub fn by_key(key: &str) -> Option<&'static Dimension> {
    DIMENSIONS.iter().find(|d| d.key == key)
}

/// What the caller asked to review on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    /// The dimensions that are on by default.
    Core,
    /// Every dimension in the table.
    All,
    /// An explicit list of keys.
    Explicit(Vec<String>),
}

#[derive(Debug, PartialEq, Eq)]
pub struct UnknownDimension(pub String);

impl std::fmt::Display for UnknownDimension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unknown review dimension '{}'; known dimensions are: {}",
            self.0,
            DIMENSIONS
                .iter()
                .map(|d| d.key)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl Selection {
    pub fn parse(spec: &str) -> Result<Selection, UnknownDimension> {
        match spec.trim() {
            "core" => Ok(Selection::Core),
            "all" => Ok(Selection::All),
            other => {
                let keys: Vec<String> = other
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                for key in &keys {
                    if by_key(key).is_none() {
                        return Err(UnknownDimension(key.clone()));
                    }
                }
                Ok(Selection::Explicit(keys))
            }
        }
    }
}

/// What the run has available to review against.
#[derive(Clone, Copy, Debug, Default)]
pub struct Inputs {
    pub has_evidence_record: bool,
    pub has_venue_list: bool,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub selected: Vec<&'static Dimension>,
    /// Dimensions asked for and not run, each with why. Named in the
    /// report: a dimension silently skipped reads as a dimension that
    /// found nothing.
    pub skipped: Vec<(&'static str, String)>,
}

/// Choose which dimensions run, dropping any whose inputs are absent.
pub fn plan(selection: &Selection, inputs: &Inputs) -> Plan {
    let asked: Vec<&'static Dimension> = match selection {
        Selection::Core => DIMENSIONS.iter().filter(|d| d.default_on).collect(),
        Selection::All => DIMENSIONS.iter().collect(),
        Selection::Explicit(keys) => keys.iter().filter_map(|k| by_key(k)).collect(),
    };

    let mut out = Plan::default();
    for dimension in asked {
        let missing = match dimension.needs {
            Needs::Nothing => None,
            Needs::EvidenceRecord if !inputs.has_evidence_record => Some(
                "the track has no recorded evidence yet, so there is nothing to trace \
                 claims to; run investigate first"
                    .to_string(),
            ),
            Needs::VenueList if !inputs.has_venue_list => Some(
                "no venue shortlist was available, so there is nothing to check fit \
                 against; run deliver first or pass --venues"
                    .to_string(),
            ),
            _ => None,
        };
        match missing {
            Some(reason) => out.skipped.push((dimension.key, reason)),
            None => out.selected.push(dimension),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_dimension_key_is_unique() {
        let mut keys: Vec<&str> = DIMENSIONS.iter().map(|d| d.key).collect();
        let total = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), total, "duplicate dimension key");
    }

    /// The whole point of the table: control flow reads it, so a new
    /// dimension is a row and nothing else. If this ever needs updating
    /// alongside a match arm somewhere, the table has stopped being data.
    #[test]
    fn the_core_selection_is_read_from_the_table_not_hardcoded() {
        let core = plan(&Selection::Core, &Inputs::default());
        let expected = DIMENSIONS
            .iter()
            .filter(|d| d.default_on && d.needs == Needs::Nothing)
            .count();
        assert_eq!(core.selected.len(), expected);
        assert!(core.selected.iter().all(|d| d.default_on));
    }

    #[test]
    fn all_selects_every_dimension_whose_inputs_are_present() {
        let inputs = Inputs {
            has_evidence_record: true,
            has_venue_list: true,
        };
        let all = plan(&Selection::All, &inputs);
        assert_eq!(all.selected.len(), DIMENSIONS.len());
        assert!(all.skipped.is_empty());
    }

    /// A dimension that cannot run must be named. Dropping it quietly
    /// would let the report imply it was checked and came back clean.
    #[test]
    fn a_dimension_with_no_evidence_record_is_skipped_and_named() {
        let inputs = Inputs {
            has_evidence_record: false,
            has_venue_list: true,
        };
        let p = plan(&Selection::All, &inputs);
        let skipped: Vec<&str> = p.skipped.iter().map(|(k, _)| *k).collect();
        assert!(skipped.contains(&"claim-evidence-traceability"));
        assert!(p
            .skipped
            .iter()
            .any(|(_, reason)| reason.contains("investigate")));
    }

    #[test]
    fn venue_fit_is_skipped_without_a_venue_list() {
        let inputs = Inputs {
            has_evidence_record: true,
            has_venue_list: false,
        };
        let p = plan(&Selection::All, &inputs);
        assert!(p.skipped.iter().any(|(k, _)| *k == "venue-fit"));
        assert!(!p.selected.iter().any(|d| d.key == "venue-fit"));
    }

    #[test]
    fn an_explicit_selection_runs_exactly_what_was_asked_for() {
        let sel = Selection::parse("readability,exec-cfo").unwrap();
        let p = plan(&sel, &Inputs::default());
        let keys: Vec<&str> = p.selected.iter().map(|d| d.key).collect();
        assert_eq!(keys, vec!["readability", "exec-cfo"]);
    }

    #[test]
    fn an_unknown_dimension_is_rejected_rather_than_ignored() {
        let err = Selection::parse("readability,made-up").unwrap_err();
        assert_eq!(err, UnknownDimension("made-up".to_string()));
        assert!(err.to_string().contains("readability"));
    }

    #[test]
    fn every_required_dimension_exists() {
        for key in [
            "technical-correctness",
            "content-quality",
            "readability",
            "benchmarking-validity",
            "data-correctness",
            "completeness",
            "business-roi",
            "problem-validation",
            "novelty-prior-art",
            "architecture-validation",
            "statistical-validity",
            "reproducibility",
            "citation-integrity",
            "claim-evidence-traceability",
            "threats-to-validity",
            "related-work-coverage",
            "venue-fit",
            "figures-tables",
            "virality-reach",
            "exec-ceo",
            "exec-cto",
            "exec-cfo",
        ] {
            assert!(by_key(key).is_some(), "{key} is missing from the table");
        }
    }

    /// Reach and executive judgement are not correctness. The report
    /// groups by category and only the technical group has no caveat.
    #[test]
    fn only_the_technical_category_carries_no_caveat() {
        assert!(Category::Technical.caveat().is_none());
        assert!(Category::Distribution.caveat().is_some());
        assert!(Category::Executive.caveat().is_some());
        assert!(Category::Communication.caveat().is_some());
    }

    #[test]
    fn virality_is_a_distribution_dimension_not_a_technical_one() {
        assert_eq!(
            by_key("virality-reach").unwrap().category,
            Category::Distribution
        );
    }

    #[test]
    fn the_three_executive_readers_are_separate_dimensions() {
        for key in ["exec-ceo", "exec-cto", "exec-cfo"] {
            let d = by_key(key).unwrap();
            assert_eq!(d.category, Category::Executive);
        }
    }
}
