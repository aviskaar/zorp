use crate::config::TelemetryThresholds;
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone)]
pub struct Transcript {
    pub turns: u32,
    pub tokens: u32,
    pub latency_ms: u64,
}

pub struct EvalContext {
    pub workspace_path: PathBuf,
    pub transcript: Option<Transcript>,
}

#[derive(Debug)]
pub struct GraderResult {
    pub passed: bool,
    pub reason: String,
}

pub trait Grader: Send + Sync {
    fn evaluate(&self, ctx: &EvalContext) -> anyhow::Result<GraderResult>;
}

pub struct ScriptGrader {
    pub command: String,
}

impl Grader for ScriptGrader {
    fn evaluate(&self, ctx: &EvalContext) -> anyhow::Result<GraderResult> {
        let trimmed = self.command.trim();
        if trimmed.is_empty() {
            return Ok(GraderResult {
                passed: false,
                reason: "Empty command".to_string(),
            });
        }

        let output_result = Command::new("sh")
            .arg("-c")
            .arg(trimmed)
            .current_dir(&ctx.workspace_path)
            .output();

        match output_result {
            Ok(output) => {
                let passed = output.status.success();
                Ok(GraderResult {
                    passed,
                    reason: format!("Exit code: {}", output.status),
                })
            }
            Err(e) => Ok(GraderResult {
                passed: false,
                reason: format!("Failed to execute command: {}", e),
            }),
        }
    }
}

pub struct TelemetryGrader {
    pub thresholds: TelemetryThresholds,
}

impl Grader for TelemetryGrader {
    fn evaluate(&self, ctx: &EvalContext) -> anyhow::Result<GraderResult> {
        let Some(transcript) = &ctx.transcript else {
            return Ok(GraderResult {
                passed: false,
                reason: "No transcript found".to_string(),
            });
        };

        if let Some(max_turns) = self.thresholds.max_turns {
            if transcript.turns > max_turns {
                return Ok(GraderResult {
                    passed: false,
                    reason: format!("Exceeded max turns {} > {}", transcript.turns, max_turns),
                });
            }
        }

        if let Some(max_tokens) = self.thresholds.max_tokens {
            if transcript.tokens > max_tokens {
                return Ok(GraderResult {
                    passed: false,
                    reason: format!("Exceeded max tokens {} > {}", transcript.tokens, max_tokens),
                });
            }
        }

        Ok(GraderResult {
            passed: true,
            reason: "Under thresholds".to_string(),
        })
    }
}

pub struct LlmRubricGrader {
    pub rubric: String,
    pub api_url: String,
}

impl Grader for LlmRubricGrader {
    fn evaluate(&self, _ctx: &EvalContext) -> anyhow::Result<GraderResult> {
        // No LLM call exists yet. Returning a fabricated pass here would
        // silently corrupt eval results, so fail loudly instead.
        anyhow::bail!(
            "LlmRubricGrader is not implemented: refusing to fabricate a grading result \
             (rubric: {:?}, api_url: {:?})",
            self.rubric,
            self.api_url
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_script_grader() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("verify.sh");
        fs::write(&script_path, "#!/bin/sh\nexit 0").unwrap();

        let grader = ScriptGrader {
            command: "sh verify.sh".to_string(),
        };
        let ctx = EvalContext {
            workspace_path: dir.path().to_path_buf(),
            transcript: None,
        };

        let result = grader.evaluate(&ctx).unwrap();
        assert!(result.passed);
    }

    #[test]
    fn test_script_grader_nonzero_exit() {
        let dir = tempfile::tempdir().unwrap();
        let grader = ScriptGrader {
            command: "false".to_string(),
        };
        let ctx = EvalContext {
            workspace_path: dir.path().to_path_buf(),
            transcript: None,
        };

        let result = grader.evaluate(&ctx).unwrap();
        assert!(!result.passed);
        assert!(result.reason.contains("Exit code:"));
    }

    #[test]
    fn test_script_grader_empty_command() {
        let dir = tempfile::tempdir().unwrap();
        let grader = ScriptGrader {
            command: "   ".to_string(),
        };
        let ctx = EvalContext {
            workspace_path: dir.path().to_path_buf(),
            transcript: None,
        };

        let result = grader.evaluate(&ctx).unwrap();
        assert!(!result.passed);
        assert_eq!(result.reason, "Empty command");
    }

    #[test]
    fn test_script_grader_failed_to_spawn() {
        let grader = ScriptGrader {
            command: "echo hello".to_string(),
        };
        let ctx = EvalContext {
            workspace_path: PathBuf::from("/dir_that_does_not_exist_zorp_test"),
            transcript: None,
        };

        let result = grader.evaluate(&ctx).unwrap();
        assert!(!result.passed);
        assert!(result.reason.contains("Failed to execute command:"));
    }

    #[test]
    fn test_telemetry_grader_no_transcript() {
        let grader = TelemetryGrader {
            thresholds: TelemetryThresholds {
                max_turns: Some(10),
                max_tokens: None,
            },
        };
        let ctx = EvalContext {
            workspace_path: PathBuf::new(),
            transcript: None,
        };

        let result = grader.evaluate(&ctx).unwrap();
        assert!(!result.passed);
        assert_eq!(result.reason, "No transcript found");
    }

    #[test]
    fn test_telemetry_grader_pass() {
        let grader = TelemetryGrader {
            thresholds: TelemetryThresholds {
                max_turns: Some(10),
                max_tokens: Some(1000),
            },
        };
        let ctx = EvalContext {
            workspace_path: PathBuf::new(),
            transcript: Some(Transcript {
                turns: 5,
                tokens: 500,
                latency_ms: 100,
            }),
        };

        let result = grader.evaluate(&ctx).unwrap();
        assert!(result.passed);
        assert_eq!(result.reason, "Under thresholds");
    }

    #[test]
    fn test_telemetry_grader_exceeds_turns() {
        let grader = TelemetryGrader {
            thresholds: TelemetryThresholds {
                max_turns: Some(5),
                max_tokens: None,
            },
        };
        let ctx = EvalContext {
            workspace_path: PathBuf::new(),
            transcript: Some(Transcript {
                turns: 6,
                tokens: 500,
                latency_ms: 100,
            }),
        };

        let result = grader.evaluate(&ctx).unwrap();
        assert!(!result.passed);
        assert!(result.reason.contains("Exceeded max turns 6 > 5"));
    }

    #[test]
    fn test_telemetry_grader_exceeds_tokens() {
        let grader = TelemetryGrader {
            thresholds: TelemetryThresholds {
                max_turns: None,
                max_tokens: Some(1000),
            },
        };
        let ctx = EvalContext {
            workspace_path: PathBuf::new(),
            transcript: Some(Transcript {
                turns: 5,
                tokens: 1001,
                latency_ms: 100,
            }),
        };

        let result = grader.evaluate(&ctx).unwrap();
        assert!(!result.passed);
        assert!(result.reason.contains("Exceeded max tokens 1001 > 1000"));
    }

    #[test]
    fn test_llm_rubric_grader_is_not_implemented() {
        let grader = LlmRubricGrader {
            rubric: "Be polite".to_string(),
            api_url: "http://example.com/api".to_string(),
        };
        let ctx = EvalContext {
            workspace_path: PathBuf::new(),
            transcript: None,
        };

        // It must not fabricate a pass. A loud error is the only honest
        // result until a real LLM call exists.
        let err = grader.evaluate(&ctx).unwrap_err().to_string();
        assert!(err.contains("not implemented"), "unexpected error: {err}");
    }
}
