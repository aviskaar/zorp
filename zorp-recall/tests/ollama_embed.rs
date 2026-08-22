//! Does a real local model actually put "invoice" near "refund"?
//!
//! `tests/index.rs` pins the search machinery with a stub embedder, which
//! is the right thing for a suite that has to run on a machine with no
//! models on it. It does not answer the question above, and the question
//! above is what decides whether the feature is any good.
//!
//! So this file asks it for real, against a model on this machine, and is
//! `#[ignore]`d so nobody's `cargo test` fails because they have not
//! installed Ollama. Same arrangement as
//! `zorp-agent/tests/ollama_calibration.rs`.
//!
//! Run with:
//!   ollama pull nomic-embed-text
//!   cargo test -p zorp-recall --test ollama_embed -- --ignored --nocapture

mod common;

use zorp_recall::{Embedder, Index, OllamaEmbedder, DEFAULT_EMBED_MODEL, DEFAULT_EMBED_URL};

fn local() -> OllamaEmbedder {
    let url = std::env::var("ZORP_EMBED_URL").unwrap_or_else(|_| DEFAULT_EMBED_URL.to_string());
    let model =
        std::env::var("ZORP_EMBED_MODEL").unwrap_or_else(|_| DEFAULT_EMBED_MODEL.to_string());
    OllamaEmbedder::at(&url, &model).expect("the embedding endpoint must be on this device")
}

#[test]
#[ignore = "needs a local embedding model, see the module comment"]
fn a_real_model_finds_a_conversation_by_meaning() {
    let embedder = local();
    let mut index = Index::open_in_memory().unwrap();
    index.prepare(&embedder.identity()).unwrap();

    let conversations = [
        (
            "conv-money",
            "Sorting out the account",
            "the customer wants their money back for last month's charge",
        ),
        (
            "conv-dog",
            "Walking the dog",
            "the retriever chewed through her leash again on the way to the park",
        ),
        (
            "conv-rust",
            "Borrow checker trouble",
            "the compiler says the value does not live long enough",
        ),
    ];
    for (id, title, text) in conversations {
        let vector = embedder.embed(text).expect("a local embedding");
        index
            .replace(
                zorp_recall::Conversation {
                    id: id.to_string(),
                    title: title.to_string(),
                    updated: 0,
                    fingerprint: "fp".to_string(),
                },
                &embedder.identity(),
                &[(
                    zorp_recall::Chunk {
                        seq: 0,
                        role: "user".into(),
                        text: text.to_string(),
                    },
                    vector,
                )],
            )
            .unwrap();
    }

    // No word in this query appears in any conversation above.
    let query = "billing refund invoice dispute";
    for term in query.split_whitespace() {
        assert!(
            !conversations.iter().any(|(_, _, t)| t.contains(term)),
            "{term:?} is in the corpus, so this test proves nothing"
        );
    }

    let hits = index
        .search(&embedder.embed(query).expect("a local embedding"), 3)
        .unwrap();
    for hit in &hits {
        println!("{:>8.4}  {}  {}", hit.score, hit.conversation_id, hit.title);
    }
    assert_eq!(
        hits[0].conversation_id, "conv-money",
        "the model did not rank the money conversation first"
    );
}

/// The guard is not a test fixture. It refuses a real remote endpoint even
/// on a machine that could reach one.
#[test]
#[ignore = "needs a local embedding model, see the module comment"]
fn the_guard_still_refuses_a_remote_endpoint() {
    assert!(OllamaEmbedder::at("https://api.openai.com/v1", "text-embedding-3-small").is_err());
}
