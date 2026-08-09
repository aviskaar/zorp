"""
ZORP Harbor Agent Adapter
===========================
Wraps `zorp-agent` as a Harbor-compatible BaseInstalledAgent so that
Terminal-Bench tasks can be evaluated against ZORP through the Harbor
framework (https://harborframework.com).

Usage:
    pip install harbor
    harbor run \
      -d terminal-bench/terminal-bench-2 \
      -m qwen3.6:35b-mlx \
      --agent evals.harbor.zorp_agent:ZorpAgent

Requirements:
    pip install harbor            # Harbor SDK
    # zorp-agent binary must be on PATH or set ZORP_AGENT_BIN env var
"""

from __future__ import annotations

import os
import subprocess
import shutil
from pathlib import Path
from typing import Any

# Harbor SDK — install via `pip install harbor`
try:
    from harbor.agents.installed.base import BaseInstalledAgent, TaskEnvironment
except ImportError:
    raise ImportError(
        "Harbor SDK not installed. Run: pip install harbor"
    )

# Path to zorp-agent binary — override with ZORP_AGENT_BIN env var
_DEFAULT_BIN = str(
    Path(__file__).resolve().parents[2] / "target" / "release" / "zorp-agent"
)
ZORP_AGENT_BIN = os.environ.get("ZORP_AGENT_BIN", _DEFAULT_BIN)


class ZorpAgent(BaseInstalledAgent):
    """
    Harbor agent adapter for zorp-agent.

    The agent binary is run non-interactively (--yes) inside the task's
    working directory. Model and endpoint are configurable via environment
    variables that `zorp-agent` already reads natively:

        ZORP_MODEL        — model identifier (default: qwen3.6:35b-mlx)
        ZORP_BASE_URL     — OpenAI-compat API base URL (default: http://localhost:11434/v1)
        ZORP_API_KEY      — API key if required

    Harbor passes the task instruction as `task.instruction` and the
    working directory as `task.workdir`.
    """

    name = "zorp-agent"
    description = (
        "ZORP coding agent — a 3.5 MB statically-linked harness "
        "that routes to any OpenAI-compatible local or remote model."
    )

    def install(self) -> None:
        """Verify the binary exists and is executable."""
        if not shutil.which(ZORP_AGENT_BIN) and not Path(ZORP_AGENT_BIN).is_file():
            raise FileNotFoundError(
                f"zorp-agent binary not found at {ZORP_AGENT_BIN}. "
                "Build it with: cargo build --release -p zorp-agent"
            )

    def run(self, task: TaskEnvironment, **kwargs: Any) -> str:
        """
        Execute zorp-agent with the task instruction in the task workdir.
        Returns the agent's stdout as the trajectory string.
        """
        env = {**os.environ}
        env["ZORP_MODEL"] = os.environ.get("ZORP_MODEL", "qwen3.6:35b-mlx")
        env["ZORP_BASE_URL"] = os.environ.get("ZORP_BASE_URL", "http://localhost:11434/v1")

        result = subprocess.run(
            [ZORP_AGENT_BIN, "--yes", task.instruction],
            cwd=task.workdir,
            env=env,
            capture_output=True,
            text=True,
        )

        if result.returncode != 0:
            # Return stderr so Harbor can surface failure details
            return f"[zorp-agent exited {result.returncode}]\n{result.stderr}"

        return result.stdout
