//! Which skills a particular model is offered.
//!
//! Two of the skills zorp ships write pages. `landing-page` writes one plain
//! HTML file with no framework and no build step, and `react-components`
//! writes the same kind of page as a component tree in JSX. The second is the
//! wrong thing to hand a small model, and it is wrong twice over: its body is
//! a large block of instructions that a small context window cannot afford,
//! and the output is still broken after the window has paid for it.
//!
//! **The choice is made at the index.** The `skill` tool's description holds
//! one line per skill and is in the request on every turn, so a skill left
//! out there costs nothing at all: not the index line each turn, not the body
//! the model might have loaded, and not the temptation of an entry it would
//! have to be trusted to decline. Discovery is untouched. `zorp-skill` still
//! finds everything on disk, and every listing a person reads still shows it.
//!
//! **A rule in code, never a model call.** `web/src/onboarding.ts` settled
//! the same question for picking a model: a classifier deciding what suits a
//! request would be a model call nobody asked for, spending their money to
//! decide how to spend their money. So this reads two facts that already
//! exist and applies thresholds a person can read.
//!
//! 1. **The size written in the model id.** Nothing reports a parameter
//!    count. Providers do not send one and `ModelDetail` has no such field,
//!    and zorp should not invent one. But local ids usually carry it:
//!    `qwen2.5:7b`, `gemma2:2b`, `llama-3.3-70b-instruct`. This is a
//!    heuristic, and `gpt-4o`, `claude-opus-5` and `mistral-nemo` say
//!    nothing, which is the answer it returns for them.
//! 2. **The context window, when anybody has stated one.** Either the person
//!    through `ZORP_CONTEXT_TOKENS`, or the provider's listing through
//!    `context_length`. This is real data rather than a parse, and for the
//!    "too much prompting" half of the problem it is the better signal: a
//!    small window cannot afford a large skill body however capable the
//!    model behind it is.
//!
//! **Unknown means the full set.** The opposite default looks safer and is
//! not. The ids that encode no size are overwhelmingly the large hosted
//! models, because it is Ollama style tags that carry `:7b`. Treating unknown
//! as small would take the component skill away from nearly every frontier
//! model, which is a far more common and far more annoying failure than
//! occasionally offering it to a model that cannot use it.
//!
//! **A person can override it either way**, with `ZORP_SKILL_TIER`, because
//! the heuristic will be wrong about some models and the person is the one
//! who knows which.
//!
//! None of this touches the trust boundary. A skill offered is still
//! untrusted text that grants no tool and bypasses no denylist entry, and a
//! skill withheld is simply not in the list.

use serde::Serialize;

/// The skills that assume a model can hold a component tree in its head.
///
/// A named list and deliberately not a frontmatter field. A field would let
/// any `SKILL.md` on the machine put itself into a tier, which is a
/// capability system, and the issue this came from rules out anything beyond
/// this one split. A third tier has to be argued for on its own.
pub const COMPONENT_SKILLS: &[&str] = &["react-components"];

/// Below this many billion parameters, as written in the id, the component
/// skill is not offered.
///
/// The common local sizes cluster at 1 to 4, 7 to 9, 12 to 14, 27 to 34 and
/// 70 and up. The line sits under the 14B tier (`qwen2.5-coder:14b`,
/// `phi4:14b`), the smallest where a multi-file component tree is a
/// reasonable thing to ask for. Everything from 12B down gets the plain HTML
/// skill, which it can actually finish.
pub const MIN_COMPONENT_PARAMS_B: f64 = 14.0;

/// Below this many tokens of context window, the component skill is not
/// offered.
///
/// A turn that uses it loads two bodies, this one and `landing-page` under
/// it, which together are around five thousand tokens before the system
/// prompt, the tool schemas and the conversation. In an 8K window that is
/// most of the request. At 16K it is under a third, which leaves room for the
/// page being written.
pub const MIN_COMPONENT_CONTEXT_TOKENS: u64 = 16_384;

/// The environment variable that overrides the rule.
pub const TIER_ENV: &str = "ZORP_SKILL_TIER";

/// Which set of skills a model is offered.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillTier {
    /// Everything installed.
    Full,
    /// Everything except `COMPONENT_SKILLS`.
    Plain,
}

impl SkillTier {
    pub fn as_str(self) -> &'static str {
        match self {
            SkillTier::Full => "full",
            SkillTier::Plain => "plain",
        }
    }
}

/// What a person asked for, through `ZORP_SKILL_TIER`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TierChoice {
    /// Unset, blank or `auto`: the rule decides.
    Auto,
    /// `full` (or `all`): offer everything whatever the model.
    Full,
    /// `plain` (or `html`): withhold the component skill whatever the model.
    Plain,
    /// Something else. The rule decides, and the reason says the value was
    /// not understood rather than quietly ignoring it.
    Unrecognized(String),
}

impl TierChoice {
    pub fn parse(raw: Option<&str>) -> TierChoice {
        let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
            return TierChoice::Auto;
        };
        match raw.to_ascii_lowercase().as_str() {
            "auto" => TierChoice::Auto,
            "full" | "all" => TierChoice::Full,
            "plain" | "html" => TierChoice::Plain,
            _ => TierChoice::Unrecognized(raw.to_string()),
        }
    }

    /// Read live, like `ZORP_CONTEXT_TOKENS`, so changing it needs no
    /// restart of a server that is already running.
    pub fn from_env() -> TierChoice {
        TierChoice::parse(std::env::var(TIER_ENV).ok().as_deref())
    }

    /// The word reported beside the offer.
    pub fn as_str(&self) -> &str {
        match self {
            TierChoice::Auto | TierChoice::Unrecognized(_) => "auto",
            TierChoice::Full => "full",
            TierChoice::Plain => "plain",
        }
    }
}

/// What is known about the model a turn will talk to.
#[derive(Clone, Copy, Debug, Default)]
pub struct ModelFacts<'a> {
    /// The id sent to the provider, when there is one.
    pub id: Option<&'a str>,
    /// The window a person configured, `ZORP_CONTEXT_TOKENS`.
    pub configured_window: Option<u64>,
    /// The window the provider's model listing stated, `context_length`.
    pub listed_window: Option<u64>,
}

/// The decision, and the sentence that explains it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SkillOffer {
    pub tier: SkillTier,
    /// `auto`, `full` or `plain`: what `ZORP_SKILL_TIER` asked for.
    pub setting: String,
    /// One sentence a person can read to find out why a skill is missing.
    /// Printed beside the choice for the same reason onboarding prints its
    /// rule: a skill that is absent with no explanation looks like a bug.
    pub reason: String,
}

impl SkillOffer {
    /// Whether this offer includes `name`.
    pub fn offers(&self, name: &str) -> bool {
        self.tier == SkillTier::Full || !COMPONENT_SKILLS.contains(&name)
    }

    /// The installed skills this offer leaves out, by name.
    pub fn withheld<'a>(&self, installed: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        installed
            .into_iter()
            .filter(|name| !self.offers(name))
            .map(str::to_string)
            .collect()
    }
}

/// Decide which skills a model is offered.
pub fn skill_offer(facts: ModelFacts<'_>, choice: &TierChoice) -> SkillOffer {
    let setting = choice.as_str().to_string();
    match choice {
        TierChoice::Full => {
            return SkillOffer {
                tier: SkillTier::Full,
                setting,
                reason: format!("{TIER_ENV}=full offers every installed skill to every model."),
            }
        }
        TierChoice::Plain => {
            return SkillOffer {
                tier: SkillTier::Plain,
                setting,
                reason: format!(
                    "{TIER_ENV}=plain withholds the component skill from every model, \
                     so pages are written as plain HTML."
                ),
            }
        }
        TierChoice::Auto | TierChoice::Unrecognized(_) => {}
    }

    let ignored = match choice {
        TierChoice::Unrecognized(raw) => {
            format!(" {TIER_ENV}={raw:?} is not auto, full or plain, so it was ignored.")
        }
        _ => String::new(),
    };
    let override_hint = format!(" Set {TIER_ENV}=full or {TIER_ENV}=plain to decide it yourself.");
    let model = facts
        .id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or("this model");
    let size = facts.id.and_then(parse_size_billions);
    let window = smallest_window(facts);

    if let Some(b) = size.filter(|b| *b < MIN_COMPONENT_PARAMS_B) {
        return SkillOffer {
            tier: SkillTier::Plain,
            setting,
            reason: format!(
                "{model} reads as {} parameters from its id, under the {}B the \
                 component skill is offered at, so pages are written as plain HTML.{ignored}{override_hint}",
                billions(b),
                MIN_COMPONENT_PARAMS_B
            ),
        };
    }
    if let Some((tokens, source)) = window.filter(|(t, _)| *t < MIN_COMPONENT_CONTEXT_TOKENS) {
        return SkillOffer {
            tier: SkillTier::Plain,
            setting,
            reason: format!(
                "{model} has a {tokens} token context window ({source}), under the \
                 {MIN_COMPONENT_CONTEXT_TOKENS} the component skill is offered at, so pages \
                 are written as plain HTML.{ignored}{override_hint}"
            ),
        };
    }

    let size_part = match size {
        Some(b) => format!("{model} reads as {} parameters from its id", billions(b)),
        None => {
            format!("{model} states no size in its id, and an unknown size is offered everything")
        }
    };
    let window_part = match window {
        Some((tokens, source)) => format!(" and has a {tokens} token window ({source})"),
        None => String::new(),
    };
    SkillOffer {
        tier: SkillTier::Full,
        setting,
        reason: format!("{size_part}{window_part}, so every installed skill is offered.{ignored}"),
    }
}

/// Both windows when both are known, the smaller one, because the smaller is
/// the one a request actually has to fit. A person who set Ollama's
/// `num_ctx` below what the model supports has the window they set.
fn smallest_window(facts: ModelFacts<'_>) -> Option<(u64, &'static str)> {
    let configured = facts
        .configured_window
        .filter(|t| *t > 0)
        .map(|t| (t, "ZORP_CONTEXT_TOKENS"));
    let listed = facts
        .listed_window
        .filter(|t| *t > 0)
        .map(|t| (t, "from the model listing"));
    match (configured, listed) {
        (Some(c), Some(l)) => Some(if l.0 < c.0 { l } else { c }),
        (c, l) => c.or(l),
    }
}

fn billions(b: f64) -> String {
    if b >= 1.0 && b.fract() == 0.0 {
        format!("{b:.0}B")
    } else if b >= 1.0 {
        format!("{b}B")
    } else {
        format!("{:.0}M", b * 1000.0)
    }
}

/// The parameter count a model id spells out, in billions, when it spells
/// one out.
///
/// A number directly followed by `b` (or `m`, for millions), standing on its
/// own between separators: `qwen2.5:7b`, `gemma2:2b`, `phi3:3.8b`,
/// `llama-3.3-70b-instruct`, `Meta-Llama-3.1-8B-Instruct`, `smollm2:135m`.
/// Mixture of experts ids written as `8x7b` count as the product, and
/// Gemma's `e4b` (effective parameters) counts as its number. When an id
/// names more than one size the largest wins, since the rule should err
/// toward the full set.
///
/// A number has to start at a boundary so the `2.5` in `qwen2.5` or the `3`
/// in `a3b` is never read as a size, and it is consumed whole so the `8` in
/// `3.8b` is never read on its own.
pub fn parse_size_billions(id: &str) -> Option<f64> {
    let chars: Vec<char> = id.chars().collect();
    let at_boundary = |i: usize| i == 0 || !chars[i - 1].is_ascii_alphanumeric();
    let ends_token = |i: usize| i >= chars.len() || !chars[i].is_ascii_alphanumeric();
    let mut best: Option<f64> = None;
    let mut i = 0;
    while i < chars.len() {
        // `e4b`: Gemma's effective size. The `e` is a prefix on the number,
        // not a letter in the middle of a word.
        let start = if (chars[i] == 'e' || chars[i] == 'E')
            && at_boundary(i)
            && chars.get(i + 1).is_some_and(char::is_ascii_digit)
        {
            i + 1
        } else {
            i
        };
        if !chars[start].is_ascii_digit() {
            i += 1;
            continue;
        }
        let boundary = if start == i { at_boundary(i) } else { true };
        let (first, after) = read_number(&chars, start);
        i = after.max(i + 1);
        if !boundary {
            continue;
        }
        let Some(first) = first else { continue };
        let mut value = first;
        let mut at = after;
        // `8x7b`
        if chars.get(at).is_some_and(|c| *c == 'x' || *c == 'X') {
            let (second, after_second) = read_number(&chars, at + 1);
            if let Some(second) = second {
                value *= second;
                at = after_second;
                i = at;
            }
        }
        let scale = match chars.get(at) {
            Some('b') | Some('B') => 1.0,
            Some('m') | Some('M') => 0.001,
            _ => continue,
        };
        if !ends_token(at + 1) {
            continue;
        }
        let size = value * scale;
        if size > 0.0 {
            best = Some(best.map_or(size, |b: f64| b.max(size)));
        }
    }
    best
}

/// `[0-9]+(\.[0-9]+)?` starting at `start`, and where it stopped.
fn read_number(chars: &[char], start: usize) -> (Option<f64>, usize) {
    let mut end = start;
    while end < chars.len() && chars[end].is_ascii_digit() {
        end += 1;
    }
    if end == start {
        return (None, start);
    }
    if end + 1 < chars.len() && chars[end] == '.' && chars[end + 1].is_ascii_digit() {
        end += 1;
        while end < chars.len() && chars[end].is_ascii_digit() {
            end += 1;
        }
    }
    let text: String = chars[start..end].iter().collect();
    (text.parse().ok(), end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(id: &str) -> Option<f64> {
        parse_size_billions(id)
    }

    fn offer(id: &str) -> SkillOffer {
        skill_offer(
            ModelFacts {
                id: Some(id),
                ..ModelFacts::default()
            },
            &TierChoice::Auto,
        )
    }

    #[test]
    fn sizes_are_read_from_ids_that_spell_them_out() {
        for (id, want) in [
            ("qwen2.5:7b", 7.0),
            ("gemma2:2b", 2.0),
            ("llama-3.3-70b-instruct", 70.0),
            ("meta-llama/Llama-3.1-8B-Instruct", 8.0),
            ("phi3:3.8b", 3.8),
            ("qwen3:0.6b", 0.6),
            ("deepseek-r1:1.5b", 1.5),
            ("qwen2.5-coder:14b-instruct-q4_K_M", 14.0),
            ("llama3.1:405b", 405.0),
            ("gpt-oss:20b", 20.0),
            ("openai/gpt-oss-120b", 120.0),
            ("qwen/qwen-2.5-72b-instruct", 72.0),
            ("mixtral:8x7b", 56.0),
            ("qwen3-30b-a3b", 30.0),
            ("gemma3n:e4b", 4.0),
            ("smollm2:135m", 0.135),
        ] {
            let got = size(id).unwrap_or_else(|| panic!("{id} should parse"));
            assert!((got - want).abs() < 1e-9, "{id}: got {got}, want {want}");
        }
    }

    /// The ids that encode nothing. Every one of these must come back as
    /// unknown rather than as a number found somewhere in the string, since
    /// a version, a date or a quantization tag read as a size would put a
    /// frontier model on the plain tier.
    #[test]
    fn ids_that_encode_no_size_read_as_unknown() {
        for id in [
            "gpt-4o",
            "gpt-4o-mini",
            "claude-opus-5",
            "claude-3-5-sonnet-20241022",
            "mistral-nemo",
            "mistral-large-2411",
            "o3-mini",
            "qwen2.5",
            "llama3.1:latest",
            "openrouter/auto",
            "text-embedding-3-small",
            "model-q8_0",
            "gemini-2.5-pro",
            "",
        ] {
            assert_eq!(size(id), None, "{id} should not parse as a size");
        }
    }

    #[test]
    fn a_small_id_is_offered_the_plain_set() {
        for id in [
            "qwen2.5:7b",
            "gemma2:2b",
            "llama3.1:8b",
            "gemma3:12b",
            "phi3:3.8b",
        ] {
            let offer = offer(id);
            assert_eq!(offer.tier, SkillTier::Plain, "{id}: {}", offer.reason);
            assert!(offer.reason.contains(id), "{}", offer.reason);
            assert!(
                offer.reason.contains(TIER_ENV),
                "no way out named: {}",
                offer.reason
            );
        }
    }

    #[test]
    fn a_large_id_is_offered_everything() {
        for id in ["qwen2.5-coder:14b", "gemma3:27b", "llama-3.3-70b-instruct"] {
            assert_eq!(offer(id).tier, SkillTier::Full, "{id}");
        }
    }

    /// The default the issue argued for. An id with no size in it is almost
    /// always a large hosted model, and downgrading those would be the common
    /// failure rather than the rare one.
    #[test]
    fn an_unknown_size_is_offered_everything() {
        for id in ["gpt-4o", "claude-opus-5", "mistral-nemo"] {
            let offer = offer(id);
            assert_eq!(offer.tier, SkillTier::Full, "{id}");
            assert!(offer.reason.contains("no size"), "{}", offer.reason);
        }
        let nameless = skill_offer(ModelFacts::default(), &TierChoice::Auto);
        assert_eq!(nameless.tier, SkillTier::Full);
    }

    #[test]
    fn a_small_window_withholds_the_component_skill_whatever_the_size() {
        for (configured, listed) in [
            (Some(8_192), None),
            (None, Some(8_192)),
            (Some(4_096), Some(200_000)),
        ] {
            let offer = skill_offer(
                ModelFacts {
                    id: Some("claude-opus-5"),
                    configured_window: configured,
                    listed_window: listed,
                },
                &TierChoice::Auto,
            );
            assert_eq!(offer.tier, SkillTier::Plain, "{configured:?} {listed:?}");
            assert!(offer.reason.contains("context window"), "{}", offer.reason);
        }
    }

    /// The window rule reports which window it used, and it uses the smaller
    /// one, since that is the one the request has to fit.
    #[test]
    fn the_window_rule_names_where_the_window_came_from() {
        let listed = skill_offer(
            ModelFacts {
                id: Some("m"),
                configured_window: Some(200_000),
                listed_window: Some(8_000),
            },
            &TierChoice::Auto,
        );
        assert!(listed.reason.contains("model listing"), "{}", listed.reason);
        let configured = skill_offer(
            ModelFacts {
                id: Some("m"),
                configured_window: Some(8_000),
                listed_window: Some(200_000),
            },
            &TierChoice::Auto,
        );
        assert!(
            configured.reason.contains("ZORP_CONTEXT_TOKENS"),
            "{}",
            configured.reason
        );
    }

    #[test]
    fn a_window_at_the_threshold_is_enough() {
        let offer = skill_offer(
            ModelFacts {
                id: Some("gpt-4o"),
                configured_window: Some(MIN_COMPONENT_CONTEXT_TOKENS),
                listed_window: None,
            },
            &TierChoice::Auto,
        );
        assert_eq!(offer.tier, SkillTier::Full, "{}", offer.reason);
    }

    #[test]
    fn the_override_wins_in_both_directions() {
        let small = ModelFacts {
            id: Some("qwen2.5:7b"),
            configured_window: Some(4_096),
            ..ModelFacts::default()
        };
        let full = skill_offer(small, &TierChoice::Full);
        assert_eq!(full.tier, SkillTier::Full);
        assert_eq!(full.setting, "full");
        assert!(full.reason.contains("ZORP_SKILL_TIER=full"));

        let large = ModelFacts {
            id: Some("claude-opus-5"),
            ..ModelFacts::default()
        };
        let plain = skill_offer(large, &TierChoice::Plain);
        assert_eq!(plain.tier, SkillTier::Plain);
        assert_eq!(plain.setting, "plain");
        assert!(plain.reason.contains("ZORP_SKILL_TIER=plain"));
    }

    #[test]
    fn the_override_is_parsed_forgivingly_and_a_typo_is_said_out_loud() {
        assert_eq!(TierChoice::parse(None), TierChoice::Auto);
        assert_eq!(TierChoice::parse(Some("  ")), TierChoice::Auto);
        assert_eq!(TierChoice::parse(Some("AUTO")), TierChoice::Auto);
        assert_eq!(TierChoice::parse(Some("full")), TierChoice::Full);
        assert_eq!(TierChoice::parse(Some("All")), TierChoice::Full);
        assert_eq!(TierChoice::parse(Some("plain")), TierChoice::Plain);
        assert_eq!(TierChoice::parse(Some("html")), TierChoice::Plain);
        let typo = TierChoice::parse(Some("fulll"));
        assert_eq!(typo, TierChoice::Unrecognized("fulll".into()));
        let offer = skill_offer(
            ModelFacts {
                id: Some("qwen2.5:7b"),
                ..ModelFacts::default()
            },
            &typo,
        );
        // The rule still decides, and the reason says the value was ignored.
        assert_eq!(offer.tier, SkillTier::Plain);
        assert_eq!(offer.setting, "auto");
        assert!(offer.reason.contains("fulll"), "{}", offer.reason);
    }

    /// The list is names, so a rename of the skill directory would quietly
    /// turn routing off. Every name in it has to be a skill this repository
    /// ships.
    #[test]
    fn every_component_skill_is_one_the_repository_ships() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("zorp-agent has a parent")
            .join(".claude")
            .join("skills");
        let (registry, _) = zorp_skill::SkillRegistry::discover(&[dir]);
        for name in COMPONENT_SKILLS {
            assert!(registry.get(name).is_some(), "{name} is not shipped");
        }
        // And the plain skill a small model falls back to is there too.
        assert!(registry.get("landing-page").is_some());
    }

    #[test]
    fn withheld_names_only_the_component_skills_on_the_plain_tier() {
        let installed = [
            "artifact-design",
            "landing-page",
            "react-components",
            "mine",
        ];
        assert_eq!(
            offer("qwen2.5:7b").withheld(installed),
            vec!["react-components"]
        );
        assert!(offer("claude-opus-5").withheld(installed).is_empty());
        // Nothing to withhold when the skill is not installed at all.
        assert!(offer("qwen2.5:7b").withheld(["landing-page"]).is_empty());
    }
}
