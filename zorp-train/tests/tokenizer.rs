use std::fs;
use tempfile::tempdir;
use zorp_train::environment::TrainingEnvironment;
use zorp_train::manifest::TokenizerConfig;
use zorp_train::tokenizer::{inspect_tokens, train_tokenizer, TokenInspection};

#[test]
fn test_default_special_tokens() {
    let cfg = TokenizerConfig {
        name: "test-bpe".to_string(),
        vocab_size: 4096,
        special_tokens: vec!["<|endoftext|>".to_string()],
    };
    assert_eq!(cfg.vocab_size, 4096);
    assert_eq!(cfg.name, "test-bpe");
    assert_eq!(cfg.special_tokens, vec!["<|endoftext|>".to_string()]);
}

#[test]
fn test_token_inspection_serde() {
    let sample = TokenInspection {
        tokens: vec!["Hello".to_string(), "world".to_string()],
        ids: vec![101, 102],
        char_count: 11,
        token_count: 2,
        compression_chars_per_token: 5.5,
    };
    let json = serde_json::to_string(&sample).expect("serialize");
    let deserialized: TokenInspection = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(sample, deserialized);
}

#[test]
fn test_train_tokenizer_missing_env_fails() {
    let tmp = tempdir().unwrap();
    let env = TrainingEnvironment::new(tmp.path().join("nonexistent-env"));
    let cfg = TokenizerConfig {
        name: "test".to_string(),
        vocab_size: 1000,
        special_tokens: vec!["<|endoftext|>".to_string()],
    };
    let data_file = tmp.path().join("data.jsonl");
    fs::write(&data_file, "{\"text\": \"hello world\"}\n").unwrap();
    let out_dir = tmp.path().join("tokenizer_out");

    let res = train_tokenizer(&env, &cfg, &data_file, &out_dir);
    assert!(res.is_err());
}

#[test]
fn test_inspect_tokens_missing_dir_fails() {
    let tmp = tempdir().unwrap();
    let env = TrainingEnvironment::new(tmp.path().join("env"));
    let res = inspect_tokens(&env, &tmp.path().join("nonexistent_dir"), "sample text");
    assert!(res.is_err());
}

#[cfg(unix)]
#[test]
fn test_train_and_inspect_tokens_e2e() {
    let status = std::process::Command::new("python3")
        .args(["-c", "import tokenizers"])
        .status();
    let tokenizers_available = matches!(status, Ok(s) if s.success());
    if !tokenizers_available {
        eprintln!("Skipping test_train_and_inspect_tokens_e2e: python3 with tokenizers not found");
        return;
    }

    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let env_dir = tmp.path().join("test-env");
    let bin_dir = env_dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let py_wrapper = bin_dir.join("python3");
    let script = "#!/bin/sh\nexec python3 \"$@\"\n";
    fs::write(&py_wrapper, script).unwrap();
    let mut perms = fs::metadata(&py_wrapper).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&py_wrapper, perms).unwrap();

    let env = TrainingEnvironment::new(env_dir);

    // Create small jsonl dataset
    let dataset_path = tmp.path().join("dataset.jsonl");
    let mut data = String::new();
    for i in 0..50 {
        data.push_str(&format!(
            "{{\"text\": \"Document number {} discusses machine learning and tokenization algorithms.\"}}\n",
            i
        ));
    }
    fs::write(&dataset_path, data).unwrap();

    let output_dir = tmp.path().join("bpe_output");
    let cfg = TokenizerConfig {
        name: "test-bpe".to_string(),
        vocab_size: 350,
        special_tokens: vec![
            "<|endoftext|>".to_string(),
            "<|im_start|>".to_string(),
            "<|im_end|>".to_string(),
            "<|pad|>".to_string(),
        ],
    };

    let train_res = train_tokenizer(&env, &cfg, &dataset_path, &output_dir);
    assert!(train_res.is_ok(), "train_tokenizer failed: {:?}", train_res);

    assert!(output_dir.join("tokenizer.json").exists());
    assert!(output_dir.join("tokenizer_config.json").exists());
    assert!(!output_dir.join("_train_bpe_tmp.py").exists());

    let inspect_res = inspect_tokens(&env, &output_dir, "machine learning tokenization");
    assert!(inspect_res.is_ok(), "inspect_tokens failed: {:?}", inspect_res);

    let inspection = inspect_res.unwrap();
    assert!(!inspection.tokens.is_empty());
    assert_eq!(inspection.tokens.len(), inspection.ids.len());
    assert_eq!(inspection.token_count, inspection.ids.len());
    assert_eq!(inspection.char_count, "machine learning tokenization".len());
    assert!(inspection.compression_chars_per_token > 0.0);
    assert!(!output_dir.join("_inspect_tmp.py").exists());

    for hyphen_text in ["--help", "-123", "--model=qwen"] {
        let inspect_res = inspect_tokens(&env, &output_dir, hyphen_text);
        assert!(
            inspect_res.is_ok(),
            "inspect_tokens failed on hyphen-prefixed text {:?}: {:?}",
            hyphen_text,
            inspect_res
        );
        let insp = inspect_res.unwrap();
        assert!(!insp.tokens.is_empty());
        assert_eq!(insp.char_count, hyphen_text.len());
        assert_eq!(insp.tokens.len(), insp.ids.len());
    }
}

#[cfg(unix)]
#[test]
fn test_train_tokenizer_empty_special_tokens() {
    let status = std::process::Command::new("python3")
        .args(["-c", "import tokenizers"])
        .status();
    let tokenizers_available = matches!(status, Ok(s) if s.success());
    if !tokenizers_available {
        eprintln!("Skipping test_train_tokenizer_empty_special_tokens: python3 with tokenizers not found");
        return;
    }

    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let env_dir = tmp.path().join("test-env");
    let bin_dir = env_dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let py_wrapper = bin_dir.join("python3");
    let script = "#!/bin/sh\nexec python3 \"$@\"\n";
    fs::write(&py_wrapper, script).unwrap();
    let mut perms = fs::metadata(&py_wrapper).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&py_wrapper, perms).unwrap();

    let env = TrainingEnvironment::new(env_dir);

    let dataset_path = tmp.path().join("dataset.jsonl");
    let mut data = String::new();
    for i in 0..50 {
        data.push_str(&format!(
            "{{\"text\": \"Sample line {} for training tokenizer with no special tokens.\"}}\n",
            i
        ));
    }
    fs::write(&dataset_path, data).unwrap();

    let output_dir = tmp.path().join("bpe_no_special");
    let cfg = TokenizerConfig {
        name: "test-no-special".to_string(),
        vocab_size: 300,
        special_tokens: vec![],
    };

    let train_res = train_tokenizer(&env, &cfg, &dataset_path, &output_dir);
    assert!(train_res.is_ok(), "train_tokenizer failed: {:?}", train_res);

    let config_content = fs::read_to_string(output_dir.join("tokenizer_config.json")).unwrap();
    let config_val: serde_json::Value = serde_json::from_str(&config_content).unwrap();
    let special = config_val["special_tokens"].as_array().expect("special_tokens array");
    assert!(
        special.is_empty(),
        "expected empty special_tokens array, got {:?}",
        special
    );
}
