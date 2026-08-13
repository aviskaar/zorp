use zorp::BoxErr;
use std::io::{self, BufRead, Write};

fn stream_enabled() -> bool {
    std::env::var("ZORP_STREAM")
        .map(|v| v != "0")
        .unwrap_or(true)
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
        zorp::zorp_stream(&url, &headers, body, |delta| {
            if let Some(t) = delta.get("content").and_then(|v| v.as_str()) {
                print!("{t}");
                let _ = io::stdout().flush();
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
        let (base, key, model, system) = zorp::env_config();
        if let Err(e) = answer(
            prompt,
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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
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
