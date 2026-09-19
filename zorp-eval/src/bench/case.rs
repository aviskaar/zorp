//! One bench case file, read.
//!
//! A bench case names one benchmark: where its items come from, which answer
//! format they are graded in, and the bounds each request runs under. Like a
//! `harness` case it is data and nothing else, and for the same reason an
//! unknown field is an error rather than a silent skip: a misspelled `limit`
//! that is quietly dropped is a run over fourteen thousand items that was
//! meant to be over two hundred, and a misspelled bound is a latency column
//! measured under a bound nobody chose.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// One benchmark, as a case file states it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// The benchmark's name in the table. Defaults to the file stem.
    pub name: Option<String>,
    /// Prose for whoever reads the file. The runner never looks at it.
    pub about: Option<String>,
    /// How items are read and graded.
    pub format: Format,
    pub source: Source,
    /// Run this many items rather than all of them. The subset is chosen by
    /// hashing each item's id, not by taking the first rows, because MMLU is
    /// sorted by subject and its first two hundred rows are two subjects.
    /// The same limit over the same dataset is always the same items.
    pub limit: Option<usize>,
    #[serde(default)]
    pub bounds: Bounds,
}

/// The answer formats bench can grade, each with its own loader. Every one
/// of them is graded by code, against a key, with no model in the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// `cais/mmlu` rows: `question`, `choices`, `answer` as an index.
    Mmlu,
    /// `TIGER-Lab/MMLU-Pro` rows: `question`, `options` (up to ten),
    /// `answer_index`.
    MmluPro,
    /// The GPQA CSV as distributed: `Question`, `Correct Answer` and three
    /// `Incorrect Answer` columns. Local files only; see [`Case::check`].
    Gpqa,
    /// `truthfulqa/truthful_qa` `multiple_choice` rows, graded on
    /// `mc1_targets`: exactly one true option.
    TruthfulqaMc1,
    /// `openai/gsm8k` rows: `question`, and an `answer` whose last line is
    /// `#### <number>`.
    Gsm8k,
}

/// Where the items come from. Exactly one of `huggingface` and `path`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// A dataset id on the Hugging Face hub, fetched once through the
    /// datasets server and cached outside the tree.
    pub huggingface: Option<String>,
    /// The dataset config, for a `huggingface` source.
    pub config: Option<String>,
    /// The split, for a `huggingface` source.
    pub split: Option<String>,
    /// A local file: `.jsonl` rows in the same shape the hub serves, or a
    /// `.csv` with a header row. Relative to the case file; `~/` is the home
    /// directory.
    pub path: Option<PathBuf>,
}

/// What one request is allowed. A case states these instead of inheriting
/// them, because bench clears every `ZORP_` variable before it sends
/// anything: a latency number means nothing if the developer's shell chose
/// the retry bound and the read timeout.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    /// A ceiling on one request, connect to last byte. A request that hits
    /// it is unevaluable, not wrong.
    #[serde(default = "Bounds::default_timeout_secs")]
    pub timeout_secs: u64,
    /// Sends allowed for one request, counting the first, when the provider
    /// asks to be asked again (429, 502, 503). The same policy `zorp::send_json`
    /// applies, stated here rather than read from the environment.
    #[serde(default = "Bounds::default_retry_attempts")]
    pub retry_attempts: u32,
    /// The most backoff one request may add, summed over its waits.
    #[serde(default = "Bounds::default_retry_budget_secs")]
    pub retry_budget_secs: u64,
    /// Sent as `max_tokens` when set. Left unset, an OpenAI-compatible
    /// request carries no limit, which is what `zorp-agent` sends too, and an
    /// Anthropic one carries 4096 because that API requires one. A reply cut
    /// off at the limit is unevaluable: the answer did not happen.
    pub max_tokens: Option<u32>,
}

impl Bounds {
    fn default_timeout_secs() -> u64 {
        300
    }
    fn default_retry_attempts() -> u32 {
        zorp::DEFAULT_RETRY_ATTEMPTS
    }
    fn default_retry_budget_secs() -> u64 {
        zorp::DEFAULT_RETRY_BUDGET_SECS
    }

    pub fn retry_policy(&self) -> zorp::RetryPolicy {
        zorp::RetryPolicy {
            attempts: self.retry_attempts.max(1),
            budget: std::time::Duration::from_secs(self.retry_budget_secs),
        }
    }
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            timeout_secs: Self::default_timeout_secs(),
            retry_attempts: Self::default_retry_attempts(),
            retry_budget_secs: Self::default_retry_budget_secs(),
            max_tokens: None,
        }
    }
}

/// A case, read and checked, with the name it goes by and the directory its
/// relative paths are resolved against.
#[derive(Debug)]
pub struct Loaded {
    pub name: String,
    pub dir: PathBuf,
    pub case: Case,
}

pub fn load(path: &Path) -> anyhow::Result<Loaded> {
    let text =
        std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let case = parse(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let name = case.name.clone().unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let dir = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(Loaded { name, dir, case })
}

pub fn parse(text: &str) -> anyhow::Result<Case> {
    let case: Case = toml::from_str(text)?;
    case.check()?;
    Ok(case)
}

/// Every `.toml` case in `dir`, in file name order. A directory with none is
/// an error: a bench run that exits having measured nothing would print an
/// empty table, and an empty table reads as "nothing to report".
pub fn load_dir(dir: &Path) -> anyhow::Result<Vec<Loaded>> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    if paths.is_empty() {
        anyhow::bail!("no .toml bench cases in {}", dir.display());
    }
    let cases = paths
        .iter()
        .map(|p| load(p))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let mut seen = std::collections::BTreeSet::new();
    for case in &cases {
        if !seen.insert(case.name.as_str()) {
            anyhow::bail!(
                "two bench cases in {} are both called {:?}; one table row each would be one row for both",
                dir.display(),
                case.name
            );
        }
    }
    Ok(cases)
}

impl Case {
    fn check(&self) -> anyhow::Result<()> {
        let source = &self.source;
        match (&source.huggingface, &source.path) {
            (Some(_), Some(_)) => anyhow::bail!("source: huggingface and path cannot both be set"),
            (None, None) => anyhow::bail!("source: set one of huggingface or path"),
            (Some(dataset), None) => {
                if source.split.is_none() {
                    anyhow::bail!("source: a huggingface source needs a split");
                }
                if dataset
                    .split('/')
                    .any(|part| part.is_empty() || part == "..")
                {
                    anyhow::bail!("source: {dataset:?} is not a dataset id");
                }
                // GPQA is gated on purpose: its authors ask that it not be
                // redistributed, and they put canary strings in it so a
                // training corpus that swallowed it can be detected. Fetching
                // it here would mean a token, a cache of it on disk that
                // nobody chose to keep, and a copy one careless path away
                // from being committed. The user downloads it, agrees to the
                // terms, and points a case at the file.
                if self.format == Format::Gpqa {
                    anyhow::bail!(
                        "source: GPQA is gated and is never downloaded by bench. \
                         Accept its terms, download it yourself, and set path to the CSV \
                         (outside this repository)"
                    );
                }
            }
            (None, Some(_)) => {
                if source.config.is_some() || source.split.is_some() {
                    anyhow::bail!(
                        "source: config and split are read only for a huggingface source"
                    );
                }
            }
        }
        if self.limit == Some(0) {
            anyhow::bail!("limit: 0 items is a run that measures nothing");
        }
        if self.bounds.timeout_secs == 0 {
            anyhow::bail!("bounds.timeout_secs: 0 would time out every request");
        }
        if self.bounds.max_tokens == Some(0) {
            anyhow::bail!("bounds.max_tokens: 0 leaves no room for an answer");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
format = "gsm8k"
[source]
huggingface = "openai/gsm8k"
config = "main"
split = "test"
"#;

    #[test]
    fn a_minimal_case_parses_with_default_bounds() {
        let case = parse(MINIMAL).unwrap();
        assert_eq!(case.format, Format::Gsm8k);
        assert_eq!(case.bounds.timeout_secs, 300);
        assert_eq!(case.bounds.retry_attempts, zorp::DEFAULT_RETRY_ATTEMPTS);
        assert_eq!(case.bounds.max_tokens, None);
    }

    #[test]
    fn an_unknown_field_is_an_error_and_not_a_silent_skip() {
        let text = format!("limt = 200\n{MINIMAL}");
        let error = parse(&text).unwrap_err().to_string();
        assert!(error.contains("limt"), "{error}");
    }

    #[test]
    fn an_unknown_field_in_a_nested_table_is_an_error_too() {
        let text = format!("{MINIMAL}\n[bounds]\ntimeout = 5\n");
        let error = parse(&text).unwrap_err().to_string();
        assert!(error.contains("timeout"), "{error}");
        let text = MINIMAL.replace("split = \"test\"", "split = \"test\"\nrevision = \"x\"");
        let error = parse(&text).unwrap_err().to_string();
        assert!(error.contains("revision"), "{error}");
    }

    #[test]
    fn an_unknown_format_is_an_error() {
        let text = MINIMAL.replace("gsm8k\"\n[source]", "humaneval\"\n[source]");
        let error = parse(&text).unwrap_err().to_string();
        assert!(error.contains("humaneval"), "{error}");
    }

    #[test]
    fn an_empty_case_directory_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        // A file that is not a case does not count as one.
        std::fs::write(dir.path().join("manifest.example.yaml"), "x").unwrap();
        let error = load_dir(dir.path()).unwrap_err().to_string();
        assert!(error.contains("no .toml bench cases"), "{error}");
    }

    #[test]
    fn gpqa_is_never_fetched_from_the_hub() {
        let text = MINIMAL
            .replace("gsm8k\"", "gpqa\"")
            .replace("openai/gsm8k", "Idavidrein/gpqa");
        let error = parse(&text).unwrap_err().to_string();
        assert!(error.contains("gated"), "{error}");
    }

    #[test]
    fn a_source_is_one_thing() {
        let both = format!("{MINIMAL}path = \"x.jsonl\"\n");
        assert!(parse(&both).unwrap_err().to_string().contains("both"));
        let neither = "format = \"mmlu\"\n[source]\n";
        assert!(parse(neither).unwrap_err().to_string().contains("one of"));
        let stray = "format = \"mmlu\"\n[source]\npath = \"x.jsonl\"\nsplit = \"test\"\n";
        assert!(parse(stray).unwrap_err().to_string().contains("split"));
    }

    #[test]
    fn a_limit_of_zero_is_an_error() {
        let text = format!("limit = 0\n{MINIMAL}");
        assert!(parse(&text).unwrap_err().to_string().contains("limit"));
    }

    #[test]
    fn two_cases_with_one_name_are_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.toml"),
            format!("name = \"x\"\n{MINIMAL}"),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("b.toml"),
            format!("name = \"x\"\n{MINIMAL}"),
        )
        .unwrap();
        let error = load_dir(dir.path()).unwrap_err().to_string();
        assert!(error.contains("both called"), "{error}");
    }

    /// The cases that ship parse, and none of them is GPQA: a default run
    /// must not need a file nobody downloaded for them.
    #[test]
    fn the_shipped_cases_parse() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("evals/bench");
        let cases = load_dir(&dir).unwrap();
        let names: Vec<&str> = cases.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["gsm8k", "mmlu-pro", "mmlu", "truthfulqa-mc1"]);
        assert!(cases.iter().all(|c| c.case.format != Format::Gpqa));
        let example = std::fs::read_to_string(dir.join("gpqa.toml.example")).unwrap();
        assert_eq!(parse(&example).unwrap().format, Format::Gpqa);
    }
}
