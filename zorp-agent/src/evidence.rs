//! The evidence record, flattened into citable items.
//!
//! One list, one key per item, derived from what a track actually
//! recorded. co-write hands this list to the model so a draft can cite
//! by key, and deliver builds a paper's reference list from the same
//! call. Both ends reading the same function is the point: a reference
//! in a delivered paper is a row in the record, not a string a model
//! produced.

use zorp_track::experiment::MetricValue;
use zorp_track::{Project, TrackError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceItem {
    /// The handle prose cites, `E1` upward, assigned by position.
    pub key: String,
    /// What the record says.
    pub claim: String,
    /// Where in the record it says it.
    pub source: String,
}

pub fn format_metric_value(value: &MetricValue) -> String {
    match value {
        MetricValue::Number(n) => n.to_string(),
        MetricValue::Text(s) => s.clone(),
        MetricValue::Bool(b) => b.to_string(),
    }
}

/// Every citable item in `track_id`'s record, in a fixed order:
/// validate's cited sources first, then every metric investigate
/// recorded. Both underlying queries are ordered, so the same record
/// produces the same keys on every run.
pub fn for_track(project: &Project, track_id: &str) -> Result<Vec<EvidenceItem>, TrackError> {
    let mut items: Vec<EvidenceItem> = Vec::new();

    match project.store.get_validation(track_id) {
        Ok(validation) => {
            for citation in &validation.redundancy_citations {
                items.push(EvidenceItem {
                    key: String::new(),
                    claim: citation.text.clone(),
                    source: format!("validate, redundancy: {}", citation.source),
                });
            }
            for citation in &validation.feasibility_citations {
                items.push(EvidenceItem {
                    key: String::new(),
                    claim: citation.text.clone(),
                    source: format!("validate, feasibility: {}", citation.source),
                });
            }
        }
        // A track that was never validated is normal, not broken.
        Err(TrackError::NotFound {
            kind: "validation", ..
        }) => {}
        Err(e) => return Err(e),
    }

    for (experiment_id, key, value) in project.store.metrics_for_track(track_id)? {
        items.push(EvidenceItem {
            key: String::new(),
            claim: format!("{key} = {}", format_metric_value(&value)),
            source: format!("investigate, experiment {experiment_id}"),
        });
    }

    for (i, item) in items.iter_mut().enumerate() {
        item.key = format!("E{}", i + 1);
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use zorp_track::experiment::ExperimentStatus;
    use zorp_track::validation::Citation;

    fn project_with_metrics() -> (tempfile::TempDir, Project) {
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project
            .store
            .create_track("t1", "does caching help")
            .unwrap();
        let exp = project.store.create_experiment("t1", "no-prereg").unwrap();
        project
            .store
            .set_experiment_status(&exp.id, ExperimentStatus::Completed)
            .unwrap();
        project
            .store
            .record_metric(&exp.id, "latency_ms", MetricValue::Number(42.0))
            .unwrap();
        project
            .store
            .record_metric(&exp.id, "cache_hit", MetricValue::Bool(true))
            .unwrap();
        (dir, project)
    }

    #[test]
    fn an_empty_track_has_no_evidence() {
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track("t1", "hyp").unwrap();
        assert!(for_track(&project, "t1").unwrap().is_empty());
    }

    #[test]
    fn every_metric_becomes_one_item_with_its_experiment_as_the_source() {
        let (_dir, project) = project_with_metrics();
        let items = for_track(&project, "t1").unwrap();

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].key, "E1");
        assert_eq!(items[0].claim, "latency_ms = 42");
        assert!(items[0].source.contains("investigate"), "{:?}", items[0]);
        assert_eq!(items[1].key, "E2");
        assert_eq!(items[1].claim, "cache_hit = true");
    }

    #[test]
    fn validation_citations_come_first_and_keep_their_source() {
        let (_dir, project) = project_with_metrics();
        project
            .store
            .record_validation(
                "t1",
                20.0,
                &[Citation {
                    text: "no prior benchmark found".into(),
                    source: "search result 1".into(),
                }],
                85.0,
                &[Citation {
                    text: "a harness already exists".into(),
                    source: "repo README".into(),
                }],
                "worth investigating",
            )
            .unwrap();

        let items = for_track(&project, "t1").unwrap();

        assert_eq!(items.len(), 4);
        assert_eq!(items[0].key, "E1");
        assert_eq!(items[0].claim, "no prior benchmark found");
        assert!(
            items[0].source.contains("search result 1"),
            "{:?}",
            items[0]
        );
        assert!(items[0].source.contains("redundancy"), "{:?}", items[0]);
        assert_eq!(items[1].claim, "a harness already exists");
        assert!(items[1].source.contains("feasibility"), "{:?}", items[1]);
        assert_eq!(items[2].claim, "latency_ms = 42");
        assert_eq!(items[3].claim, "cache_hit = true");
    }

    #[test]
    fn a_missing_validation_is_not_an_error() {
        let (_dir, project) = project_with_metrics();
        assert_eq!(for_track(&project, "t1").unwrap().len(), 2);
    }

    #[test]
    fn the_keys_are_the_same_on_every_call() {
        let (_dir, project) = project_with_metrics();
        assert_eq!(
            for_track(&project, "t1").unwrap(),
            for_track(&project, "t1").unwrap()
        );
    }
}
