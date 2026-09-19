use zorp_train::manifest::ArchitectureRecipe;
use zorp_train::recipe::{calculate_parameters, default_qwen_recipe, ParameterBreakdown};

#[test]
fn test_qwen_dense_250m_parameter_calculation() {
    let recipe = default_qwen_recipe();
    let breakdown = calculate_parameters(&recipe);

    // Embedding: 32768 * 896 = 29,360,128 (tied)
    assert_eq!(breakdown.embedding_params, 29_360_128);
    // Attention: 24 layers * ((896 * 896) + (2 * 896 * 128) + (896 * 896) + (2 * 896))
    // = 24 * (802,816 + 229,376 + 802,816 + 1,792) = 24 * 1,836,800 = 44,083,200
    assert_eq!(breakdown.attention_params, 44_083_200);
    // MLP (SwiGLU): 24 layers * 3 * 896 * 2432 = 24 * 6,537,216 = 156,893,184
    assert_eq!(breakdown.mlp_params, 156_893_184);
    // Norm: (24 * 2 * 896) + 896 = 43,904
    assert_eq!(breakdown.norm_params, 43_904);

    let expected_total = 29_360_128 + 44_083_200 + 156_893_184 + 43_904;
    assert_eq!(breakdown.total_params, expected_total);
    assert_eq!(breakdown.total_params, 230_380_416);

    // Verify total is in the ~220M - 260M range
    assert!(breakdown.total_params > 200_000_000);
    assert!(breakdown.total_params < 270_000_000);
}

#[test]
fn test_untied_embeddings_doubles_embedding_params() {
    let mut recipe = default_qwen_recipe();
    recipe.tie_word_embeddings = false;
    let breakdown = calculate_parameters(&recipe);

    assert_eq!(breakdown.embedding_params, 2 * 32768 * 896);
    assert_eq!(breakdown.embedding_params, 58_720_256);
    assert_eq!(breakdown.total_params, 230_380_416 + 29_360_128);
}

#[test]
fn test_qk_norm_toggle() {
    let mut recipe = default_qwen_recipe();
    recipe.qk_norm = false;
    let breakdown = calculate_parameters(&recipe);

    // QK norm removes 2 * d per layer = 2 * 896 * 24 = 43,008
    let baseline = calculate_parameters(&default_qwen_recipe());
    assert_eq!(
        baseline.attention_params - breakdown.attention_params,
        43_008
    );
    assert_eq!(baseline.total_params - breakdown.total_params, 43_008);
}

#[test]
fn test_parameter_breakdown_serde() {
    let recipe = default_qwen_recipe();
    let breakdown = calculate_parameters(&recipe);

    let serialized = serde_json::to_string(&breakdown).expect("serialize breakdown");
    let deserialized: ParameterBreakdown =
        serde_json::from_str(&serialized).expect("deserialize breakdown");
    assert_eq!(breakdown, deserialized);
}

#[test]
fn test_yaml_recipe_parsing_and_calculation() {
    let yaml = r#"
name: custom-mini
family: qwen-inspired
vocab_size: 1000
max_position_embeddings: 512
hidden_size: 64
intermediate_size: 128
num_hidden_layers: 2
num_attention_heads: 4
num_key_value_heads: 2
rms_norm_eps: 0.000001
rope_theta: 10000.0
qk_norm: true
tie_word_embeddings: true
"#;
    let recipe: ArchitectureRecipe = serde_yaml::from_str(yaml).expect("parse recipe");
    let breakdown = calculate_parameters(&recipe);

    // head_dim = 64 / 4 = 16
    // q_dim = 4 * 16 = 64
    // kv_dim = 2 * 16 = 32
    // embedding: 1000 * 64 = 64_000
    assert_eq!(breakdown.embedding_params, 64_000);
    // attn: 2 * (64*64 + 2*64*32 + 64*64 + 2*64) = 2 * (4096 + 4096 + 4096 + 128) = 2 * 12416 = 24,832
    assert_eq!(breakdown.attention_params, 24_832);
    // mlp: 2 * 3 * 64 * 128 = 49,152
    assert_eq!(breakdown.mlp_params, 49_152);
    // norm: (2 * 2 * 64) + 64 = 320
    assert_eq!(breakdown.norm_params, 320);
    // total: 64000 + 24832 + 49152 + 320 = 138,304
    assert_eq!(breakdown.total_params, 138_304);
}
