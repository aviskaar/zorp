use std::fs;
use tempfile::tempdir;
use zorp_train::environment::TrainingEnvironment;
use zorp_train::manifest::CheckpointMetadata;
use zorp_train::registry::ModelRegistry;

#[test]
fn test_registry_empty_dir() {
    let tmp = tempdir().unwrap();
    let reg = ModelRegistry::new(tmp.path().to_path_buf());
    let list = reg.list_checkpoints();
    assert_eq!(list.len(), 0);
}

#[test]
fn test_registry_nonexistent_dir() {
    let tmp = tempdir().unwrap();
    let reg = ModelRegistry::new(tmp.path().join("does_not_exist"));
    let list = reg.list_checkpoints();
    assert_eq!(list.len(), 0);
}

#[test]
fn test_registry_list_checkpoints_direct_and_nested() {
    let tmp = tempdir().unwrap();
    let models_dir = tmp.path().to_path_buf();

    // 1. Direct checkpoint: models_dir/model_alpha/model.safetensors
    let dir_alpha = models_dir.join("model_alpha");
    fs::create_dir_all(&dir_alpha).unwrap();
    fs::write(dir_alpha.join("model.safetensors"), b"alpha_weights").unwrap();

    // 2. Nested checkpoint in runs/*/checkpoints/*: models_dir/runs/run_beta/checkpoints/step_100/model.safetensors
    let dir_beta = models_dir
        .join("runs")
        .join("run_beta")
        .join("checkpoints")
        .join("step_100");
    fs::create_dir_all(&dir_beta).unwrap();
    fs::write(dir_beta.join("model.safetensors"), b"beta_weights").unwrap();

    // 3. Nested checkpoint with custom checkpoint.json: models_dir/runs/run_gamma/step_250/model.safetensors
    let dir_gamma = models_dir.join("runs").join("run_gamma").join("step_250");
    fs::create_dir_all(&dir_gamma).unwrap();
    fs::write(dir_gamma.join("model.safetensors"), b"gamma_weights").unwrap();
    let gamma_meta = CheckpointMetadata {
        run_id: "custom_gamma".to_string(),
        step: 250,
        loss: 1.45,
        checkpoint_dir: dir_gamma.to_string_lossy().to_string(),
        created_at_iso: "2026-09-14T10:00:00Z".to_string(),
    };
    fs::write(
        dir_gamma.join("checkpoint.json"),
        serde_json::to_string(&gamma_meta).unwrap(),
    )
    .unwrap();

    // 4. Ignored directory without model.safetensors
    let dir_ignored = models_dir.join("ignored_dir");
    fs::create_dir_all(&dir_ignored).unwrap();
    fs::write(dir_ignored.join("notes.txt"), b"not a model").unwrap();

    let reg = ModelRegistry::new(models_dir);
    let mut list = reg.list_checkpoints();
    assert_eq!(list.len(), 3, "expected 3 checkpoints, got {:?}", list);

    // Sort by run_id for deterministic assertion
    list.sort_by(|a, b| a.run_id.cmp(&b.run_id));

    // Assert custom_gamma metadata
    let gamma = list
        .iter()
        .find(|c| c.run_id == "custom_gamma")
        .expect("custom_gamma found");
    assert_eq!(gamma.step, 250);
    assert_eq!(gamma.loss, 1.45);
    assert_eq!(gamma.created_at_iso, "2026-09-14T10:00:00Z");

    // Assert model_alpha metadata
    let alpha = list
        .iter()
        .find(|c| c.run_id == "model_alpha")
        .expect("model_alpha found");
    assert_eq!(alpha.step, 0);

    // Assert run_beta metadata
    let beta = list
        .iter()
        .find(|c| c.run_id == "run_beta")
        .expect("run_beta found");
    assert_eq!(beta.step, 100);
}

#[cfg(unix)]
#[tokio::test]
async fn test_registry_serve_and_stop_lifecycle() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let env_dir = tmp.path().join("mock-env");
    let bin_dir = env_dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    // Python wrapper script pointing to system python3
    let py_path = bin_dir.join("python3");
    let wrapper = "#!/bin/sh\nexec python3 \"$@\"\n";
    fs::write(&py_path, wrapper).unwrap();
    let mut perms = fs::metadata(&py_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&py_path, perms).unwrap();

    let env = TrainingEnvironment::new(env_dir);

    let models_dir = tmp.path().join("models");
    let ckpt_dir = models_dir.join("test_model");
    fs::create_dir_all(&ckpt_dir).unwrap();
    fs::write(ckpt_dir.join("model.safetensors"), b"weights").unwrap();

    let reg = ModelRegistry::new(models_dir);
    assert!(!reg.is_serving());
    assert_eq!(reg.active_port(), None);

    // Launch server on ephemeral port
    let port = reg
        .serve_checkpoint(&env, &ckpt_dir)
        .await
        .expect("server starts");
    assert!(port > 0, "bound port must be > 0");
    assert!(reg.is_serving());
    assert_eq!(reg.active_port(), Some(port));

    // Verify tempfile was used and checkpoint_dir is completely clean of python scripts
    let ckpt_entries: Vec<_> = fs::read_dir(&ckpt_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(
        ckpt_entries,
        vec!["model.safetensors"],
        "checkpoint dir must remain clean without temporary script files"
    );

    // Test POST /v1/chat/completions
    let chat_url = format!("http://127.0.0.1:{port}/v1/chat/completions");
    let chat_req = serde_json::json!({
        "model": "test-model",
        "messages": [{"role": "user", "content": "What is reinforcement learning?"}]
    });
    let chat_resp: serde_json::Value = ureq::post(&chat_url)
        .send_json(chat_req)
        .expect("chat completion request succeeds")
        .into_json()
        .expect("valid JSON response");

    assert_eq!(chat_resp["model"], "zorp-local-model");
    assert!(chat_resp["choices"][0]["message"]["content"]
        .as_str()
        .unwrap()
        .contains("Machine learning architectures"));

    // Test POST /v1/completions
    let compl_url = format!("http://127.0.0.1:{port}/v1/completions");
    let compl_req = serde_json::json!({
        "prompt": "The transformer architecture",
        "max_tokens": 50
    });
    let compl_resp: serde_json::Value = ureq::post(&compl_url)
        .send_json(compl_req)
        .expect("completion request succeeds")
        .into_json()
        .expect("valid JSON response");

    assert!(compl_resp["choices"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Machine learning architectures"));

    // Stop server
    reg.stop_server().expect("stop_server succeeds");
    assert!(!reg.is_serving());
    assert_eq!(reg.active_port(), None);

    // Repeated stop should be safe
    reg.stop_server().expect("repeated stop_server succeeds");
}

/// A checkpoint id from a request is looked up, never joined onto a path.
///
/// `serve_checkpoint` starts a Python process pointed at the directory it
/// is handed. When the id was joined onto the models directory, `../`
/// segments walked out of it, and an id that was already absolute skipped
/// it entirely, so a request could name any directory on the machine.
/// Resolution answers only with a directory `list_checkpoints` found.
#[test]
fn resolve_checkpoint_refuses_a_path_the_listing_never_offered() {
    let tmp = tempdir().unwrap();
    let models_dir = tmp.path().join("models");

    let listed = models_dir.join("run_alpha");
    fs::create_dir_all(&listed).unwrap();
    fs::write(listed.join("model.safetensors"), b"alpha").unwrap();

    // A checkpoint shaped directory that sits outside the models directory.
    let outside = tmp.path().join("elsewhere").join("not_ours");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("model.safetensors"), b"someone else's").unwrap();

    let reg = ModelRegistry::new(models_dir.clone());

    // The listing's own path, and the path under the models directory, are
    // the two things a browser sends back.
    assert_eq!(
        reg.resolve_checkpoint(&listed.to_string_lossy()),
        Some(listed.clone())
    );
    assert_eq!(reg.resolve_checkpoint("run_alpha"), Some(listed));

    // An absolute path outside the models directory.
    assert_eq!(reg.resolve_checkpoint(&outside.to_string_lossy()), None);

    // The same one reached by walking out of the models directory.
    let traversal = format!(
        "../{}/{}",
        tmp.path()
            .join("elsewhere")
            .file_name()
            .unwrap()
            .to_string_lossy(),
        "not_ours"
    );
    assert_eq!(reg.resolve_checkpoint(&traversal), None);

    // And a directory that exists but holds no weights, so the listing
    // never named it.
    fs::create_dir_all(models_dir.join("empty_dir")).unwrap();
    assert_eq!(reg.resolve_checkpoint("empty_dir"), None);
    assert_eq!(reg.resolve_checkpoint(""), None);
}
