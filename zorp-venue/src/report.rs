//! The conformance report: one line per rule, and for a failure exactly
//! what to change and where.
//!
//! The report has a second job besides listing failures. It has to be
//! honest about what it does not know: which rules carry no source, how
//! old the profile is, whether the cycle it describes has already closed,
//! and which checks it could not run at all. A report that quietly omits
//! those reads as a clean bill of health, which is the failure this whole
//! capability exists to prevent.

use crate::date::Date;
use crate::manuscript::Manuscript;
use crate::profile::{CycleState, Freshness, Rule, VenueProfile};

/// What a single check concluded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    /// Needs a human. Either the rule is conditional, or the evidence in
    /// the draft is not enough to settle it.
    Warn,
    /// A violation. At most venues this is what a desk rejection is made
    /// of.
    Fail,
    /// The check needs an input that was not supplied, so it did not run.
    /// Deliberately not a pass.
    NotChecked,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Pass => "PASS",
            Verdict::Warn => "WARN",
            Verdict::Fail => "FAIL",
            Verdict::NotChecked => "NOT CHECKED",
        }
    }

    fn rank(self) -> u8 {
        match self {
            Verdict::Pass => 0,
            Verdict::NotChecked => 1,
            Verdict::Warn => 2,
            Verdict::Fail => 3,
        }
    }

    /// The more serious of the two.
    pub fn worse_of(self, other: Verdict) -> Verdict {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

/// One rule's outcome.
#[derive(Clone, Debug)]
pub struct Finding {
    pub rule_id: String,
    pub requirement: String,
    pub verdict: Verdict,
    /// What was found, including the arithmetic behind it.
    pub detail: String,
    /// What to change, and where. One entry per thing to fix.
    pub remedy: Vec<String>,
    /// False when the rule cites no source. Carried onto the finding so a
    /// pass on a guess never reads as compliance.
    pub verified: bool,
    pub source: Option<String>,
    pub checked: Option<String>,
    pub quote: Option<String>,
    pub note: Option<String>,
}

impl Finding {
    pub fn new(rule: &Rule, verdict: Verdict, detail: String, remedy: Vec<String>) -> Finding {
        Finding {
            rule_id: rule.id.clone(),
            requirement: rule.requirement.clone(),
            verdict,
            detail,
            remedy,
            verified: rule.is_verified(),
            source: rule.source.clone(),
            checked: rule.checked.clone(),
            quote: rule.quote.clone(),
            note: rule.note.clone(),
        }
    }
}

/// How many findings landed in each verdict.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub pass: usize,
    pub warn: usize,
    pub fail: usize,
    pub not_checked: usize,
    /// Findings whose rule cites no source, whatever the verdict.
    pub unverified: usize,
}

/// The whole verdict on one draft against one venue.
#[derive(Clone, Debug)]
pub struct Report {
    pub venue_id: String,
    pub venue_name: String,
    pub venue_kind: String,
    pub draft: String,
    pub profile_layers: Vec<String>,
    pub profile_checked: Date,
    pub freshness: Freshness,
    pub cycle: CycleState,
    pub today: Date,
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn new(
        profile: &VenueProfile,
        manuscript: &Manuscript,
        findings: Vec<Finding>,
        today: Date,
    ) -> Report {
        Report {
            venue_id: profile.id.clone(),
            venue_name: profile.name.clone(),
            venue_kind: profile.kind.clone(),
            draft: manuscript
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(draft supplied as text)".to_string()),
            profile_layers: profile.layers.clone(),
            profile_checked: profile.checked,
            freshness: profile.freshness(today),
            cycle: profile.cycle(today),
            today,
            findings,
        }
    }

    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        for finding in &self.findings {
            match finding.verdict {
                Verdict::Pass => counts.pass += 1,
                Verdict::Warn => counts.warn += 1,
                Verdict::Fail => counts.fail += 1,
                Verdict::NotChecked => counts.not_checked += 1,
            }
            if !finding.verified {
                counts.unverified += 1;
            }
        }
        counts
    }

    /// True when something in here would plausibly get the paper
    /// desk-rejected. The CLI exits non-zero on this.
    pub fn has_failures(&self) -> bool {
        self.findings.iter().any(|f| f.verdict == Verdict::Fail)
    }

    /// Whether this report can be trusted as a clean bill of health. It
    /// cannot if any rule is unsourced, the profile is stale, or a check
    /// did not run.
    pub fn caveats(&self) -> Vec<String> {
        let mut out = Vec::new();
        let counts = self.counts();
        if counts.unverified > 0 {
            out.push(format!(
                "{} rule(s) in this profile cite no source. Their verdicts are \
                 marked UNVERIFIED below and this report cannot tell you that \
                 you comply with them. Check them against the venue's own call \
                 for papers before submitting.",
                counts.unverified
            ));
        }
        if let Freshness::Stale {
            age_days,
            limit_days,
        } = self.freshness
        {
            out.push(format!(
                "This profile was last checked {age_days} days ago, past its \
                 {limit_days} day staleness limit. Venue requirements change \
                 between cycles. Re-read the call for papers and update \
                 {}.toml before trusting any of this.",
                self.venue_id
            ));
        }
        if let CycleState::Closed {
            days_ago,
            date,
            label,
        } = &self.cycle
        {
            out.push(format!(
                "The {label} deadline in this profile was {date}, {days_ago} days \
                 ago. This profile describes a cycle that has closed, and the next \
                 cycle's rules may differ."
            ));
        }
        if counts.not_checked > 0 {
            out.push(format!(
                "{} check(s) did not run because an input was missing. They are \
                 not passes.",
                counts.not_checked
            ));
        }
        out
    }

    /// The report as markdown, ready to write next to the draft.
    pub fn to_markdown(&self) -> String {
        let counts = self.counts();
        let mut out = String::new();
        out.push_str(&format!(
            "# Conformance report: {} ({})\n\n",
            self.venue_name, self.venue_kind
        ));
        out.push_str(&format!("- Draft: `{}`\n", self.draft));
        out.push_str(&format!(
            "- Venue profile: `{}`, from {}\n",
            self.venue_id,
            self.profile_layers.join(" then ")
        ));
        let age = match self.freshness {
            Freshness::Fresh { age_days } => format!("{age_days} days old"),
            Freshness::Stale { age_days, .. } => format!("{age_days} days old, STALE"),
        };
        out.push_str(&format!(
            "- Requirements last checked against their sources: {} ({age})\n",
            self.profile_checked
        ));
        match &self.cycle {
            CycleState::NoDeadline => {
                out.push_str("- Deadline: none in this profile\n");
            }
            CycleState::Open {
                days_left,
                date,
                label,
            } => {
                out.push_str(&format!(
                    "- Deadline: {date} ({label}), {days_left} days away\n"
                ));
            }
            CycleState::Closed {
                days_ago,
                date,
                label,
            } => {
                out.push_str(&format!(
                    "- Deadline: {date} ({label}), PASSED {days_ago} days ago\n"
                ));
            }
        }
        out.push_str(&format!("- Checked on: {}\n\n", self.today));

        out.push_str(&format!(
            "**{} fail, {} warn, {} pass, {} not checked.**",
            counts.fail, counts.warn, counts.pass, counts.not_checked
        ));
        if counts.unverified > 0 {
            out.push_str(&format!(
                " {} of these rest on rules with no source.",
                counts.unverified
            ));
        }
        out.push_str("\n\n");

        let caveats = self.caveats();
        if !caveats.is_empty() {
            out.push_str("## What this report cannot tell you\n\n");
            for caveat in caveats {
                out.push_str(&format!("- {caveat}\n"));
            }
            out.push('\n');
        }

        out.push_str("## Summary\n\n");
        out.push_str("| Verdict | Rule | Requirement |\n|---|---|---|\n");
        for finding in self.ordered() {
            let mark = if finding.verified {
                finding.verdict.label().to_string()
            } else {
                format!("{} (UNVERIFIED)", finding.verdict.label())
            };
            out.push_str(&format!(
                "| {mark} | `{}` | {} |\n",
                finding.rule_id,
                escape_pipes(&finding.requirement)
            ));
        }
        out.push('\n');

        out.push_str("## Detail\n\n");
        for finding in self.ordered() {
            let mark = if finding.verified {
                finding.verdict.label().to_string()
            } else {
                format!("{} (UNVERIFIED)", finding.verdict.label())
            };
            out.push_str(&format!("### {mark}: `{}`\n\n", finding.rule_id));
            out.push_str(&format!("**Requirement.** {}\n\n", finding.requirement));
            out.push_str(&format!("**Found.** {}\n\n", finding.detail));
            if !finding.remedy.is_empty() {
                out.push_str("**What to change.**\n\n");
                for item in &finding.remedy {
                    out.push_str(&format!("- {item}\n"));
                }
                out.push('\n');
            }
            match &finding.source {
                Some(source) => {
                    let checked = finding
                        .checked
                        .clone()
                        .unwrap_or_else(|| self.profile_checked.to_string());
                    out.push_str(&format!("**Source.** {source} (checked {checked})\n\n"));
                    if let Some(quote) = &finding.quote {
                        out.push_str(&format!("> {quote}\n\n"));
                    }
                }
                None => {
                    out.push_str(
                        "**Source.** None. This rule is unverified: it was not read \
                         off the venue's own call for papers, so treat its verdict \
                         as a prompt to go and check, not as an answer.\n\n",
                    );
                }
            }
            if let Some(note) = &finding.note {
                out.push_str(&format!("**Note.** {note}\n\n"));
            }
        }
        out
    }

    /// Findings worst first, stable within a verdict.
    fn ordered(&self) -> Vec<&Finding> {
        let mut findings: Vec<&Finding> = self.findings.iter().collect();
        findings.sort_by_key(|f| std::cmp::Reverse(f.verdict.rank()));
        findings
    }
}

fn escape_pipes(text: &str) -> String {
    text.replace('|', "\\|")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worse_of_orders_fail_above_warn_above_not_checked_above_pass() {
        assert_eq!(Verdict::Pass.worse_of(Verdict::NotChecked), Verdict::NotChecked);
        assert_eq!(Verdict::NotChecked.worse_of(Verdict::Warn), Verdict::Warn);
        assert_eq!(Verdict::Warn.worse_of(Verdict::Fail), Verdict::Fail);
        assert_eq!(Verdict::Fail.worse_of(Verdict::Pass), Verdict::Fail);
    }
}
