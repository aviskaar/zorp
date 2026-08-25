//! deliver's paper mode: a co-written draft plus the track's evidence
//! record, turned into a paper-shaped markdown file and a PDF of it.
//!
//! No model is called here. Everything this produces is a function of
//! `draft.md` and the record, so the same investigation produces the
//! same document, and the command needs neither a network nor an API
//! key. That is also why the reference list cannot be invented: it is
//! built from `crate::evidence::for_track` and the draft's own prose is
//! only allowed to cite into it.

use super::DeliverError;
use crate::evidence;
use std::path::PathBuf;
use zorp_paper::pdf::PdfOptions;
use zorp_paper::{date, markdown, pdf, Block, Paper, PaperParts, Reference, Section};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::track::TrackStatus;
use zorp_track::Project;

#[derive(Debug, Clone)]
pub struct PaperOutcome {
    pub markdown_path: PathBuf,
    /// `None` when the PDF could not be written. The markdown is still
    /// there, and `pdf_error` says why the PDF is not.
    pub pdf_path: Option<PathBuf>,
    pub pdf_error: Option<String>,
    pub reference_count: usize,
    pub approved: bool,
}

/// Headings whose content is replaced wholesale by the record-derived
/// reference list. A model asked for a paper will write a plausible
/// bibliography, and a plausible bibliography is the exact artifact this
/// feature exists to keep out of the output.
const REFERENCE_HEADINGS: [&str; 4] = ["references", "bibliography", "works cited", "citations"];

fn is_reference_heading(title: &str) -> bool {
    let t = title.trim().to_ascii_lowercase();
    REFERENCE_HEADINGS.contains(&t.as_str())
}

fn strip_front_matter(draft: &str) -> &str {
    let rest = match draft.strip_prefix("---\n") {
        Some(rest) => rest,
        None => return draft,
    };
    match rest.find("\n---\n") {
        Some(at) => &rest[at + 5..],
        None => draft,
    }
}

fn heading(line: &str) -> Option<(u8, String)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    let text = rest.strip_prefix(' ')?;
    Some((hashes as u8, text.trim().to_string()))
}

fn bullet(line: &str) -> Option<String> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some(rest.trim().to_string());
        }
    }
    None
}

fn is_rule(line: &str) -> bool {
    line.len() >= 3
        && (line.chars().all(|c| c == '-')
            || line.chars().all(|c| c == '*')
            || line.chars().all(|c| c == '_'))
}

#[derive(Default)]
struct Builder {
    sections: Vec<Section>,
    lead: Vec<Block>,
    current: Option<Section>,
    paragraph: Vec<String>,
    table: Vec<String>,
}

impl Builder {
    fn untouched(&self) -> bool {
        self.sections.is_empty()
            && self.current.is_none()
            && self.lead.is_empty()
            && self.paragraph.is_empty()
            && self.table.is_empty()
    }

    fn push(&mut self, block: Block) {
        match &mut self.current {
            Some(section) => section.blocks.push(block),
            None => self.lead.push(block),
        }
    }

    fn flush_paragraph(&mut self) {
        if !self.paragraph.is_empty() {
            let text = std::mem::take(&mut self.paragraph).join(" ");
            self.push(Block::Paragraph(text));
        }
    }

    /// A markdown table has no equivalent in this document model, and
    /// folding one into a paragraph turns it into noise. Verbatim in a
    /// fixed-width font at least keeps the columns readable.
    fn flush_table(&mut self) {
        if !self.table.is_empty() {
            let text = std::mem::take(&mut self.table).join("\n");
            self.push(Block::Code(text));
        }
    }

    fn flush(&mut self) {
        self.flush_paragraph();
        self.flush_table();
    }

    /// Fold an indented continuation line back into the bullet it
    /// belongs to. Without this a wrapped list item becomes a paragraph
    /// of its own and loses the indent, which is what a hand-wrapped
    /// draft looks like on the page.
    fn continue_bullet(&mut self, text: &str) -> bool {
        if !self.paragraph.is_empty() || !self.table.is_empty() {
            return false;
        }
        let blocks = match &mut self.current {
            Some(section) => &mut section.blocks,
            None => &mut self.lead,
        };
        match blocks.last_mut() {
            Some(Block::Bullet(existing)) => {
                existing.push(' ');
                existing.push_str(text);
                true
            }
            _ => false,
        }
    }

    fn open(&mut self, title: String, level: u8) {
        self.flush();
        if let Some(section) = self.current.take() {
            self.sections.push(section);
        }
        self.current = Some(Section {
            title,
            level,
            blocks: Vec::new(),
        });
    }

    fn finish(mut self) -> (Vec<Block>, Vec<Section>) {
        self.flush();
        if let Some(section) = self.current.take() {
            self.sections.push(section);
        }
        (self.lead, self.sections)
    }
}

struct ParsedDraft {
    title: String,
    abstract_text: String,
    sections: Vec<Section>,
}

/// Read `draft.md` as a paper. Anything the document model has no place
/// for is degraded rather than rejected: this runs after a human has
/// already reviewed the draft, so refusing to typeset it over a stray
/// construct would be the wrong answer.
fn parse_draft(draft: &str, fallback_title: &str) -> ParsedDraft {
    let body = strip_front_matter(draft);
    let mut title: Option<String> = None;
    let mut builder = Builder::default();
    let mut fence: Option<Vec<String>> = None;

    for raw in body.lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            match fence.take() {
                Some(lines) => builder.push(Block::Code(lines.join("\n"))),
                None => {
                    builder.flush();
                    fence = Some(Vec::new());
                }
            }
            continue;
        }
        if let Some(lines) = fence.as_mut() {
            lines.push(line.to_string());
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            builder.flush();
            continue;
        }
        if trimmed.starts_with('|') {
            builder.flush_paragraph();
            builder.table.push(trimmed.to_string());
            continue;
        }
        builder.flush_table();

        if is_rule(trimmed) {
            builder.flush();
            continue;
        }
        if let Some((level, text)) = heading(trimmed) {
            // A level-one heading opening the file is the paper's title,
            // not its first section.
            if title.is_none() && level == 1 && builder.untouched() {
                title = Some(text);
                continue;
            }
            builder.open(text, level);
            continue;
        }
        if let Some(item) = bullet(trimmed) {
            builder.flush_paragraph();
            builder.push(Block::Bullet(item));
            continue;
        }
        if is_ordered_item(trimmed) {
            builder.flush_paragraph();
            builder.push(Block::Paragraph(trimmed.to_string()));
            continue;
        }

        let text = trimmed.strip_prefix("> ").unwrap_or(trimmed);
        if line.starts_with(' ') || line.starts_with('\t') {
            if builder.continue_bullet(text) {
                continue;
            }
        }
        builder.paragraph.push(text.to_string());
    }
    if let Some(lines) = fence.take() {
        builder.push(Block::Code(lines.join("\n")));
    }

    let (lead, mut sections) = builder.finish();
    sections = drop_reference_sections(sections);

    let (abstract_text, leading_blocks) = split_abstract(&mut sections, lead);
    if !leading_blocks.is_empty() {
        sections.insert(
            0,
            Section {
                title: String::new(),
                level: 1,
                blocks: leading_blocks,
            },
        );
    }
    normalize_levels(&mut sections);

    ParsedDraft {
        title: title.unwrap_or_else(|| fallback_title.to_string()),
        abstract_text,
        sections,
    }
}

fn is_ordered_item(line: &str) -> bool {
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    digits > 0 && line[digits..].starts_with(". ")
}

/// Remove a reference section and everything nested under it. A stray
/// bibliography entry left behind as a stranded paragraph would still be
/// an uncheckable citation on the page.
fn drop_reference_sections(sections: Vec<Section>) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut dropping_at: Option<u8> = None;
    for section in sections {
        match dropping_at {
            Some(level) if section.level > level => continue,
            _ => dropping_at = None,
        }
        if is_reference_heading(&section.title) {
            dropping_at = Some(section.level);
            continue;
        }
        out.push(section);
    }
    out
}

fn blocks_to_text(blocks: &[Block]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(t) => Some(t.clone()),
            Block::Bullet(t) => Some(t.clone()),
            Block::Code(_) => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// An explicit `Abstract` section wins. Otherwise whatever sits between
/// the title and the first heading is the abstract, which is the shape a
/// short memo actually has.
fn split_abstract(sections: &mut Vec<Section>, lead: Vec<Block>) -> (String, Vec<Block>) {
    if let Some(at) = sections
        .iter()
        .position(|s| s.title.trim().eq_ignore_ascii_case("abstract"))
    {
        let section = sections.remove(at);
        return (blocks_to_text(&section.blocks), lead);
    }
    (blocks_to_text(&lead), Vec::new())
}

/// Shift heading levels so the shallowest one present is level 1. A
/// draft whose title was a `#` leaves every section at `##`, and
/// numbering those as `0.1` would be nonsense.
fn normalize_levels(sections: &mut [Section]) {
    let Some(min) = sections
        .iter()
        .filter(|s| !s.title.trim().is_empty())
        .map(|s| s.level)
        .min()
    else {
        return;
    };
    if min <= 1 {
        return;
    }
    for section in sections.iter_mut() {
        section.level = section.level.saturating_sub(min - 1).max(1);
    }
}

/// Assemble the document from a parsed draft and a record-derived
/// reference list. Pure, and the only place the paper's date is chosen:
/// it comes from the track's own creation time, so re-running deliver on
/// an unchanged track rewrites the same bytes rather than today's date.
fn build_parts(
    track: &zorp_track::track::Track,
    parsed: ParsedDraft,
    authors: &[String],
    references: Vec<Reference>,
) -> PaperParts {
    let at = date::from_millis(track.created_at);
    PaperParts {
        title: parsed.title,
        authors: authors.to_vec(),
        date: date::month_and_year(at),
        provenance: vec![format!("zorp track {}", track.id)],
        abstract_text: parsed.abstract_text,
        sections: parsed.sections,
        references,
    }
}

/// Build the paper, write `paper.md`, then try for `paper.pdf`, then
/// checkpoint. Refuses a killed track, a track with no draft, and a
/// track whose evidence record is empty.
pub fn run(
    project: &Project,
    track_id: &str,
    hypothesis: &str,
    authors: &[String],
    checkpoint_mode: &CheckpointMode,
) -> Result<PaperOutcome, DeliverError> {
    let track = project.store.get_track(track_id)?;
    if track.status == TrackStatus::Killed {
        return Err(DeliverError::TrackKilled);
    }

    let track_dir = project.track_dir(track_id);
    let draft_path = track_dir.join("draft.md");
    let draft = match std::fs::read_to_string(&draft_path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(DeliverError::NoDraft),
        Err(e) => return Err(e.into()),
    };

    let items = evidence::for_track(project, track_id)?;
    if items.is_empty() {
        return Err(DeliverError::NoEvidence);
    }
    let references: Vec<Reference> = items
        .into_iter()
        .map(|item| Reference {
            key: item.key,
            claim: item.claim,
            source: item.source,
        })
        .collect();

    let parsed = parse_draft(&draft, hypothesis);
    let parts = build_parts(&track, parsed, authors, references);
    let at = date::from_millis(track.created_at);
    let reference_count = parts.references.len();
    let paper = Paper::assemble(parts).map_err(DeliverError::Paper)?;

    std::fs::create_dir_all(&track_dir)?;
    let markdown_path = track_dir.join("paper.md");
    std::fs::write(&markdown_path, markdown::render(&paper))?;

    // Typesetting never sinks the delivery. The markdown above is the
    // artifact; the PDF is a rendering of it, and a rendering that
    // failed is worth a loud message and nothing more.
    let pdf_path = track_dir.join("paper.pdf");
    let options = PdfOptions {
        creation_date: Some(date::pdf_date(at)),
    };
    let (pdf_path, pdf_error) = match std::fs::write(&pdf_path, pdf::render(&paper, &options)) {
        Ok(()) => (Some(pdf_path), None),
        Err(e) => (None, Some(e.to_string())),
    };

    let prompt = match &pdf_error {
        None => format!(
            "deliver: paper written to {} and {} ({reference_count} references). Ready for review?",
            markdown_path.display(),
            pdf_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        ),
        Some(why) => format!(
            "deliver: paper written to {} ({reference_count} references). No PDF: {why}. Ready for review?",
            markdown_path.display()
        ),
    };
    let approved =
        project
            .store
            .record_checkpoint(track_id, "deliver-paper", checkpoint_mode, &prompt)?;

    Ok(PaperOutcome {
        markdown_path,
        pdf_path,
        pdf_error,
        reference_count,
        approved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;
    use zorp_track::experiment::{ExperimentStatus, MetricValue};

    fn project_with_draft(draft: &str) -> (tempfile::TempDir, Project) {
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
        let track_dir = project.track_dir("t1");
        std::fs::create_dir_all(&track_dir).unwrap();
        std::fs::write(track_dir.join("draft.md"), draft).unwrap();
        (dir, project)
    }

    fn yes() -> CheckpointMode {
        CheckpointMode::terminal(true).unwrap()
    }

    #[test]
    fn a_full_run_writes_both_artifacts() {
        let (_dir, project) = project_with_draft(
            "# Caching\n\nWe measured it.\n\n## Findings\n\nLatency fell [E1].\n",
        );

        let outcome = run(&project, "t1", "does caching help", &[], &yes()).unwrap();

        assert!(outcome.approved);
        assert_eq!(outcome.reference_count, 1);
        assert!(outcome.pdf_error.is_none(), "{:?}", outcome.pdf_error);
        let md = std::fs::read_to_string(&outcome.markdown_path).unwrap();
        assert!(md.contains("title: \"Caching\""), "{md}");
        assert!(md.contains("Latency fell [E1]."), "{md}");
        let pdf = std::fs::read(outcome.pdf_path.unwrap()).unwrap();
        assert!(pdf.starts_with(b"%PDF-1."));
    }

    #[test]
    fn a_draft_citing_evidence_the_record_does_not_have_is_refused() {
        let (_dir, project) = project_with_draft("# Caching\n\nLatency fell [E9].\n");

        let err = run(&project, "t1", "does caching help", &[], &yes()).unwrap_err();

        match err {
            DeliverError::Paper(zorp_paper::PaperError::UnknownCitations(keys)) => {
                assert_eq!(keys, vec!["E9".to_string()]);
            }
            other => panic!("expected an unknown-citation refusal, got {other:?}"),
        }
        assert!(
            !project.track_dir("t1").join("paper.md").exists(),
            "a paper with an unresolvable citation must not be written at all"
        );
    }

    #[test]
    fn a_reference_list_written_by_the_model_is_replaced_by_the_record() {
        let (_dir, project) = project_with_draft(
            "# Caching\n\nLatency fell.\n\n## References\n\n- Smith, J. (2020). Caching Considered.\n- Jones, A. (2019). More Caching.\n",
        );

        let outcome = run(&project, "t1", "does caching help", &[], &yes()).unwrap();

        let md = std::fs::read_to_string(&outcome.markdown_path).unwrap();
        assert!(!md.contains("Smith"), "invented reference survived:\n{md}");
        assert!(!md.contains("Jones"), "invented reference survived:\n{md}");
        assert!(md.contains("latency_ms = 42"), "{md}");
        assert_eq!(outcome.reference_count, 1);

        let pdf = std::fs::read(outcome.pdf_path.unwrap()).unwrap();
        let raw = String::from_utf8_lossy(&pdf);
        assert!(!raw.contains("Smith"), "invented reference reached the PDF");
    }

    #[test]
    fn a_nested_section_under_references_is_dropped_too() {
        let (_dir, project) = project_with_draft(
            "# Caching\n\nLatency fell.\n\n## References\n\n### Primary\n\nSmith, J. (2020).\n\n## Appendix\n\nKept.\n",
        );

        let outcome = run(&project, "t1", "does caching help", &[], &yes()).unwrap();

        let md = std::fs::read_to_string(&outcome.markdown_path).unwrap();
        assert!(!md.contains("Smith"), "{md}");
        assert!(md.contains("Appendix"), "{md}");
        assert!(md.contains("Kept."), "{md}");
    }

    #[test]
    fn the_abstract_comes_from_the_draft() {
        let (_dir, project) = project_with_draft(
            "# Caching\n\n## Abstract\n\nWe measured a cached path.\n\n## Findings\n\nIt was faster.\n",
        );

        let outcome = run(&project, "t1", "does caching help", &[], &yes()).unwrap();

        let md = std::fs::read_to_string(&outcome.markdown_path).unwrap();
        let abstract_section = md.split_once("# Abstract").unwrap().1;
        assert!(
            abstract_section.starts_with("\n\nWe measured a cached path."),
            "{md}"
        );
        // The explicit section is consumed, not left behind as a body
        // section as well.
        assert_eq!(md.matches("Abstract").count(), 1, "{md}");
    }

    #[test]
    fn text_before_the_first_heading_becomes_the_abstract() {
        let (_dir, project) = project_with_draft(
            "# Caching\n\nA one line summary.\n\n## Findings\n\nIt was faster.\n",
        );

        let outcome = run(&project, "t1", "does caching help", &[], &yes()).unwrap();

        let md = std::fs::read_to_string(&outcome.markdown_path).unwrap();
        assert!(md.contains("# Abstract\n\nA one line summary."), "{md}");
    }

    #[test]
    fn the_title_falls_back_to_the_hypothesis() {
        let (_dir, project) = project_with_draft("Latency fell by half.\n");

        let outcome = run(&project, "t1", "does caching help", &[], &yes()).unwrap();

        let md = std::fs::read_to_string(&outcome.markdown_path).unwrap();
        assert!(md.contains("title: \"does caching help\""), "{md}");
    }

    #[test]
    fn the_date_comes_from_the_track_and_not_from_the_clock() {
        // A track created in January 2003, which is not this month and
        // never will be. Reading the clock instead of the record puts
        // today's month here and fails.
        let track = zorp_track::track::Track {
            id: "t1".to_string(),
            hypothesis: "does caching help".to_string(),
            status: TrackStatus::Active,
            created_at: 1_041_476_645_000,
            updated_at: 1_041_476_645_000,
        };
        let parsed = parse_draft("# Caching\n\nLatency fell.\n", "does caching help");

        let parts = build_parts(&track, parsed, &[], vec![]);

        assert_eq!(parts.date, "January 2003");
        assert_eq!(parts.provenance, vec!["zorp track t1".to_string()]);
    }

    #[test]
    fn re_running_on_an_unchanged_track_writes_the_same_bytes() {
        let (_dir, project) = project_with_draft("# Caching\n\nLatency fell [E1].\n");

        let first = run(&project, "t1", "does caching help", &[], &yes()).unwrap();
        let first_pdf = std::fs::read(first.pdf_path.clone().unwrap()).unwrap();
        let first_md = std::fs::read(&first.markdown_path).unwrap();

        let second = run(&project, "t1", "does caching help", &[], &yes()).unwrap();
        let second_pdf = std::fs::read(second.pdf_path.unwrap()).unwrap();
        let second_md = std::fs::read(&second.markdown_path).unwrap();

        assert_eq!(first_pdf, second_pdf);
        assert_eq!(first_md, second_md);
    }

    #[test]
    fn a_pdf_that_cannot_be_written_still_delivers_the_markdown() {
        let (_dir, project) = project_with_draft("# Caching\n\nLatency fell.\n");
        // A directory where paper.pdf should go: the write fails, and
        // nothing else may.
        std::fs::create_dir_all(project.track_dir("t1").join("paper.pdf")).unwrap();

        let outcome = run(&project, "t1", "does caching help", &[], &yes()).unwrap();

        assert!(outcome.pdf_path.is_none());
        assert!(outcome.pdf_error.is_some(), "the reason must be reported");
        let md = std::fs::read_to_string(&outcome.markdown_path).unwrap();
        assert!(md.contains("latency_ms = 42"), "{md}");
    }

    struct CapturingDecider {
        prompt: Arc<Mutex<Option<String>>>,
    }

    impl zorp_track::checkpoint::Decider for CapturingDecider {
        fn decide(&self, prompt: &str) -> bool {
            *self.prompt.lock().unwrap() = Some(prompt.to_string());
            true
        }
    }

    #[test]
    fn the_checkpoint_says_why_there_is_no_pdf() {
        let (_dir, project) = project_with_draft("# Caching\n\nLatency fell.\n");
        std::fs::create_dir_all(project.track_dir("t1").join("paper.pdf")).unwrap();
        let captured = Arc::new(Mutex::new(None));
        let mode = CheckpointMode::Interactive(Arc::new(CapturingDecider {
            prompt: captured.clone(),
        }));

        run(&project, "t1", "does caching help", &[], &mode).unwrap();

        let prompt = captured.lock().unwrap().clone().expect("asked");
        assert!(prompt.contains("No PDF:"), "{prompt}");
        assert!(prompt.contains("1 references"), "{prompt}");
    }

    #[test]
    fn a_killed_track_is_refused() {
        let (_dir, project) = project_with_draft("# Caching\n\nLatency fell.\n");
        project
            .store
            .set_track_status("t1", TrackStatus::Killed)
            .unwrap();

        let err = run(&project, "t1", "does caching help", &[], &yes()).unwrap_err();
        assert!(matches!(err, DeliverError::TrackKilled));
    }

    #[test]
    fn a_track_with_no_draft_is_refused() {
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track("t1", "hyp").unwrap();

        let err = run(&project, "t1", "hyp", &[], &yes()).unwrap_err();
        assert!(matches!(err, DeliverError::NoDraft));
    }

    #[test]
    fn a_track_with_an_empty_record_is_refused() {
        let dir = tempdir().unwrap();
        let project = Project::open(dir.path()).unwrap();
        project.store.create_track("t1", "hyp").unwrap();
        let track_dir = project.track_dir("t1");
        std::fs::create_dir_all(&track_dir).unwrap();
        std::fs::write(track_dir.join("draft.md"), "# A draft\n\nWords.\n").unwrap();

        let err = run(&project, "t1", "hyp", &[], &yes()).unwrap_err();
        assert!(matches!(err, DeliverError::NoEvidence), "{err:?}");
    }

    #[test]
    fn authors_reach_the_front_matter_only_when_given() {
        let (_dir, project) = project_with_draft("# Caching\n\nLatency fell.\n");

        let outcome = run(&project, "t1", "does caching help", &[], &yes()).unwrap();
        let md = std::fs::read_to_string(&outcome.markdown_path).unwrap();
        assert!(!md.contains("author:"), "{md}");

        let outcome = run(
            &project,
            "t1",
            "does caching help",
            &["A. Researcher".to_string()],
            &yes(),
        )
        .unwrap();
        let md = std::fs::read_to_string(&outcome.markdown_path).unwrap();
        assert!(md.contains("author: \"A. Researcher\""), "{md}");
    }

    #[test]
    fn front_matter_on_the_draft_is_stripped_rather_than_typeset() {
        let (_dir, project) =
            project_with_draft("---\ntitle: raw\n---\n\n# Caching\n\nLatency fell.\n");

        let outcome = run(&project, "t1", "does caching help", &[], &yes()).unwrap();

        let md = std::fs::read_to_string(&outcome.markdown_path).unwrap();
        assert!(md.contains("title: \"Caching\""), "{md}");
        assert!(!md.contains("title: raw"), "{md}");
    }

    #[test]
    fn an_indented_line_under_a_bullet_stays_part_of_that_bullet() {
        let (_dir, project) = project_with_draft(
            "# Caching\n\nSummary.\n\n## Method\n\n- record whether a rebalance happened,\n  since one is enough\n- and then stop\n",
        );

        let outcome = run(&project, "t1", "does caching help", &[], &yes()).unwrap();

        let md = std::fs::read_to_string(&outcome.markdown_path).unwrap();
        assert!(
            md.contains(
                "- record whether a rebalance happened, since one is enough\n- and then stop"
            ),
            "the continuation broke out of its bullet:\n{md}"
        );
    }

    #[test]
    fn a_table_survives_as_a_verbatim_block() {
        let (_dir, project) = project_with_draft(
            "# Caching\n\nSummary.\n\n## Numbers\n\n| run | ms |\n| --- | -- |\n| 1 | 42 |\n",
        );

        let outcome = run(&project, "t1", "does caching help", &[], &yes()).unwrap();

        let md = std::fs::read_to_string(&outcome.markdown_path).unwrap();
        assert!(md.contains("```\n| run | ms |"), "{md}");
    }

    #[test]
    fn a_code_fence_in_the_draft_is_not_scanned_for_citations() {
        let (_dir, project) = project_with_draft(
            "# Caching\n\nSee below.\n\n## Code\n\n```\nlet rows = [1, 2, 9];\n```\n",
        );

        let outcome = run(&project, "t1", "does caching help", &[], &yes()).unwrap();

        let md = std::fs::read_to_string(&outcome.markdown_path).unwrap();
        assert!(md.contains("let rows = [1, 2, 9];"), "{md}");
    }

    #[test]
    fn sections_are_shifted_so_the_shallowest_is_level_one() {
        let (_dir, project) = project_with_draft(
            "# Caching\n\nSummary.\n\n## Findings\n\nFast.\n\n### Detail\n\nDetails.\n",
        );

        let outcome = run(&project, "t1", "does caching help", &[], &yes()).unwrap();

        let md = std::fs::read_to_string(&outcome.markdown_path).unwrap();
        assert!(md.contains("\n# Findings\n"), "{md}");
        assert!(md.contains("\n## Detail\n"), "{md}");
    }
}
