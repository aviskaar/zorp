#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Mutex, MutexGuard, PoisonError};
use zorp_voice::{
    QwenAsr, SetupBackend, SetupError, SetupStage, VoiceSetup, QWEN_ASR_PACKAGE,
    QWEN_ASR_VLLM_PACKAGE, VOICE_AUTOSTART_VAR,
};

/// Every test here writes a script and then runs it. Linux refuses to run a
/// file that any process holds open for writing, and a child forked by another
/// test's spawn holds every open descriptor until it execs, so one test's write
/// can make another test's exec fail with ETXTBSY. Holding this for the whole
/// test keeps the write and the fork apart. It failed once in CI as
/// PythonMissing before the spawn error kept its reason.
fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

fn executable(path: &std::path::Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn fake_python(path: &std::path::Path, log: &std::path::Path, reject_vllm: bool) {
    executable(
        path,
        &format!(
            r#"#!/bin/sh
printf '%s\n' "$@" >> '{}'
if [ "$1" = "-m" ] && [ "$2" = "venv" ]; then
  mkdir -p "$3/bin"
  cp "$0" "$3/bin/python"
  exit 0
fi
if [ "$1" = "-m" ] && [ "$2" = "pip" ]; then
  case "$*" in
    *'[vllm]'*) exit {} ;;
    *) exit 0 ;;
  esac
fi
exit 0
"#,
            log.display(),
            if reject_vllm { 1 } else { 0 },
        ),
    );
}

#[test]
fn setup_installs_the_pinned_runtime_in_a_private_environment() {
    let _serial = serial();
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("argv.log");
    let python = temp.path().join("python3");
    fake_python(&python, &log, false);
    let home = temp.path().join("voice-runtime");
    let client = QwenAsr::at("http://127.0.0.1:8123", "Qwen/model").unwrap();
    let setup = VoiceSetup::new(&client, &home, &python).unwrap();

    let mut stages = Vec::new();
    let backend = setup
        .prepare(|progress| stages.push(progress.stage))
        .unwrap();

    let argv = fs::read_to_string(log).unwrap();
    assert_eq!(backend, SetupBackend::Vllm);
    assert_eq!(
        stages,
        vec![SetupStage::CreatingEnvironment, SetupStage::Installing]
    );
    assert_eq!(
        argv.lines().collect::<Vec<_>>(),
        vec![
            "-m",
            "venv",
            home.to_str().unwrap(),
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            "--no-input",
            QWEN_ASR_VLLM_PACKAGE,
        ]
    );
}

#[test]
fn failed_vllm_resolution_recreates_the_environment_and_uses_transformers() {
    let _serial = serial();
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("argv.log");
    let python = temp.path().join("python3");
    fake_python(&python, &log, true);
    let home = temp.path().join("voice-runtime");
    let client = QwenAsr::at("http://127.0.0.1:8123", "Qwen/model").unwrap();
    let setup = VoiceSetup::new(&client, &home, &python).unwrap();

    let mut stages = Vec::new();
    let backend = setup
        .prepare(|progress| stages.push(progress.stage))
        .unwrap();

    assert_eq!(backend, SetupBackend::Transformers);
    assert_eq!(
        stages,
        vec![
            SetupStage::CreatingEnvironment,
            SetupStage::Installing,
            SetupStage::CreatingEnvironment,
            SetupStage::Installing,
        ]
    );
    let argv = fs::read_to_string(log).unwrap();
    assert!(argv.contains(QWEN_ASR_VLLM_PACKAGE), "{argv}");
    assert!(argv.contains(QWEN_ASR_PACKAGE), "{argv}");
    assert!(home.join("zorp-qwen-asr-server.py").is_file());
}

#[test]
fn setup_never_reuses_an_unmarked_environment() {
    let _serial = serial();
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("voice-runtime");
    fs::create_dir_all(home.join("bin")).unwrap();
    executable(&home.join("bin/python"), "#!/bin/sh\nexit 0\n");
    fs::write(home.join("backend"), "vllm").unwrap();
    let client = QwenAsr::at("http://127.0.0.1:8123", "Qwen/model").unwrap();
    let setup = VoiceSetup::new(&client, &home, temp.path().join("python3")).unwrap();

    let error = setup.prepare(|_| {}).unwrap_err();

    assert!(error.to_string().contains("not marked as zorp-owned"));
    assert!(home.join("bin/python").is_file());
}

#[test]
fn autostart_zero_returns_before_python_or_pip_can_run() {
    let _serial = serial();
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("argv.log");
    let python = temp.path().join("python3");
    fake_python(&python, &log, false);
    let client = QwenAsr::at("http://127.0.0.1:8123", "Qwen/model").unwrap();

    std::env::set_var(VOICE_AUTOSTART_VAR, "0");
    std::env::set_var("ZORP_VOICE_SETUP_DIR", temp.path().join("voice-runtime"));
    std::env::set_var("ZORP_VOICE_PYTHON", &python);
    let result = VoiceSetup::from_env(&client);
    std::env::remove_var(VOICE_AUTOSTART_VAR);
    std::env::remove_var("ZORP_VOICE_SETUP_DIR");
    std::env::remove_var("ZORP_VOICE_PYTHON");

    assert!(matches!(result, Err(SetupError::Disabled { .. })));
    assert!(!log.exists(), "automatic setup ran despite the opt-out");
}

#[test]
fn setup_launches_only_the_validated_target_without_a_shell() {
    let _serial = serial();
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("runtime-argv.log");
    let marker = temp.path().join("shell-injection");
    let home = temp.path().join("voice-runtime");
    fs::create_dir_all(home.join("bin")).unwrap();
    fs::write(home.join(".zorp-voice-environment"), "owned\n").unwrap();
    executable(&home.join("bin/python"), "#!/bin/sh\nexit 0\n");
    executable(
        &home.join("bin/qwen-asr-serve"),
        &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", log.display()),
    );
    let model = format!("Qwen/model; touch {}", marker.display());
    let client = QwenAsr::at("http://127.0.0.1:8123", model).unwrap();
    let setup = VoiceSetup::new(&client, &home, temp.path().join("unused-python")).unwrap();

    let mut stages = Vec::new();
    let mut child = setup
        .start(SetupBackend::Vllm, |progress| stages.push(progress.stage))
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(
        stages,
        vec![SetupStage::DownloadingModel, SetupStage::Loading]
    );

    let argv = fs::read_to_string(log).unwrap();
    assert_eq!(
        argv.lines().collect::<Vec<_>>(),
        vec![
            format!("Qwen/model; touch {}", marker.display()),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            "8123".into(),
        ]
    );
    assert!(
        !marker.exists(),
        "the model argument was interpreted by a shell"
    );
}

#[test]
fn transformers_server_reports_download_and_binds_an_explicit_loopback_address() {
    let _serial = serial();
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("runtime-argv.log");
    let home = temp.path().join("voice-runtime");
    fs::create_dir_all(home.join("bin")).unwrap();
    fs::write(home.join(".zorp-voice-environment"), "owned\n").unwrap();
    executable(
        &home.join("bin/python"),
        &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", log.display()),
    );
    let client = QwenAsr::at("http://localhost:8123", "Qwen/model").unwrap();
    let setup = VoiceSetup::new(&client, &home, temp.path().join("unused-python")).unwrap();

    let mut stages = Vec::new();
    let mut child = setup
        .start(SetupBackend::Transformers, |progress| {
            stages.push(progress.stage)
        })
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(stages, vec![SetupStage::DownloadingModel]);

    let argv = fs::read_to_string(log).unwrap();
    assert_eq!(
        argv.lines().collect::<Vec<_>>(),
        vec![
            home.join("zorp-qwen-asr-server.py")
                .to_str()
                .unwrap()
                .to_string(),
            "--model".into(),
            "Qwen/model".into(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            "8123".into(),
        ]
    );
}

#[test]
fn the_embedded_transformers_server_is_valid_python() {
    let _serial = serial();
    use std::io::Write;
    use std::process::{Command, Stdio};

    let source = include_str!("../src/transformers_server.py");
    let mut child = Command::new("python3")
        .args(["-c", "import ast,sys; ast.parse(sys.stdin.read())"])
        .stdin(Stdio::piped())
        .spawn()
        .expect("python3 is required to create the voice runtime");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
}

#[test]
fn embedded_audio_helpers_use_dtype_max_and_the_verified_result_type() {
    let _serial = serial();
    use std::io::Write;
    use std::process::{Command, Stdio};

    // The probe imports numpy, because what `normalize` promises is a numpy
    // dtype maximum and a stub would only test the stub. Setup installs numpy
    // into its own environment, so the host python3 need not have it, and a
    // machine without it says so rather than reporting a failure it cannot fix.
    // CI is such a machine, which means the assertions below run on developer
    // machines only; `the_embedded_transformers_server_is_valid_python` is what
    // covers this file everywhere.
    let has_numpy = Command::new("python3")
        .args(["-c", "import numpy"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !has_numpy {
        eprintln!("skipping: python3 has no numpy");
        return;
    }

    let source = include_str!("../src/transformers_server.py");
    let probe = r#"
import ast, sys, types
import numpy as np
tree = ast.parse(sys.stdin.read())
wanted = {"normalize", "result_fields"}
nodes = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name in wanted]
namespace = {"np": np}
class ASRTranscription:
    def __init__(self, language, text):
        self.language = language
        self.text = text
namespace["ASRTranscription"] = ASRTranscription
exec(compile(ast.Module(body=nodes, type_ignores=[]), "server", "exec"), namespace)
maximum = np.iinfo(np.int16).max
assert namespace["normalize"](np.array([maximum], dtype=np.int16))[0] == 1.0
try:
    namespace["result_fields"]([{"language": "English", "text": "hello"}])
except ValueError:
    pass
else:
    raise AssertionError("a dictionary was accepted instead of ASRTranscription")
assert namespace["result_fields"]([ASRTranscription("English", "hello")]) == ("English", "hello")
"#;
    let mut child = Command::new("python3")
        .args(["-c", probe])
        .stdin(Stdio::piped())
        .spawn()
        .expect("python3 is required to create the voice runtime");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
}

#[test]
fn automatic_setup_refuses_an_operator_managed_proxy() {
    let _serial = serial();
    let temp = tempfile::tempdir().unwrap();
    let client = QwenAsr::at("https://127.0.0.1:8123/proxy", "Qwen/model").unwrap();
    let error = VoiceSetup::new(
        &client,
        temp.path().join("voice-runtime"),
        temp.path().join("python3"),
    )
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("operator-managed loopback proxy"));
}
