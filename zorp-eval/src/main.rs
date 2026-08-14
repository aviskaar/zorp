use clap::Parser;
use zorp_eval::runner;
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
            println!("Compatibility experiment complete. Results in {}", db.display());
        }
    }
    Ok(())
}
