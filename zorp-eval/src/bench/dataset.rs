//! Benchmark items: fetched, cached outside the tree, and read into one shape.
//!
//! Nothing here is vendored. MMLU, MMLU-Pro, TruthfulQA and GSM8K are
//! fetched once from the Hugging Face datasets server and cached under the
//! user's cache directory (`ZORP_BENCH_CACHE` overrides it), for size and
//! for licensing: `reference/` is gitignored for the same reason, its license
//! does not permit redistribution under zorp's terms. GPQA is never fetched
//! at all; see `case.rs` and [`refuse_gpqa_inside_the_tree`].
//!
//! Every format becomes an [`Item`]: a question and a key. The loaders are
//! strict. A row missing its answer is an error naming the row, never an
//! item quietly dropped, because a benchmark that lost a tenth of its items
//! to a schema change reports a number over a different benchmark.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::case::{Format, Loaded};

/// Where the datasets server lives. A parameter everywhere below so a test
/// can serve pages from loopback.
pub const DATASETS_SERVER: &str = "https://datasets-server.huggingface.co";

/// The most rows the datasets server returns in one page.
const PAGE: usize = 100;

/// One benchmark item.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// Stable within a dataset: the row index the hub assigned, or the
    /// dataset's own id column when it has one.
    pub id: String,
    pub question: String,
    pub key: Key,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    /// Options in the order they are shown, and the index of the right one.
    Choice {
        options: Vec<String>,
        correct: usize,
    },
    /// The number the answer has to equal, as the dataset wrote it.
    Number(String),
}

impl Key {
    /// The key as the table's `expected` column holds it: a letter or a
    /// number, never the option's text.
    pub fn expected(&self) -> String {
        match self {
            Key::Choice { correct, .. } => letter(*correct).to_string(),
            Key::Number(n) => n.clone(),
        }
    }
}

/// `A` for 0, `B` for 1, up to `J`, the most options any format here has.
pub fn letter(index: usize) -> char {
    (b'A' + index as u8) as char
}

/// The default cache directory: `ZORP_BENCH_CACHE` when set, else
/// `<user cache dir>/zorp/bench`. Read once, before bench clears the
/// environment.
pub fn default_cache_dir() -> anyhow::Result<PathBuf> {
    if let Some(dir) = std::env::var_os("ZORP_BENCH_CACHE").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    dirs::cache_dir()
        .map(|dir| dir.join("zorp").join("bench"))
        .ok_or_else(|| {
            anyhow::anyhow!("no cache directory on this system; set ZORP_BENCH_CACHE or --cache")
        })
}

/// The items one case runs, after its limit.
pub fn load(case: &Loaded, cache: &Path, server: &str) -> anyhow::Result<Vec<Item>> {
    let source = &case.case.source;
    let rows = if let Some(dataset) = &source.huggingface {
        let config = source.config.as_deref().unwrap_or("default");
        let split = source.split.as_deref().unwrap_or("test");
        cached_hub_rows(server, cache, dataset, config, split)?
    } else {
        let path = resolve(
            &case.dir,
            source.path.as_deref().expect("checked in case.rs"),
        );
        if case.case.format == Format::Gpqa {
            refuse_gpqa_inside_the_tree(&path)?;
        }
        read_file_rows(&path)?
    };
    let format = case.case.format;
    let mut items = rows
        .into_iter()
        .map(|(id, row)| {
            item(format, &id, &row).map_err(|e| anyhow::anyhow!("{}: row {id}: {e}", case.name))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    if items.is_empty() {
        anyhow::bail!("{}: the dataset has no rows", case.name);
    }
    if let Some(limit) = case.case.limit {
        items = sample(&case.name, items, limit);
    }
    Ok(items)
}

/// `limit` items chosen by hashing each id, kept in dataset order.
fn sample(benchmark: &str, items: Vec<Item>, limit: usize) -> Vec<Item> {
    if limit >= items.len() {
        return items;
    }
    let mut ranked: Vec<(Vec<u8>, usize)> = items
        .iter()
        .enumerate()
        .map(|(i, item)| (digest(&[benchmark, &item.id]).to_vec(), i))
        .collect();
    ranked.sort();
    let mut keep: Vec<usize> = ranked.into_iter().take(limit).map(|(_, i)| i).collect();
    keep.sort_unstable();
    let mut items: Vec<Option<Item>> = items.into_iter().map(Some).collect();
    keep.into_iter()
        .map(|i| items[i].take().expect("each index once"))
        .collect()
}

fn digest(parts: &[&str]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0u8]);
    }
    hasher.finalize().into()
}

fn resolve(dir: &Path, path: &Path) -> PathBuf {
    if let Ok(rest) = path.strip_prefix("~") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    dir.join(path)
}

/// GPQA's authors ask that its items not appear anywhere they could be
/// scraped, and a file inside this repository is one `git add .` from a
/// public commit. So a GPQA path inside the tree is refused outright,
/// fixtures included: the one in the test suite is invented text and is
/// copied to a temporary directory before it is read.
pub fn refuse_gpqa_inside_the_tree(path: &Path) -> anyhow::Result<()> {
    let tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let (Ok(tree), Ok(path)) = (tree.canonicalize(), path.canonicalize()) else {
        // A path that does not exist fails on read with a better message.
        return Ok(());
    };
    if path.starts_with(&tree) {
        anyhow::bail!(
            "{}: GPQA must not live inside the zorp repository ({}). It is gated \
             and carries canary strings; keep it where it cannot be committed",
            path.display(),
            tree.display()
        );
    }
    Ok(())
}

/// Rows from a local file, each with its id.
fn read_file_rows(path: &Path) -> anyhow::Result<Vec<(String, Value)>> {
    let is_csv = path.extension().is_some_and(|e| e == "csv");
    let rows = if is_csv {
        read_csv(path)
    } else {
        read_jsonl(path)
    };
    rows.map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}

fn read_jsonl(path: &Path) -> anyhow::Result<Vec<(String, Value)>> {
    let file = std::fs::File::open(path)?;
    let mut rows = Vec::new();
    for (n, line) in std::io::BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let row: Value =
            serde_json::from_str(&line).map_err(|e| anyhow::anyhow!("line {}: {e}", n + 1))?;
        let id = own_id(&row).unwrap_or_else(|| rows.len().to_string());
        rows.push((id, row));
    }
    Ok(rows)
}

fn read_csv(path: &Path) -> anyhow::Result<Vec<(String, Value)>> {
    let mut reader = csv::Reader::from_path(path)?;
    let headers = reader.headers()?.clone();
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record?;
        let row: serde_json::Map<String, Value> = headers
            .iter()
            .zip(record.iter())
            .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
            .collect();
        let row = Value::Object(row);
        let id = own_id(&row).unwrap_or_else(|| rows.len().to_string());
        rows.push((id, row));
    }
    Ok(rows)
}

/// A dataset's own id column, when it has one: GPQA's `Record ID`, MMLU-Pro's
/// `question_id`. Preferred over a row index because it survives a reorder.
fn own_id(row: &Value) -> Option<String> {
    ["Record ID", "question_id"]
        .iter()
        .find_map(|k| match row.get(*k)? {
            Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
}

/// Hub rows from the cache, fetching them first if they are not there.
fn cached_hub_rows(
    server: &str,
    cache: &Path,
    dataset: &str,
    config: &str,
    split: &str,
) -> anyhow::Result<Vec<(String, Value)>> {
    let path = cache
        .join("huggingface")
        .join(dataset)
        .join(config)
        .join(format!("{split}.jsonl"));
    if !path.exists() {
        eprintln!(
            "bench: fetching {dataset} ({config}/{split}) into {}",
            path.display()
        );
        let rows = fetch_hub_rows(server, dataset, config, split)?;
        write_cache(&path, &rows)?;
    }
    read_jsonl(&path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}

/// Written whole to a temporary file and renamed, so an interrupted fetch
/// leaves no cache file rather than a short one that reads as the dataset.
fn write_cache(path: &Path, rows: &[Value]) -> anyhow::Result<()> {
    let dir = path.parent().expect("a cache path has a parent");
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    for row in rows {
        serde_json::to_writer(&mut tmp, row)?;
        tmp.write_all(b"\n")?;
    }
    tmp.flush()?;
    tmp.persist(path)?;
    Ok(())
}

/// Every row of one split, a page at a time. The row index the server
/// assigned is written into the row as `row_idx` so the id survives the
/// cache.
pub fn fetch_hub_rows(
    server: &str,
    dataset: &str,
    config: &str,
    split: &str,
) -> anyhow::Result<Vec<Value>> {
    let mut rows = Vec::new();
    let mut offset = 0usize;
    loop {
        let url = format!(
            "{}/rows?dataset={}&config={}&split={}&offset={offset}&length={PAGE}",
            server.trim_end_matches('/'),
            encode(dataset),
            encode(config),
            encode(split),
        );
        let page = get_json(&url)?;
        if page.get("partial").and_then(Value::as_bool) == Some(true) {
            // The server only indexes part of a large split. A benchmark over
            // whichever part it indexed is not the benchmark.
            anyhow::bail!("{url}: the datasets server only has part of this split");
        }
        let total = page
            .get("num_rows_total")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow::anyhow!("{url}: no num_rows_total in the reply"))?
            as usize;
        let page_rows = page
            .get("rows")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("{url}: no rows in the reply"))?;
        for entry in page_rows {
            if entry
                .get("truncated_cells")
                .and_then(Value::as_array)
                .is_some_and(|cells| !cells.is_empty())
            {
                anyhow::bail!(
                    "{url}: the server truncated a cell, so a question or key is cut short"
                );
            }
            let index = entry.get("row_idx").and_then(Value::as_u64);
            let mut row = entry
                .get("row")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("{url}: an entry with no row"))?;
            if let (Some(index), Some(fields)) = (index, row.as_object_mut()) {
                fields.insert("row_idx".into(), Value::from(index));
            }
            rows.push(row);
        }
        offset += page_rows.len();
        if offset >= total || page_rows.is_empty() {
            break;
        }
    }
    Ok(rows)
}

/// A GET through the workspace's one HTTP agent, sent again a few times while
/// the server says it is rate limiting. Fetching is not a measurement, so
/// its patience is fixed here rather than taken from any case.
fn get_json(url: &str) -> anyhow::Result<Value> {
    let mut wait = std::time::Duration::from_secs(2);
    for attempt in 1.. {
        match zorp::http_agent()
            .get(url)
            .timeout(std::time::Duration::from_secs(120))
            .call()
        {
            Ok(resp) => return Ok(resp.into_json()?),
            Err(ureq::Error::Status(429 | 502 | 503, _)) if attempt < 6 => {
                eprintln!("bench: {url}: busy, waiting {}s", wait.as_secs());
                std::thread::sleep(wait);
                wait *= 2;
            }
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                anyhow::bail!("{url}: status {code}: {}", body.trim());
            }
            Err(e) => anyhow::bail!("{url}: {e}"),
        }
    }
    unreachable!("the loop returns or bails")
}

fn encode(part: &str) -> String {
    part.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// One row, read as `format` says.
fn item(format: Format, id: &str, row: &Value) -> anyhow::Result<Item> {
    let id = row
        .get("row_idx")
        .and_then(Value::as_u64)
        .filter(|_| own_id(row).is_none())
        .map(|n| n.to_string())
        .unwrap_or_else(|| id.to_string());
    let text = |field: &str| -> anyhow::Result<String> {
        row.get(field)
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("no {field:?}"))
    };
    let strings = |value: Option<&Value>, field: &str| -> anyhow::Result<Vec<String>> {
        value
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("no {field:?} list"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| anyhow::anyhow!("{field:?} holds a non-string"))
            })
            .collect()
    };
    let index = |field: &str, options: usize| -> anyhow::Result<usize> {
        let value = row
            .get(field)
            .ok_or_else(|| anyhow::anyhow!("no {field:?}"))?;
        let index = match value {
            Value::Number(n) => n.as_u64().map(|n| n as usize),
            // Some exports write the class label as its letter.
            Value::String(s) if s.len() == 1 => {
                let c = s.as_bytes()[0].to_ascii_uppercase();
                c.checked_sub(b'A').map(usize::from)
            }
            _ => None,
        };
        index
            .filter(|i| *i < options)
            .ok_or_else(|| anyhow::anyhow!("{field:?} is {value}, not one of {options} options"))
    };
    let item = match format {
        Format::Mmlu => {
            let options = strings(row.get("choices"), "choices")?;
            let correct = index("answer", options.len())?;
            Item {
                id,
                question: text("question")?,
                key: Key::Choice { options, correct },
            }
        }
        Format::MmluPro => {
            let options = strings(row.get("options"), "options")?;
            let correct = index("answer_index", options.len())?;
            Item {
                id,
                question: text("question")?,
                key: Key::Choice { options, correct },
            }
        }
        Format::TruthfulqaMc1 => {
            let targets = row
                .get("mc1_targets")
                .ok_or_else(|| anyhow::anyhow!("no \"mc1_targets\""))?;
            let options = strings(targets.get("choices"), "mc1_targets.choices")?;
            let labels: Vec<i64> = targets
                .get("labels")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow::anyhow!("no \"mc1_targets.labels\""))?
                .iter()
                .map(|v| v.as_i64().unwrap_or(-1))
                .collect();
            let true_ones: Vec<usize> = labels
                .iter()
                .enumerate()
                .filter(|(_, l)| **l == 1)
                .map(|(i, _)| i)
                .collect();
            if labels.len() != options.len() || true_ones.len() != 1 {
                anyhow::bail!("mc1_targets needs one label per choice and exactly one true");
            }
            // The true option is always first in the dataset. Shown in that
            // order, "always answer A" would score perfectly.
            shuffled(&id, text("question")?, options, true_ones[0])
        }
        Format::Gpqa => {
            let mut options = vec![text("Correct Answer")?];
            for n in 1..=3 {
                options.push(text(&format!("Incorrect Answer {n}"))?);
            }
            // The same problem as TruthfulQA: the CSV puts the right answer
            // in its own column, so the order shown has to be decided here.
            shuffled(&id, text("Question")?, options, 0)
        }
        Format::Gsm8k => {
            let solution = text("answer")?;
            let key = solution
                .rsplit_once("####")
                .map(|(_, n)| n.trim().replace(',', ""))
                .filter(|n| super::grade::parse_number(n).is_some())
                .ok_or_else(|| anyhow::anyhow!("\"answer\" does not end in #### <number>"))?;
            Item {
                id,
                question: text("question")?,
                key: Key::Number(key),
            }
        }
    };
    if let Key::Choice { options, .. } = &item.key {
        if options.len() < 2 || options.len() > 10 {
            anyhow::bail!("{} options; bench grades 2 to 10", options.len());
        }
    }
    Ok(item)
}

/// The options in an order fixed by the item's id: the same item is shown the
/// same way on every run and every runtime, so a paired comparison compares
/// like with like, and no position is favoured across the dataset.
fn shuffled(id: &str, question: String, options: Vec<String>, correct: usize) -> Item {
    let seed = digest(&["bench-option-order", id]);
    let mut state = u64::from_le_bytes(seed[..8].try_into().expect("32 bytes"));
    let mut order: Vec<usize> = (0..options.len()).collect();
    for i in (1..order.len()).rev() {
        // splitmix64: small, fixed, and not something a dependency bump can
        // change the output of.
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        order.swap(i, (z % (i as u64 + 1)) as usize);
    }
    let correct = order
        .iter()
        .position(|&i| i == correct)
        .expect("a permutation");
    let options = order.into_iter().map(|i| options[i].clone()).collect();
    Item {
        id: id.to_string(),
        question,
        key: Key::Choice { options, correct },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::case;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/bench")
            .join(name)
    }

    fn loaded(format: &str, path: &Path, limit: Option<usize>) -> Loaded {
        let mut text = format!("format = \"{format}\"\n");
        if let Some(limit) = limit {
            text.push_str(&format!("limit = {limit}\n"));
        }
        text.push_str(&format!(
            "[source]\npath = {:?}\n",
            path.display().to_string()
        ));
        Loaded {
            name: format.to_string(),
            dir: PathBuf::from("/"),
            case: case::parse(&text).unwrap(),
        }
    }

    #[test]
    fn every_format_loads_its_fixture() {
        let cache = tempfile::tempdir().unwrap();
        for (format, file, first_key) in [
            ("mmlu", "mmlu.jsonl", "B"),
            ("mmlu_pro", "mmlu_pro.jsonl", "C"),
            ("gsm8k", "gsm8k.jsonl", "12"),
        ] {
            let items = load(&loaded(format, &fixture(file), None), cache.path(), "").unwrap();
            assert!(items.len() >= 2, "{format}");
            assert_eq!(items[0].key.expected(), first_key, "{format}");
        }
    }

    #[test]
    fn truthfulqa_does_not_leave_the_true_option_first_every_time() {
        let items = load(
            &loaded("truthfulqa_mc1", &fixture("truthfulqa_mc1.jsonl"), None),
            Path::new("/nonexistent"),
            "",
        )
        .unwrap();
        let positions: std::collections::BTreeSet<String> =
            items.iter().map(|i| i.key.expected()).collect();
        assert!(positions.len() > 1, "every key is {positions:?}");
        // And the order is the same every time it is loaded.
        let again = load(
            &loaded("truthfulqa_mc1", &fixture("truthfulqa_mc1.jsonl"), None),
            Path::new("/nonexistent"),
            "",
        )
        .unwrap();
        assert_eq!(items, again);
    }

    #[test]
    fn gpqa_reads_the_csv_as_distributed_from_outside_the_tree() {
        let outside = tempfile::tempdir().unwrap();
        let copy = outside.path().join("gpqa_fixture.csv");
        std::fs::copy(fixture("gpqa_fixture.csv"), &copy).unwrap();
        let items = load(&loaded("gpqa", &copy, None), outside.path(), "").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "recFIXTURE1");
        let Key::Choice { options, correct } = &items[0].key else {
            panic!("gpqa is multiple choice");
        };
        assert_eq!(options.len(), 4);
        assert_eq!(options[*correct], "Invented correct answer one");
    }

    #[test]
    fn gpqa_inside_the_repository_is_refused() {
        let error = load(
            &loaded("gpqa", &fixture("gpqa_fixture.csv"), None),
            Path::new("/nonexistent"),
            "",
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("must not live inside"), "{error}");
    }

    #[test]
    fn a_limit_takes_the_same_items_every_time_in_dataset_order() {
        let path = fixture("mmlu.jsonl");
        let all = load(&loaded("mmlu", &path, None), Path::new("/x"), "").unwrap();
        let some = load(&loaded("mmlu", &path, Some(2)), Path::new("/x"), "").unwrap();
        let again = load(&loaded("mmlu", &path, Some(2)), Path::new("/x"), "").unwrap();
        assert_eq!(some.len(), 2);
        assert_eq!(some, again);
        let positions: Vec<usize> = some
            .iter()
            .map(|s| all.iter().position(|a| a.id == s.id).unwrap())
            .collect();
        assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
    }

    #[test]
    fn a_row_without_its_key_is_an_error_naming_the_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.jsonl");
        std::fs::write(
            &path,
            "{\"question\":\"q\",\"choices\":[\"a\",\"b\"],\"answer\":0}\n{\"question\":\"q\",\"choices\":[\"a\",\"b\"]}\n",
        )
        .unwrap();
        let error = load(&loaded("mmlu", &path, None), dir.path(), "")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("row 1") && error.contains("answer"),
            "{error}"
        );
    }

    #[test]
    fn hub_rows_are_fetched_a_page_at_a_time_and_cached() {
        use zorp_stub::{scripted_server, Framing, Reply};
        let page = |rows: &str, total: usize| -> &'static str {
            Box::leak(
                format!("{{\"rows\":[{rows}],\"num_rows_total\":{total},\"partial\":false}}")
                    .into_boxed_str(),
            )
        };
        let row = |i: usize, answer: usize| {
            format!(
                "{{\"row_idx\":{i},\"row\":{{\"question\":\"invented {i}\",\"choices\":[\"w\",\"x\",\"y\",\"z\"],\"answer\":{answer}}},\"truncated_cells\":[]}}"
            )
        };
        // Two pages; the server's page size is taken from what came back,
        // so a short first page still reaches the end.
        let first = [row(0, 1), row(1, 2)].join(",");
        let second = row(2, 3);
        let (address, connections) = scripted_server(
            Framing::CloseDelimited,
            vec![
                Reply::Status {
                    code: 200,
                    retry_after: None,
                    body: page(&first, 3),
                },
                Reply::Status {
                    code: 200,
                    retry_after: None,
                    body: page(&second, 3),
                },
            ],
        );
        let cache = tempfile::tempdir().unwrap();
        let text = "format = \"mmlu\"\n[source]\nhuggingface = \"invented/set\"\nconfig = \"all\"\nsplit = \"test\"\n";
        let case = Loaded {
            name: "mmlu".into(),
            dir: PathBuf::from("/"),
            case: case::parse(text).unwrap(),
        };
        let server = format!("http://{address}");
        let items = load(&case, cache.path(), &server).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[2].id, "2");
        assert_eq!(items[2].key.expected(), "D");
        assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(cache
            .path()
            .join("huggingface/invented/set/all/test.jsonl")
            .exists());
        // Cached: a second load sends nothing.
        let again = load(&case, cache.path(), &server).unwrap();
        assert_eq!(items, again);
        assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
