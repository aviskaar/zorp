#[derive(Debug, PartialEq)]
pub enum ReasoningCommand {
    Show,
    Set(String),
}

/// A parsed line of chat input: a slash-command or plain text to send.
#[derive(Debug, PartialEq)]
pub enum ChatCommand {
    Help,
    Model,
    Context,
    /// Summarize the older conversation now, with an optional steer.
    ///
    /// `/compact` and `/compact <what to keep>`. The focus is a person's
    /// own words and is untrusted like any other: it is fenced as a
    /// preference when it reaches the model and can change no rule about
    /// what the summary must contain.
    Compact(Option<String>),
    Diff,
    Status,
    Undo,
    Approve,
    Deny,
    Clear,
    Exit,
    Tools,
    Reasoning(ReasoningCommand),
    /// The same report `zorp-agent doctor` prints.
    Doctor,
    /// Fork the conversation you are in at one of its answers.
    ///
    /// The browser's button is per answer because the page has the answers
    /// on screen to click. A terminal does not, so the number is optional
    /// and defaults to the most recent.
    Branch(Option<usize>),
    /// Search your own conversations. A person reading their own history;
    /// the model gets nothing from this.
    Recall(String),
    /// Review this conversation's last answer with the panel.
    ///
    /// With a path, reviews that file instead. A person types this, which
    /// is the same bound the browser's button has.
    Panel(Option<String>),
    /// Say which project this conversation is in, or file it in one.
    ///
    /// `/project` reports, `/project <name or id>` files, and
    /// `/project none` takes it out.
    Project(Option<String>),
    Capsules,
    /// List the skills this session can see.
    ///
    /// A reader, not a loader. Loading a skill is the `skill` tool, which
    /// the model calls when the task matches a description, gated exactly
    /// as every other tool call is. A person typing this wants to know what
    /// the model has to choose from.
    Skills,
    LoadCapsule(String),
    UnloadCapsule(String),
    InvokeCapsule {
        name: String,
        prompt: Option<String>,
    },
    CreateCapsule {
        name: String,
        description: String,
    },
    Say(String),
    Unknown(String),
}

/// Every slash command, for Tab completion.
///
/// Here rather than in `main.rs` so it sits beside the `match` in
/// `parse_command` that recognizes them, which is the thing it has to stay
/// in step with. A command completable but unrecognized, or the reverse,
/// is the failure this placement is for.
pub const COMMANDS: &[&str] = &[
    "help",
    "model",
    "context",
    "diff",
    "status",
    "undo",
    "approve",
    "deny",
    "clear",
    "exit",
    "tools",
    "reasoning",
    "capsules",
    "load",
    "unload",
    "capsule-create",
    "branch",
];

/// Parse one line of REPL input. A leading `/` marks a command (case-insensitive,
/// first word only); anything else, including an empty line, is `Say`.
/// `capsule_names` is the set of currently discoverable capsule names, checked
/// only after every reserved built-in name has been ruled out, so a capsule can
/// never shadow a built-in.
pub fn parse_command(line: &str, capsule_names: &[String]) -> ChatCommand {
    let trimmed = line.trim();
    let Some(rest) = trimmed.strip_prefix('/') else {
        return ChatCommand::Say(trimmed.to_string());
    };
    let mut split = rest.splitn(2, char::is_whitespace);
    let name = split.next().unwrap_or("");
    let remainder = split.next().unwrap_or("").trim();

    match name.to_ascii_lowercase().as_str() {
        "reasoning" => {
            let mut args = remainder.split_whitespace();
            match (args.next(), args.next()) {
                (None, None) => ChatCommand::Reasoning(ReasoningCommand::Show),
                (Some(value), None) => {
                    ChatCommand::Reasoning(ReasoningCommand::Set(value.to_string()))
                }
                _ => ChatCommand::Unknown("reasoning".to_string()),
            }
        }
        "help" | "h" | "?" => ChatCommand::Help,
        "model" => ChatCommand::Model,
        "context" => ChatCommand::Context,
        "compact" => ChatCommand::Compact((!remainder.is_empty()).then(|| remainder.to_string())),
        "diff" => ChatCommand::Diff,
        "status" => ChatCommand::Status,
        "undo" => ChatCommand::Undo,
        "approve" => ChatCommand::Approve,
        "deny" => ChatCommand::Deny,
        "clear" => ChatCommand::Clear,
        "exit" | "quit" | "q" => ChatCommand::Exit,
        "tools" | "commands" => ChatCommand::Tools,
        "doctor" => ChatCommand::Doctor,
        "branch" => match remainder {
            "" => ChatCommand::Branch(None),
            n => match n.parse::<usize>() {
                Ok(n) if n > 0 => ChatCommand::Branch(Some(n)),
                _ => ChatCommand::Unknown("branch".to_string()),
            },
        },
        "recall" => ChatCommand::Recall(remainder.to_string()),
        "panel" => ChatCommand::Panel((!remainder.is_empty()).then(|| remainder.to_string())),
        "project" => ChatCommand::Project((!remainder.is_empty()).then(|| remainder.to_string())),
        "capsules" => ChatCommand::Capsules,
        "skills" => ChatCommand::Skills,
        "load" => ChatCommand::LoadCapsule(remainder.to_string()),
        "unload" => ChatCommand::UnloadCapsule(remainder.to_string()),
        "capsule-create" => {
            let mut parts = remainder.splitn(2, char::is_whitespace);
            let cap_name = parts.next().unwrap_or("").to_string();
            let description = parts.next().unwrap_or("").trim().to_string();
            ChatCommand::CreateCapsule {
                name: cap_name,
                description,
            }
        }
        other => match capsule_names.iter().find(|n| n.eq_ignore_ascii_case(other)) {
            Some(matched) => ChatCommand::InvokeCapsule {
                name: matched.clone(),
                prompt: (!remainder.is_empty()).then(|| remainder.to_string()),
            },
            None => ChatCommand::Unknown(other.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_say_trimmed() {
        assert_eq!(
            parse_command("  fix the bug  ", &[]),
            ChatCommand::Say("fix the bug".to_string())
        );
    }

    #[test]
    fn known_commands_parse_case_insensitively() {
        assert_eq!(parse_command("/HELP", &[]), ChatCommand::Help);
        assert_eq!(parse_command("/Exit", &[]), ChatCommand::Exit);
        assert_eq!(parse_command("/diff", &[]), ChatCommand::Diff);
        assert_eq!(parse_command("/undo", &[]), ChatCommand::Undo);
        assert_eq!(parse_command("/approve", &[]), ChatCommand::Approve);
        assert_eq!(parse_command("/tools", &[]), ChatCommand::Tools);
        assert_eq!(parse_command("/deny", &[]), ChatCommand::Deny);
    }

    /// A capsule cannot shadow it, the way no capsule can shadow any
    /// built-in.
    #[test]
    fn skills_is_a_builtin_a_capsule_cannot_take() {
        assert_eq!(parse_command("/skills", &[]), ChatCommand::Skills);
        assert_eq!(parse_command("/SKILLS", &[]), ChatCommand::Skills);
        assert_eq!(
            parse_command("/skills", &["skills".to_string()]),
            ChatCommand::Skills
        );
    }

    /// The number is optional because a terminal cannot see the answers to
    /// click one. Zero and anything that is not a number are refusals
    /// rather than a silent default, because both are somebody meaning
    /// something the command cannot do.
    #[test]
    fn branch_takes_an_optional_answer_number() {
        assert_eq!(parse_command("/branch", &[]), ChatCommand::Branch(None));
        assert_eq!(
            parse_command("/branch 2", &[]),
            ChatCommand::Branch(Some(2))
        );
        assert_eq!(
            parse_command("/branch 0", &[]),
            ChatCommand::Unknown("branch".to_string())
        );
        assert_eq!(
            parse_command("/branch latest", &[]),
            ChatCommand::Unknown("branch".to_string())
        );
    }

    /// Every name Tab offers is a name `parse_command` recognizes.
    ///
    /// The failure this catches is a command that completes and then says
    /// "unknown command", or one that exists and cannot be completed. Both
    /// are the two lists drifting.
    #[test]
    fn every_completable_command_is_a_real_one() {
        for name in COMMANDS {
            let parsed = parse_command(&format!("/{name}"), &[]);
            assert!(
                !matches!(parsed, ChatCommand::Unknown(_)),
                "/{name} completes and is not a command"
            );
        }
    }

    #[test]
    fn command_ignores_trailing_arguments() {
        assert_eq!(parse_command("/model gpt-4o", &[]), ChatCommand::Model);
    }

    #[test]
    fn unknown_slash_command_is_reported() {
        assert_eq!(
            parse_command("/frobnicate", &[]),
            ChatCommand::Unknown("frobnicate".to_string())
        );
    }

    #[test]
    fn aliases_map_to_canonical_commands() {
        assert_eq!(parse_command("/q", &[]), ChatCommand::Exit);
        assert_eq!(parse_command("/?", &[]), ChatCommand::Help);
        assert_eq!(parse_command("/commands", &[]), ChatCommand::Tools);
    }

    #[test]
    fn reasoning_without_argument_parses_as_show() {
        assert_eq!(
            parse_command("/reasoning", &[]),
            ChatCommand::Reasoning(ReasoningCommand::Show)
        );
    }

    #[test]
    fn reasoning_with_value_parses_as_set() {
        assert_eq!(
            parse_command("/reasoning high", &[]),
            ChatCommand::Reasoning(ReasoningCommand::Set("high".to_string()))
        );
    }

    #[test]
    fn reasoning_rejects_extra_arguments() {
        assert_eq!(
            parse_command("/reasoning high extra", &[]),
            ChatCommand::Unknown("reasoning".to_string())
        );
    }

    #[test]
    fn capsules_command_parses() {
        assert_eq!(parse_command("/capsules", &[]), ChatCommand::Capsules);
    }

    #[test]
    fn load_capsule_parses_name() {
        assert_eq!(
            parse_command("/load foo", &[]),
            ChatCommand::LoadCapsule("foo".to_string())
        );
    }

    #[test]
    fn load_without_name_is_empty_string() {
        assert_eq!(
            parse_command("/load", &[]),
            ChatCommand::LoadCapsule(String::new())
        );
    }

    #[test]
    fn unload_capsule_parses_name() {
        assert_eq!(
            parse_command("/unload foo", &[]),
            ChatCommand::UnloadCapsule("foo".to_string())
        );
    }

    #[test]
    fn capsule_name_invokes_with_no_prompt() {
        let names = vec!["foo".to_string()];
        assert_eq!(
            parse_command("/foo", &names),
            ChatCommand::InvokeCapsule {
                name: "foo".to_string(),
                prompt: None
            }
        );
    }

    #[test]
    fn capsule_name_invokes_with_prompt_text() {
        let names = vec!["foo".to_string()];
        assert_eq!(
            parse_command("/foo do the thing", &names),
            ChatCommand::InvokeCapsule {
                name: "foo".to_string(),
                prompt: Some("do the thing".to_string()),
            }
        );
    }

    #[test]
    fn capsule_name_matches_case_insensitively_and_returns_canonical_case() {
        let names = vec!["Foo".to_string()];
        assert_eq!(
            parse_command("/foo", &names),
            ChatCommand::InvokeCapsule {
                name: "Foo".to_string(),
                prompt: None
            }
        );
    }

    #[test]
    fn builtin_name_always_wins_over_a_same_named_capsule() {
        let names = vec!["model".to_string()];
        assert_eq!(parse_command("/model", &names), ChatCommand::Model);
    }

    #[test]
    fn unknown_name_with_no_capsule_match_is_still_unknown() {
        assert_eq!(
            parse_command("/frobnicate", &["foo".to_string()]),
            ChatCommand::Unknown("frobnicate".to_string())
        );
    }

    #[test]
    fn capsule_create_with_name_and_description() {
        assert_eq!(
            parse_command("/capsule-create foo does the thing", &[]),
            ChatCommand::CreateCapsule {
                name: "foo".to_string(),
                description: "does the thing".to_string()
            }
        );
    }

    #[test]
    fn capsule_create_with_name_but_no_description() {
        assert_eq!(
            parse_command("/capsule-create foo", &[]),
            ChatCommand::CreateCapsule {
                name: "foo".to_string(),
                description: String::new()
            }
        );
    }

    #[test]
    fn capsule_create_with_no_args() {
        assert_eq!(
            parse_command("/capsule-create", &[]),
            ChatCommand::CreateCapsule {
                name: String::new(),
                description: String::new()
            }
        );
    }

    #[test]
    fn capsule_create_trims_extra_whitespace_around_description() {
        assert_eq!(
            parse_command("/capsule-create foo   does the thing  ", &[]),
            ChatCommand::CreateCapsule {
                name: "foo".to_string(),
                description: "does the thing".to_string()
            }
        );
    }
}
