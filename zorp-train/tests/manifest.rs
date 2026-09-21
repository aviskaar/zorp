use zorp_train::manifest::{
    ArchitectureRecipe, CheckpointMetadata, DatasetManifest, TokenizerConfig, TrainEvent,
    TrainingJobConfig,
};

#[test]
fn test_architecture_recipe_serde() {
    let yaml = r#"
name: zorp-dense-250m
family: qwen-inspired
vocab_size: 32768
max_position_embeddings: 2048
hidden_size: 896
intermediate_size: 2432
num_hidden_layers: 24
num_attention_heads: 14
num_key_value_heads: 2
rms_norm_eps: 0.000001
rope_theta: 1000000.0
qk_norm: true
tie_word_embeddings: true
"#;
    let recipe: ArchitectureRecipe = serde_yaml::from_str(yaml).expect("parse recipe");
    assert_eq!(recipe.name, "zorp-dense-250m");
    assert_eq!(recipe.hidden_size, 896);
    assert_eq!(recipe.num_attention_heads, 14);
    assert_eq!(recipe.num_key_value_heads, 2);
    assert!(recipe.qk_norm);
    assert!(recipe.tie_word_embeddings);
}

#[test]
fn test_dataset_manifest_serde() {
    let json = r#"{
        "id": "fineweb-edu-sub",
        "name": "FineWeb-Edu Subset",
        "source_path": "/tmp/data.jsonl",
        "total_documents": 1000,
        "approx_tokens": 1500000,
        "sample_documents": ["First document text", "Second document text"]
    }"#;
    let manifest: DatasetManifest = serde_json::from_str(json).expect("parse dataset manifest");
    assert_eq!(manifest.id, "fineweb-edu-sub");
    assert_eq!(manifest.total_documents, 1000);
    assert_eq!(manifest.sample_documents.len(), 2);
}

#[test]
fn test_tokenizer_config_default_special_tokens() {
    let json = r#"{
        "name": "zorp-tok-32k",
        "vocab_size": 32768
    }"#;
    let config: TokenizerConfig = serde_json::from_str(json).expect("parse tokenizer config");
    assert_eq!(config.name, "zorp-tok-32k");
    assert_eq!(config.vocab_size, 32768);
    assert_eq!(config.special_tokens.len(), 4);
    assert_eq!(config.special_tokens[0], "<|endoftext|>");
}

#[test]
fn test_training_job_config_serde() {
    let config = TrainingJobConfig {
        run_id: "run-001".to_string(),
        dataset_id: "ds-001".to_string(),
        tokenizer_name: "tok-32k".to_string(),
        recipe_name: "dense-250m".to_string(),
        batch_size: 4,
        gradient_accumulation_steps: 8,
        learning_rate: 3e-4,
        warmup_steps: 100,
        max_tokens: 1_000_000,
        checkpoint_every_steps: 500,
        sample_every_steps: 100,
        tokenizer_dir: None,
        dataset_path: None,
    };
    let serialized = serde_json::to_string(&config).expect("serialize config");
    let deserialized: TrainingJobConfig =
        serde_json::from_str(&serialized).expect("deserialize config");
    assert_eq!(config, deserialized);
}

#[test]
fn test_checkpoint_metadata_serde() {
    let meta = CheckpointMetadata {
        run_id: "run-001".to_string(),
        step: 500,
        loss: 2.45,
        checkpoint_dir: "/checkpoints/step-500".to_string(),
        created_at_iso: "2026-09-14T20:00:00Z".to_string(),
    };
    let serialized = serde_json::to_string(&meta).expect("serialize metadata");
    let deserialized: CheckpointMetadata =
        serde_json::from_str(&serialized).expect("deserialize metadata");
    assert_eq!(meta, deserialized);
}

#[test]
fn test_train_event_serde() {
    let event = TrainEvent::Step {
        step: 42,
        loss: 1.85,
        lr: 0.0003,
        tokens: 100000,
        tok_per_sec: 1250.5,
        memory_gb: 14.2,
        eta_seconds: 3600,
    };
    let serialized = serde_json::to_string(&event).expect("serialize event");
    assert!(serialized.contains("\"type\":\"step\""));
    let deserialized: TrainEvent = serde_json::from_str(&serialized).expect("deserialize event");
    assert_eq!(event, deserialized);
}

/// The Python side says whether it trained on a corpus or on synthetic
/// tokens. Serde drops unknown fields by default, so before these fields
/// existed on the enum that report parsed fine and was silently thrown
/// away, and the browser drew a loss curve with nothing saying what made
/// it. This is the test that fails if they go again.
#[test]
fn an_init_event_carries_what_the_run_trained_on() {
    let on_corpus: TrainEvent = serde_json::from_str(
        r#"{"type":"init","parameters":5476352,"device":"Apple Metal","memory_total_gb":0.02,
             "data":"corpus","corpus_tokens":818184,"dropped_tokens":109804}"#,
    )
    .expect("deserialize init");
    assert_eq!(
        on_corpus,
        TrainEvent::Init {
            parameters: 5_476_352,
            device: "Apple Metal".to_string(),
            memory_total_gb: 0.02,
            data: Some("corpus".to_string()),
            corpus_tokens: 818_184,
            dropped_tokens: 109_804,
        }
    );

    let synthetic: TrainEvent = serde_json::from_str(
        r#"{"type":"init","parameters":1,"device":"Apple Metal","memory_total_gb":0.0,
             "data":"synthetic","corpus_tokens":0,"dropped_tokens":0}"#,
    )
    .expect("deserialize init");
    match synthetic {
        TrainEvent::Init { data, .. } => assert_eq!(data.as_deref(), Some("synthetic")),
        other => panic!("expected an init event, got {other:?}"),
    }

    // A run that said nothing keeps its init event and loses only the
    // claim. Dropping the whole event would take the parameter count and
    // the device off the page too.
    let silent: TrainEvent = serde_json::from_str(
        r#"{"type":"init","parameters":7,"device":"Apple Metal","memory_total_gb":1.0}"#,
    )
    .expect("deserialize init");
    match silent {
        TrainEvent::Init {
            data, parameters, ..
        } => {
            assert_eq!(data, None, "absent must not read as synthetic");
            assert_eq!(parameters, 7);
        }
        other => panic!("expected an init event, got {other:?}"),
    }
}
