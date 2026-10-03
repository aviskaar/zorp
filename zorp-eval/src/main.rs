use clap::Parser;
use std::process::ExitCode;
use zorp_eval::{bench, harness, runner, BoxErr};
mod cli;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", report(&*e));
            ExitCode::FAILURE
        }
    }
}

/// The error and every cause under it, laid out the way `anyhow` printed a
/// failed `main` before this crate dropped it, so what a person reads on
/// stderr did not change: the message, then a "Caused by:" list, numbered
/// only when there is more than one cause.
fn report(error: &(dyn std::error::Error + 'static)) -> String {
    let mut out = error.to_string();
    let Some(first) = error.source() else {
        return out;
    };
    out.push_str("\n\nCaused by:");
    let numbered = first.source().is_some();
    let mut cause = Some(first);
    let mut n = 0;
    while let Some(c) = cause {
        out.push('\n');
        let (lead, rest) = if numbered {
            (format!("{n: >5}: "), "       ")
        } else {
            ("    ".to_string(), "    ")
        };
        for (i, line) in c.to_string().split('\n').enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(if i == 0 { &lead } else { rest });
            out.push_str(line);
        }
        cause = c.source();
        n += 1;
    }
    out
}

fn run() -> Result<(), BoxErr> {
    let args = cli::Cli::parse();
    match args.command {
        cli::Command::Eval { suite } => {
            // No grader pipeline is wired up yet. Exiting successfully here
            // would look like a completed eval that never ran, so refuse.
            return Err(format!(
                "the eval subcommand is not implemented yet: no graders were run for suite '{suite}'. \
                 Use the compat subcommand to run a contract-based experiment."
            ).into());
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

#[cfg(test)]
mod tests {
    use super::report;
    use std::fmt;

    #[derive(Debug)]
    struct Layer(&'static str, Option<Box<Layer>>);

    impl fmt::Display for Layer {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    impl std::error::Error for Layer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.1.as_deref().map(|e| e as _)
        }
    }

    #[test]
    fn a_bare_message_prints_alone() {
        assert_eq!(report(&Layer("top", None)), "top");
    }

    #[test]
    fn one_cause_is_indented_and_not_numbered() {
        let e = Layer("top", Some(Box::new(Layer("io\nsecond", None))));
        assert_eq!(report(&e), "top\n\nCaused by:\n    io\n    second");
    }

    #[test]
    fn several_causes_are_numbered() {
        let e = Layer(
            "top",
            Some(Box::new(Layer(
                "mid",
                Some(Box::new(Layer("low\nmore", None))),
            ))),
        );
        assert_eq!(
            report(&e),
            "top\n\nCaused by:\n    0: mid\n    1: low\n       more"
        );
    }
}
