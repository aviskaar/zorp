use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DatasetManifest {
    pub id: String,
    pub name: String,
    pub source_path: String,
    pub total_documents: usize,
    pub approx_tokens: usize,
    pub sample_documents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenizerConfig {
    pub name: String,
    pub vocab_size: usize,
    #[serde(default = "default_special_tokens")]
    pub special_tokens: Vec<String>,
}

fn default_special_tokens() -> Vec<String> {
    vec![
        "<|endoftext|>".to_string(),
        "<|im_start|>".to_string(),
        "<|im_end|>".to_string(),
        "<|pad|>".to_string(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArchitectureRecipe {
    pub name: String,
    pub family: String,
    pub vocab_size: usize,
    pub max_position_embeddings: usize,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub num_key_value_heads: usize,
    pub rms_norm_eps: f64,
    pub rope_theta: f64,
    pub qk_norm: bool,
    pub tie_word_embeddings: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrainingJobConfig {
    pub run_id: String,
    pub dataset_id: String,
    pub tokenizer_name: String,
    pub recipe_name: String,
    pub batch_size: usize,
    pub gradient_accumulation_steps: usize,
    pub learning_rate: f64,
    pub warmup_steps: usize,
    pub max_tokens: usize,
    pub checkpoint_every_steps: usize,
    pub sample_every_steps: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckpointMetadata {
    pub run_id: String,
    pub step: usize,
    pub loss: f64,
    pub checkpoint_dir: String,
    pub created_at_iso: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TrainEvent {
    Init {
        parameters: usize,
        device: String,
        memory_total_gb: f64,
    },
    Step {
        step: usize,
        loss: f64,
        lr: f64,
        tokens: usize,
        tok_per_sec: f64,
        memory_gb: f64,
        eta_seconds: u64,
    },
    Sample {
        step: usize,
        prompt: String,
        output: String,
    },
    Checkpoint {
        step: usize,
        loss: f64,
        path: String,
    },
    Error {
        message: String,
    },
}
