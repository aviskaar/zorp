//! The index, and the thing it is for: finding a conversation you cannot
//! spell.
//!
//! The embedder here is a stub, not a model. That is deliberate and it is
//! the honest thing to test: what these cases pin is the search machinery,
//! that a vector close to the query wins and that scoring, roll-up, and
//! incremental reindexing behave. Whether a real model puts "refund" near
//! "invoice" is the model's business, and pinning it here would mean a test
//! suite that needs Ollama installed to run. The real model is exercised by
//! `tests/ollama_embed.rs`, which is `#[ignore]`d for exactly that reason.

use zorp_recall::{Chunk, Index, IndexError};

/// A four-axis topic embedder. Each axis is a set of words with no spelling
/// in common, which is what makes the substring assertion below meaningful.
fn embed(text: &str) -> Vec<f32> {
    const TOPICS: [&[&str]; 4] = [
        &["invoice", "billing", "refund", "charge", "payment", "price"],
        &["dog", "puppy", "retriever", "leash", "kennel"],
        &["rust", "cargo", "crate", "borrow", "lifetime"],
        &["sleep", "insomnia", "nap", "bedtime"],
    ];
    let lowered = text.to_lowercase();
    let mut v = vec![0.0f32; TOPICS.len()];
    for (axis, words) in TOPICS.iter().enumerate() {
        for word in *words {
            if lowered.contains(word) {
                v[axis] += 1.0;
            }
        }
    }
    // A vector of zeroes has no direction, so give unmatched text its own
    // corner rather than making it equidistant from everything.
    if v.iter().all(|x| *x == 0.0) {
        v[0] = 0.01;
        v[1] = -0.01;
    }
    v
}

fn chunk(seq: i64, role: &str, text: &str) -> (Chunk, Vec<f32>) {
    (
        Chunk {
            seq,
            role: role.to_string(),
            text: text.to_string(),
        },
        embed(text),
    )
}

fn corpus() -> Index {
    let mut index = Index::open_in_memory().unwrap();
    index.prepare("stub/topics").unwrap();
    index
        .replace(
            "conv-money",
            "Sorting out the account",
            "fp-money",
            "stub/topics",
            &[
                chunk(
                    0,
                    "user",
                    "the customer wants a refund on last month's charge",
                ),
                chunk(1, "assistant", "I can look at the payment history"),
            ],
        )
        .unwrap();
    index
        .replace(
            "conv-dog",
            "Walking the dog",
            "fp-dog",
            "stub/topics",
            &[chunk(
                0,
                "user",
                "the retriever chewed through her leash again",
            )],
        )
        .unwrap();
    index
        .replace(
            "conv-rust",
            "Borrow checker trouble",
            "fp-rust",
            "stub/topics",
            &[chunk(
                0,
                "user",
                "cargo will not build, the borrow outlives the crate",
            )],
        )
        .unwrap();
    index
}

/// The whole point of the feature. "invoice dispute" appears nowhere in the
/// corpus, letter for letter, and still finds the conversation about a
/// refund. The first assertion proves the substring search really does miss
/// it, so the second one is not passing by accident.
#[test]
fn search_finds_a_conversation_no_substring_match_would() {
    let query = "invoice dispute";
    let texts = [
        "the customer wants a refund on last month's charge",
        "I can look at the payment history",
        "the retriever chewed through her leash again",
        "cargo will not build, the borrow outlives the crate",
    ];
    for term in query.split_whitespace() {
        assert!(
            !texts.iter().any(|t| t.contains(term)),
            "{term:?} is in the corpus, so this test proves nothing"
        );
    }

    let hits = corpus().search(&embed(query), 5).unwrap();
    assert!(!hits.is_empty(), "semantic search returned nothing");
    assert_eq!(hits[0].conversation_id, "conv-money");
    assert_eq!(hits[0].title, "Sorting out the account");
    assert!(hits[0].score > 0.5, "score was {}", hits[0].score);
}

/// One row per conversation, carrying the message that matched. A result
/// list with the same conversation four times is a worse answer than the
/// same list with three conversations in it.
#[test]
fn results_are_one_row_per_conversation_naming_the_message_that_matched() {
    let hits = corpus().search(&embed("refund"), 10).unwrap();
    let money: Vec<_> = hits
        .iter()
        .filter(|h| h.conversation_id == "conv-money")
        .collect();
    assert_eq!(money.len(), 1, "a conversation appeared more than once");
    assert_eq!(money[0].seq, 0);
    assert_eq!(money[0].role, "user");
    assert!(money[0].snippet.contains("refund"));
}

/// Results are ordered by score and cut to the limit.
#[test]
fn results_are_ranked_and_capped() {
    let hits = corpus().search(&embed("puppy"), 2).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].conversation_id, "conv-dog");
    assert!(hits[0].score >= hits[1].score);
}

/// Reindexing one conversation replaces it rather than adding to it.
#[test]
fn replacing_a_conversation_does_not_duplicate_it() {
    let mut index = corpus();
    let before = index.stats().unwrap();
    index
        .replace(
            "conv-dog",
            "Walking the dog",
            "fp-dog-2",
            "stub/topics",
            &[chunk(0, "user", "the puppy needs a new kennel")],
        )
        .unwrap();
    let after = index.stats().unwrap();
    assert_eq!(after.conversations, before.conversations);
    assert_eq!(after.chunks, before.chunks);
    assert_eq!(
        index.fingerprint("conv-dog").unwrap().as_deref(),
        Some("fp-dog-2")
    );
}

/// The fingerprint is how a reindex skips what has not changed. Unknown
/// conversations answer `None`, which is what makes a first run index
/// everything.
#[test]
fn fingerprints_drive_the_incremental_reindex() {
    let index = corpus();
    assert_eq!(
        index.fingerprint("conv-money").unwrap().as_deref(),
        Some("fp-money")
    );
    assert_eq!(index.fingerprint("never-seen").unwrap(), None);
}

/// A conversation the store no longer has stops being searchable.
#[test]
fn retaining_drops_conversations_the_store_no_longer_has() {
    let mut index = corpus();
    let dropped = index
        .retain(&["conv-money".to_string(), "conv-rust".to_string()])
        .unwrap();
    assert_eq!(dropped, 1);
    assert_eq!(index.stats().unwrap().conversations, 2);
    let hits = index.search(&embed("puppy"), 10).unwrap();
    assert!(hits.iter().all(|h| h.conversation_id != "conv-dog"));
}

/// Vectors from two different models are not comparable, and comparing them
/// anyway would return confident nonsense. Switching embedder clears the
/// index, and says it did.
#[test]
fn changing_the_embedder_clears_the_index() {
    let mut index = corpus();
    assert!(index.stats().unwrap().chunks > 0);

    let cleared = index.prepare("stub/other").unwrap();
    assert!(cleared, "switching embedder did not report a clear");
    assert_eq!(index.stats().unwrap().chunks, 0);
    assert_eq!(
        index.stats().unwrap().embedder.as_deref(),
        Some("stub/other")
    );

    // And preparing again with the same one leaves it alone.
    index
        .replace(
            "conv-a",
            "A",
            "fp-a",
            "stub/other",
            &[chunk(0, "user", "refund")],
        )
        .unwrap();
    assert!(!index.prepare("stub/other").unwrap());
    assert_eq!(index.stats().unwrap().chunks, 1);
}

/// Writing a vector under the wrong embedder name is a bug in the caller,
/// not something to paper over.
#[test]
fn writing_under_a_different_embedder_is_refused() {
    let mut index = corpus();
    let err = index
        .replace(
            "conv-x",
            "X",
            "fp-x",
            "stub/other",
            &[chunk(0, "user", "refund")],
        )
        .unwrap_err();
    assert!(
        matches!(err, IndexError::EmbedderMismatch { .. }),
        "{err:?}"
    );
}

/// A query vector of the wrong width is refused with a message that says
/// what to do about it, rather than scoring against whatever happens to be
/// in the first four floats.
#[test]
fn a_query_of_the_wrong_width_is_refused() {
    let err = corpus().search(&[1.0, 0.0], 5).unwrap_err();
    assert!(matches!(err, IndexError::Dimensions { .. }), "{err:?}");
    assert!(err.to_string().contains("reindex"), "{err}");
}

/// An empty index is not an error. It is a search with no results, which is
/// exactly what someone who has not indexed yet should see.
#[test]
fn searching_an_empty_index_returns_nothing() {
    let index = Index::open_in_memory().unwrap();
    assert!(index.search(&embed("refund"), 5).unwrap().is_empty());
    let stats = index.stats().unwrap();
    assert_eq!(stats.conversations, 0);
    assert_eq!(stats.embedder, None);
}

/// The index survives being closed and reopened, which is the only reason
/// it is a file and not a HashMap.
#[test]
fn the_index_persists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recall.db");
    {
        let mut index = Index::open_at(&path).unwrap();
        index.prepare("stub/topics").unwrap();
        index
            .replace(
                "conv-money",
                "Sorting out the account",
                "fp-money",
                "stub/topics",
                &[chunk(0, "user", "a refund on last month's charge")],
            )
            .unwrap();
    }
    let index = Index::open_at(&path).unwrap();
    let hits = index.search(&embed("invoice"), 5).unwrap();
    assert_eq!(hits[0].conversation_id, "conv-money");
}
