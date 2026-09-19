use serde::{Deserialize, Serialize};

use crate::manifest::ArchitectureRecipe;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParameterBreakdown {
    pub embedding_params: usize,
    pub attention_params: usize,
    pub mlp_params: usize,
    pub norm_params: usize,
    pub total_params: usize,
}

pub fn default_qwen_recipe() -> ArchitectureRecipe {
    ArchitectureRecipe {
        name: "zorp-dense-250m".to_string(),
        family: "qwen-inspired".to_string(),
        vocab_size: 32768,
        max_position_embeddings: 2048,
        hidden_size: 896,
        intermediate_size: 2432,
        num_hidden_layers: 24,
        num_attention_heads: 14,
        num_key_value_heads: 2,
        rms_norm_eps: 1e-6,
        rope_theta: 1000000.0,
        qk_norm: true,
        tie_word_embeddings: true,
    }
}

pub fn calculate_parameters(recipe: &ArchitectureRecipe) -> ParameterBreakdown {
    let v = recipe.vocab_size;
    let d = recipe.hidden_size;
    let l = recipe.num_hidden_layers;
    let d_ffn = recipe.intermediate_size;
    let h_q = recipe.num_attention_heads;
    let h_kv = recipe.num_key_value_heads;
    let head_dim = d / h_q;

    // Embedding: V * d (if tied, else 2 * V * d)
    let embedding_params = if recipe.tie_word_embeddings {
        v * d
    } else {
        2 * v * d
    };

    // Attention per layer:
    // W_q: d * (h_q * head_dim) = d * d
    // W_k: d * (h_kv * head_dim)
    // W_v: d * (h_kv * head_dim)
    // W_o: (h_q * head_dim) * d = d * d
    // QK RMSNorms (if enabled): 2 * d
    let q_dim = h_q * head_dim;
    let kv_dim = h_kv * head_dim;
    let mut attn_per_layer = (d * q_dim) + (2 * d * kv_dim) + (q_dim * d);
    if recipe.qk_norm {
        attn_per_layer += 2 * d;
    }
    let attention_params = l * attn_per_layer;

    // MLP (SwiGLU) per layer: 3 projections (gate, up, down) -> 3 * (d * d_ffn)
    let mlp_params = l * (3 * d * d_ffn);

    // Layer norms: 2 per layer (input norm + post-attention norm) + final norm
    let norm_params = (l * 2 * d) + d;

    let total_params = embedding_params + attention_params + mlp_params + norm_params;

    ParameterBreakdown {
        embedding_params,
        attention_params,
        mlp_params,
        norm_params,
        total_params,
    }
}
