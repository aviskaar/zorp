use std::io::{self, BufRead, Write};
use zorp::BoxErr;

fn stream_enabled() -> bool {
    std::env::var("ZORP_STREAM")
        .map(|v| v != "0")
        .unwrap_or(true)
}

/// The skills this process can see, in the directory it was started in.
///
/// The same scopes `zorp-agent` and `zorp-web` use, through the same crate,
/// so `zorp --skill foo` and the agent's `skill` tool see one set of
/// skills and not two.
fn skills() -> zorp_skill::SkillRegistry {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (registry, warnings) =
        zorp_skill::SkillRegistry::discover(&zorp_skill::scope_dirs_from_env(&cwd));
    // A skill that could not be read is named, never swallowed. Somebody
    // whose skill is missing needs to know why.
    for warning in warnings {
        eprintln!("zorp: {warning}");
    }
    registry
}

/// One `name: description` line per skill, for `--skills` and `/skills`.
fn list_skills() {
    let registry = skills();
    if registry.is_empty() {
        eprintln!(
            "zorp: no skills found. Put a directory holding a SKILL.md under \
             ~/.claude/skills, under .claude/skills here, or wherever \
             ZORP_SKILLS_DIR points."
        );
        return;
    }
    for skill in registry.iter() {
        println!("{}: {}", skill.name, skill.description);
    }
}

/// Look one up, or say why not.
fn load_skill(name: &str) -> Option<zorp_skill::Skill> {
    let registry = skills();
    match registry.get(name) {
        Some(skill) => Some(skill.clone()),
        None => {
            eprintln!("zorp: no skill called {name:?}.");
            if !registry.is_empty() {
                eprintln!("zorp: installed: {}", registry.names().join(", "));
            }
            None
        }
    }
}

/// The prompt a loaded skill produces.
///
/// The skill body goes in front of the user's own words and **never into
/// the system prompt**. It is the content of a file this binary did not
/// write, and the system slot is the one channel the harness speaks in.
/// `Skill::instructions` already ends with the sentence saying the text is
/// skill content and not a grant of permission, which is the same boundary
/// `zorp-agent` states when it hands a body back as a tool result.
fn with_skill(skill: &zorp_skill::Skill, prompt: &str) -> String {
    format!("{}\n\n---\n\n{prompt}", skill.instructions())
}

/// Answer one prompt with the given config, writing the model text to stdout.
fn answer(
    prompt: &str,
    base: &str,
    key: Option<&str>,
    model: &str,
    system: Option<&str>,
    stream: bool,
) -> Result<(), BoxErr> {
    let url = zorp::join_url(base, "chat/completions");
    let body = zorp::build_body(system, prompt, model);
    let auth = key.map(|k| format!("Bearer {k}"));
    let mut headers: Vec<(&str, &str)> = Vec::new();
    if let Some(a) = &auth {
        headers.push(("Authorization", a.as_str()));
    }
    if stream {
        // Lock stdout once for the whole stream instead of per token.
        let mut out = io::stdout().lock();
        zorp::zorp_stream(&url, &headers, body, |delta| {
            if let Some(t) = delta.get("content").and_then(|v| v.as_str()) {
                let _ = write!(out, "{t}");
                let _ = out.flush();
            }
        })?;
    } else {
        let resp = zorp::zorp_raw(&url, &headers, body)?;
        print!("{}", zorp::extract_content(&resp)?);
    }
    Ok(())
}

fn run_oneshot(prompt: &str) {
    let (base, key, model, system) = zorp::env_config();
    if let Err(e) = answer(
        prompt,
        &base,
        key.as_deref(),
        &model,
        system.as_deref(),
        stream_enabled(),
    ) {
        eprintln!("zorp: {e}");
        if key.is_none() {
            eprintln!(
                "zorp: no ZORP_API_KEY set (talking to {base}). \
                 Set ZORP_API_KEY, or point ZORP_BASE_URL at a local endpoint that doesn't need one."
            );
        }
        std::process::exit(1);
    }
    println!();
}

/// Stateless REPL: re-read env (incl. system prompt) each turn; no history retained.
fn run_repl() {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut line = String::new();
    // The skill loaded with `/skill <name>`, if any. Applied to every turn
    // until it is put down, because a skill is a way of working and not a
    // thing you re-ask for each message.
    let mut active: Option<zorp_skill::Skill> = None;
    loop {
        eprint!("zorp\u{203a} "); // "zorp› "
        let _ = io::stderr().flush();
        line.clear();
        match input.read_line(&mut line) {
            Ok(0) => break, // EOF / Ctrl-D
            Ok(_) => {}
            Err(_) => break,
        }
        let prompt = line.trim();
        if prompt.is_empty() {
            continue;
        }
        if prompt == "exit" || prompt == "quit" {
            break;
        }
        // Two commands, answered here rather than sent to the model. A
        // loaded skill stays loaded for the rest of the session, because
        // that is what somebody who typed it meant; `/skill off` puts it
        // down again.
        if prompt == "/skills" {
            list_skills();
            continue;
        }
        // The command word has to end here, or `/skillet` would strip to
        // `et` and go looking for a skill by that name instead of asking
        // the model what a skillet is.
        if let Some(rest) = prompt
            .strip_prefix("/skill")
            .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        {
            let rest = rest.trim();
            match rest {
                "" => match &active {
                    Some(skill) => eprintln!("zorp: {} is loaded.", skill.name),
                    None => eprintln!("zorp: no skill is loaded. /skills lists them."),
                },
                "off" | "none" => {
                    active = None;
                    eprintln!("zorp: no skill is loaded.");
                }
                name => {
                    if let Some(skill) = load_skill(name) {
                        eprintln!("zorp: loaded {} from {}.", skill.name, skill.path.display());
                        active = Some(skill);
                    }
                }
            }
            continue;
        }
        let (base, key, model, system) = zorp::env_config();
        let sent = match &active {
            Some(skill) => with_skill(skill, prompt),
            None => prompt.to_string(),
        };
        if let Err(e) = answer(
            &sent,
            &base,
            key.as_deref(),
            &model,
            system.as_deref(),
            stream_enabled(),
        ) {
            eprintln!("zorp: {e}"); // per-turn failure never kills the loop
        }
        println!();
    }
}

fn run_init() -> Result<(), BoxErr> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stderr = io::stderr();
    let mut prompts = stderr.lock();
    let pairs = zorp::init_exports(&mut input, &mut prompts)?;
    for (k, v) in pairs {
        // Single-quote the value so $, backticks, and double quotes are inert
        // under `eval "$(zorp --init)"`; escape any embedded single quote.
        let escaped = v.replace('\'', "'\\''");
        println!("export {k}='{escaped}'");
    }
    Ok(())
}

const USAGE: &str = "\
zorp: one prompt, one answer, against any OpenAI-compatible endpoint.

Usage:
  zorp <prompt>...          answer a prompt and exit
  zorp --skill <name> <prompt>...  the same, with a skill's instructions in front
  zorp                      read prompts from stdin until EOF
  zorp --skills             list the skills this directory can see
  zorp --init               print `export` lines for your shell, interactively
  zorp --version            print the version
  zorp --help               print this

In the stdin loop, /skills lists them and /skill <name> loads one for the
rest of the session. /skill off puts it down.

Configuration, all environment variables:
  ZORP_BASE_URL            endpoint base (default https://api.openai.com/v1)
  ZORP_API_KEY             bearer token; leave unset for a local endpoint
  ZORP_MODEL               model name
  ZORP_SYSTEM              system prompt
  ZORP_STREAM              set to 0 to buffer the reply instead of streaming
  ZORP_HTTP_TIMEOUT_SECS   idle read timeout; raise it for a slow cold model
  ZORP_SKILLS_DIR          an extra skills directory, searched last

Skills are Claude Code's format: a directory holding a SKILL.md, found under
~/.claude/skills, under .claude/skills here, and in ZORP_SKILLS_DIR. A skill
body is text that goes in front of your prompt. It grants nothing, because
this binary has no tools to grant.

Only a leading flag is read as a flag. Anywhere else it is part of the
prompt, so `zorp what does --version print` still asks the model.
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Answered here rather than forwarded to the model. These are the first
    // things anyone types at an unfamiliar binary, and sending them costs a
    // completion to be told, at best, something the model guessed.
    match args.first().map(|s| s.as_str()) {
        Some("--version" | "-V") => {
            println!("zorp {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some("--help" | "-h") => {
            print!("{USAGE}");
            return;
        }
        Some("--skills") => {
            list_skills();
            return;
        }
        _ => {}
    }

    // `--skill <name>` puts one skill's instructions in front of the
    // prompt. Only as a leading flag, the same rule every other flag here
    // follows, so `zorp what does --skill do` still asks the model.
    if args.first().map(|s| s.as_str()) == Some("--skill") {
        let Some(name) = args.get(1) else {
            eprintln!("zorp: --skill needs a name. --skills lists them.");
            std::process::exit(2);
        };
        let Some(skill) = load_skill(name) else {
            std::process::exit(1);
        };
        let prompt = args[2..].join(" ");
        if prompt.trim().is_empty() {
            eprintln!("zorp: --skill {name} needs a prompt after it.");
            std::process::exit(2);
        }
        run_oneshot(&with_skill(&skill, &prompt));
        return;
    }
    if args.first().map(|s| s.as_str()) == Some("--init") {
        if let Err(e) = run_init() {
            eprintln!("zorp: {e}");
            std::process::exit(1);
        }
        return;
    }
    if args.is_empty() {
        run_repl();
    } else {
        run_oneshot(&args.join(" "));
    }
}
