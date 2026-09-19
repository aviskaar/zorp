use clap::Parser;
use zorp_eval::{bench, harness, runner};
mod cli;

fn main() -> anyhow::Result<()> {
    let args = cli::Cli::parse();
    match args.command {
        cli::Command::Eval { suite } => {
            // No grader pipeline is wired up yet. Exiting successfully here
            // would look like a completed eval that never ran, so refuse.
            anyhow::bail!(
                "the eval subcommand is not implemented yet: no graders were run for suite '{suite}'. \
                 Use the compat subcommand to run a contract-based experiment."
            );
        }
        cli::Command::Compat {
            manifest,
            tasks_dir,
            db,
            agent_binary,
        } => {
            runner::run_suite(&manifest, &tasks_dir, &db, &agent_binary)?;
            println!(
                "Compatibility experiment complete. Results in {}",
                db.display()
            );
        }
        cli::Command::Harness {
            cases,
            agent_binary,
        } => {
            // Non-zero on any failed case, so continuous integration can
            // gate on it.
            if !harness::run_suite(&cases, &agent_binary)? {
                std::process::exit(1);
            }
        }
        cli::Command::Bench {
            manifest,
            cases,
            db,
            cache,
        } => {
            // Everything read from the environment is read first: the cache
            // directory, and each runtime's key, which may itself be a
            // ZORP_ variable. Then every inherited ZORP_ variable goes,
            // before the first request builds the shared HTTP agent.
            let cache = match cache {
                Some(cache) => cache,
                None => bench::dataset::default_cache_dir()?,
            };
            let plan = bench::Plan::prepare(bench::Options {
                manifest,
                cases,
                db: db.clone(),
                cache,
                datasets_server: bench::dataset::DATASETS_SERVER.to_string(),
            })?;
            let cleared = bench::clear_inherited_zorp_env();
            if !cleared.is_empty() {
                eprintln!("bench: cleared inherited {}", cleared.join(", "));
            }
            let outcomes = plan.run()?;
            println!("{}", outcomes.table);
            println!(
                "Results in {} (session {}).",
                db.display(),
                outcomes.session
            );
            // Exit zero whatever the rows say. Nothing gates on this, and an
            // exit code that tracked accuracy would invite something to.
        }
    }
    Ok(())
}
