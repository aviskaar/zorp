use std::collections::BTreeSet;
use std::fmt::Write as _;
use zorp_track::experiment::MetricValue;
use zorp_track::prereg::{get_preregistration, Preregistration};
use zorp_track::validation::{Citation, Validation};
use zorp_track::{Project, TrackError};

/// Everything a track actually recorded, and nothing else.
///
/// This is the whole point of the critique pass. A draft is checked
/// against this, not against the model's opinion of the draft, so the
/// ledger is built from the run record by code and is never handed to
/// the model to extend.
#[derive(Debug, Clone, Default)]
pub struct EvidenceLedger {
    /// `(experiment_id, metric_key, value)`, in experiment then metric
    /// order, exactly as co-write receives them.
    pub metrics: Vec<(String, String, MetricValue)>,
    pub validation: Option<Validation>,
    pub prereg: Option<Preregistration>,
    /// How many experiments the track ran. A draft that counts attempts
    /// is making a checkable claim about the record, so the count is
    /// part of the record for auditing purposes.
    pub experiment_count: usize,
}

impl EvidenceLedger {
    pub fn from_track(project: &Project, track_id: &str) -> Result<Self, TrackError> {
        let metrics = project.store.metrics_for_track(track_id)?;
        let validation = match project.store.get_validation(track_id) {
            Ok(v) => Some(v),
            Err(TrackError::NotFound {
                kind: "validation", ..
            }) => None,
            Err(e) => return Err(e),
        };
        let prereg = get_preregistration(&project.store, track_id)?;
        let experiment_count = project.store.experiments_for(track_id)?.len();
        Ok(EvidenceLedger {
            metrics,
            validation,
            prereg,
            experiment_count,
        })
    }

    /// Nothing was gathered, so there is nothing to check a draft
    /// against. Auditing against an empty ledger would flag every
    /// sentence, which is noise, not criticism.
    pub fn is_empty(&self) -> bool {
        self.metrics.is_empty() && self.validation.is_none()
    }

    fn citations(&self) -> Vec<&Citation> {
        match &self.validation {
            Some(v) => v
                .redundancy_citations
                .iter()
                .chain(v.feasibility_citations.iter())
                .collect(),
            None => Vec::new(),
        }
    }

    /// The exact set of keys a claim is allowed to rest on. The model is
    /// given this list and may only choose from it, so "cites something
    /// the record does not contain" is a set-membership test rather
    /// than a judgement call.
    pub fn evidence_keys(&self) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        for (_, key, _) in &self.metrics {
            keys.insert(format!("metric:{key}"));
        }
        for citation in self.citations() {
            keys.insert(format!("source:{}", citation.source));
        }
        if self.validation.is_some() {
            keys.insert("verdict".to_string());
        }
        if self.prereg.is_some() {
            keys.insert("prereg:kill-threshold".to_string());
        }
        keys
    }

    /// Every figure the record supports, before any rounding. Includes
    /// numbers written inside citation text and the validation verdict:
    /// those were gathered too, and a draft quoting one of them is
    /// quoting the record.
    pub fn recorded_numbers(&self) -> Vec<f64> {
        let mut out = Vec::new();
        for (_, _, value) in &self.metrics {
            match value {
                MetricValue::Number(n) => out.push(*n),
                MetricValue::Text(s) => out.extend(super::audit::all_numbers(s)),
                MetricValue::Bool(_) => {}
            }
        }
        if let Some(v) = &self.validation {
            out.push(v.redundancy_score);
            out.push(v.feasibility_score);
            out.extend(super::audit::all_numbers(&v.verdict));
            for citation in self.citations() {
                out.extend(super::audit::all_numbers(&citation.text));
                out.extend(super::audit::all_numbers(&citation.source));
            }
        }
        if let Some(p) = &self.prereg {
            out.push(p.kill_threshold);
        }
        out
    }

    /// Facts about the shape of the record: how many attempts it holds,
    /// how many metrics, how many sources. A draft that counts attempts
    /// is asserting one of these.
    ///
    /// Kept apart from `recorded_numbers` because these must match
    /// exactly. A track with one experiment would otherwise license any
    /// drafted 100 through the proportion-to-percentage scaling.
    pub fn recorded_counts(&self) -> Vec<f64> {
        vec![
            self.experiment_count as f64,
            self.metrics.len() as f64,
            self.citations().len() as f64,
        ]
    }

    /// The ledger as the model sees it. Keys are shown verbatim so an
    /// extracted claim can name one without paraphrasing.
    pub fn render(&self) -> String {
        let mut out = String::new();
        if let Some(p) = &self.prereg {
            let _ = writeln!(
                out,
                "- prereg:kill-threshold = {} on metric '{}' ({}). Pre-registered by a human; it is fixed.",
                p.kill_threshold,
                p.metric_name,
                p.threshold_direction
                    .map(|d| d.as_str())
                    .unwrap_or("no direction recorded")
            );
        }
        for (experiment_id, key, value) in &self.metrics {
            let _ = writeln!(
                out,
                "- metric:{key} = {} (recorded by experiment {experiment_id})",
                format_metric_value(value)
            );
        }
        if let Some(v) = &self.validation {
            let _ = writeln!(
                out,
                "- verdict = {} (redundancy {:.0}/100, feasibility {:.0}/100)",
                v.verdict, v.redundancy_score, v.feasibility_score
            );
            for citation in self.citations() {
                let _ = writeln!(out, "- source:{} = \"{}\"", citation.source, citation.text);
            }
        }
        if out.is_empty() {
            out.push_str("- (nothing recorded)\n");
        }
        out
    }
}

pub fn format_metric_value(value: &MetricValue) -> String {
    match value {
        MetricValue::Number(n) => n.to_string(),
        MetricValue::Text(s) => s.clone(),
        MetricValue::Bool(b) => b.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger_with_metric(key: &str, value: MetricValue) -> EvidenceLedger {
        EvidenceLedger {
            metrics: vec![("exp-1".to_string(), key.to_string(), value)],
            experiment_count: 1,
            ..Default::default()
        }
    }

    fn validation(verdict: &str, citations: Vec<Citation>) -> Validation {
        Validation {
            id: "v1".to_string(),
            track_id: "t1".to_string(),
            redundancy_score: 20.0,
            redundancy_citations: citations,
            feasibility_score: 85.0,
            feasibility_citations: vec![],
            verdict: verdict.to_string(),
            created_at: 0,
        }
    }

    #[test]
    fn evidence_keys_name_every_recorded_metric_and_source() {
        let mut ledger = ledger_with_metric("latency_ms", MetricValue::Number(42.0));
        ledger.validation = Some(validation(
            "worth investigating",
            vec![Citation {
                text: "no prior benchmark".to_string(),
                source: "search result 1".to_string(),
            }],
        ));

        let keys = ledger.evidence_keys();
        assert!(keys.contains("metric:latency_ms"), "{keys:?}");
        assert!(keys.contains("source:search result 1"), "{keys:?}");
        assert!(keys.contains("verdict"), "{keys:?}");
        // Nothing else. A key the record does not have must not appear
        // just because it is plausible.
        assert!(!keys.contains("metric:throughput"), "{keys:?}");
    }

    #[test]
    fn recorded_numbers_include_metric_values_scores_and_the_threshold() {
        let mut ledger = ledger_with_metric("latency_ms", MetricValue::Number(42.0));
        ledger.validation = Some(validation("worth investigating", vec![]));

        let numbers = ledger.recorded_numbers();
        assert!(numbers.contains(&42.0), "{numbers:?}");
        assert!(numbers.contains(&20.0), "{numbers:?}");
        assert!(numbers.contains(&85.0), "{numbers:?}");
    }

    #[test]
    fn a_number_written_inside_a_citation_counts_as_recorded() {
        let mut ledger = ledger_with_metric("latency_ms", MetricValue::Number(42.0));
        ledger.validation = Some(validation(
            "worth investigating",
            vec![Citation {
                text: "the prior benchmark reported 913 requests per second".to_string(),
                source: "search result 1".to_string(),
            }],
        ));

        // It was gathered, cited, and stored. A draft quoting it is
        // quoting the record, not inventing a figure.
        assert!(ledger.recorded_numbers().contains(&913.0));
    }

    #[test]
    fn an_empty_ledger_is_reported_as_empty() {
        assert!(EvidenceLedger::default().is_empty());
        assert!(!ledger_with_metric("latency_ms", MetricValue::Number(1.0)).is_empty());
    }

    #[test]
    fn render_names_keys_verbatim_so_a_claim_can_cite_one() {
        let ledger = ledger_with_metric("latency_ms", MetricValue::Number(42.0));
        let rendered = ledger.render();
        assert!(rendered.contains("metric:latency_ms = 42"), "{rendered}");
    }
}
