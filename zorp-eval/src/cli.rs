use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the existing grader-based eval suite.
    Eval {
        #[arg(short, long)]
        suite: String,
    },
    /// Run a manifest-driven paired behavioral-compatibility experiment.
    Compat {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        tasks_dir: PathBuf,
        #[arg(long, default_value = "evals/results/telemetry.db")]
        db: PathBuf,
        #[arg(long, default_value = "../target/release/zorp-agent")]
        agent_binary: PathBuf,
    },
    /// Run the deterministic harness suite: the real agent binary against a
    /// scripted provider on loopback. No network, no key, no model.
    Harness {
        #[arg(long, default_value = "evals/harness")]
        cases: PathBuf,
        #[arg(long, default_value = "../target/debug/zorp-agent")]
        agent_binary: PathBuf,
    },
    /// Run public benchmarks (MMLU, MMLU-Pro, GPQA, TruthfulQA, GSM8K)
    /// against every runtime in a manifest and print one table. Live models
    /// and real network: it reports, and never gates anything.
    Bench {
        /// The manifest whose reference and candidates are the runtimes.
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long, default_value = "evals/bench")]
        cases: PathBuf,
        #[arg(long, default_value = "evals/results/telemetry.db")]
        db: PathBuf,
        /// Where fetched datasets are cached. Defaults to ZORP_BENCH_CACHE,
        /// then the user's cache directory under zorp/bench.
        #[arg(long)]
        cache: Option<PathBuf>,
    },
}
