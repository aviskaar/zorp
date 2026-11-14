//! The browser's emoji table and the terminal's cannot drift.
//!
//! Both are generated from one source by `web/scripts/gen-emoji.mjs`, but a
//! generated file is only as good as the last time someone ran the script. If
//! one surface's table is regenerated and the other is not, `:rocket:` would
//! expand to a rocket in the browser and stay literal in the terminal, or
//! worse expand to two different characters. This reads both files and asserts
//! the shortcode-to-character maps agree, in the shape of
//! `there_is_one_chunker_and_one_fingerprint`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use zorp_agent::emoji_table::SHORTCODES;

/// Parse `"<alias>": "<char>",` pairs out of the generated TypeScript table,
/// stopping at the catalog (whose lines have a different shape).
fn browser_table() -> BTreeMap<String, String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../web/src/emoji-table.ts");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    // Only the EMOJI_SHORTCODES object, not the catalog after it. Split on the
    // catalog's declaration, not the bare word, which also appears in the
    // header comment.
    let codes = text
        .split("export const EMOJI_CATALOG")
        .next()
        .expect("emoji-table.ts has no EMOJI_CATALOG declaration, so its shape changed");
    let re = regex::Regex::new(r#"(?m)^\s*"([a-z0-9_+-]+)": "([^"]*)",$"#).unwrap();
    re.captures_iter(codes)
        .map(|c| (c[1].to_string(), c[2].to_string()))
        .collect()
}

fn terminal_table() -> BTreeMap<String, String> {
    SHORTCODES
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn the_two_emoji_tables_agree() {
    let browser = browser_table();
    let terminal = terminal_table();

    assert!(
        browser.len() > 1000,
        "only {} shortcodes parsed from the TS table, the parser or the file shape changed",
        browser.len()
    );
    assert_eq!(
        browser.len(),
        terminal.len(),
        "browser has {} shortcodes, terminal has {}",
        browser.len(),
        terminal.len()
    );

    for (alias, ch) in &terminal {
        match browser.get(alias) {
            Some(other) => assert_eq!(
                ch, other,
                "shortcode :{alias}: is {ch:?} in the terminal and {other:?} in the browser"
            ),
            None => panic!(":{alias}: is in the terminal table but not the browser's"),
        }
    }
    for alias in browser.keys() {
        assert!(
            terminal.contains_key(alias),
            ":{alias}: is in the browser table but not the terminal's"
        );
    }
}
