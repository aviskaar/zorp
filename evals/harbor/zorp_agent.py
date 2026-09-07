"""Harbor agent adapter for zorp-agent.

Harbor (https://harborframework.com) is the harness Terminal-Bench runs on.
It starts a container per task, hands an agent the instruction, then runs the
task's own tests against what the agent left behind. This adapter is the
`--agent` it can be pointed at.

Run it with:

    harbor run -d terminal-bench-science/terminal-bench-science@latest \
      --agent evals.harbor.zorp_agent:ZorpAgent \
      -m openai/<model> --task-id <task>

See docs/benchmarks.md for the full recipe.

The binary is built from the working tree, not downloaded, because the point
is to benchmark this checkout. Build it first with evals/harbor/build-agent.sh
and the adapter uploads it into each task container.
"""

from __future__ import annotations

import os
import shlex
from pathlib import Path
from typing import override

from harbor.agents.installed.base import (
    ApiError,
    BaseInstalledAgent,
    NonZeroAgentExitCodeError,
    with_prompt_template,
)
from harbor.agents.model_connection import ModelConnectionSpec
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

# Where the adapter puts the binary inside the task container. Harbor's
# BaseInstalledAgent.setup() creates /installed-agent before calling install().
REMOTE_BIN = "/installed-agent/zorp-agent"

# Where the agent runs, and so the root of its path policy. See the class doc.
AGENT_CWD = "/"

# Where the ensemble roles file goes when the host names one in
# ZORP_ENSEMBLE. The container's zorp-agent reads it from here; the
# instruction text never names a role.
REMOTE_ROSTER = "/installed-agent/ensemble.toml"

# `uname -m` in the container to the arch name build-agent.sh writes under.
_ARCH_DIRS = {"aarch64": "arm64", "arm64": "arm64", "x86_64": "amd64"}

_REPO_ROOT = Path(__file__).resolve().parents[2]


class ZorpAgent(BaseInstalledAgent):
    """Runs zorp-agent --yes "<instruction>" inside the task container, or
    zorp-agent ensemble --yes when ZORP_ENSEMBLE names a roles file on the
    host.

    `--yes` answers the asks an approval preset produces. It does not touch
    the hard denylist in zorp-agent/src/policy.rs, which still refuses sudo,
    backticks, `git push`, recursive force-rm outside the working directory
    and shell redirects whose target escapes it.

    The agent runs with `/` as its working directory, so that root is the
    policy's root and every absolute path in the container is inside it.
    The task's own WORKDIR would be the natural choice, but eleven of the
    seventy science tasks want their output outside it (`/results`,
    `/root/results`, `/output`, `/logs`), twenty of them live in `/root`
    where every `> /tmp/x` is a denied redirect, and the file tools refuse
    any absolute path that leaves the root. The container is disposable, so
    nothing is lost by widening the root to all of it. The model then has to
    use absolute paths with the file tools, which the instructions already
    give it.
    """

    # No default_provider, so the provider comes from `-m provider/model`.
    # The resolved api key and base url become ZORP_API_KEY and ZORP_BASE_URL.
    MODEL_CONNECTION = ModelConnectionSpec()

    @staticmethod
    @override
    def name() -> str:
        return "zorp-agent"

    @override
    def get_version_command(self) -> str | None:
        return f"{REMOTE_BIN} --version"

    @override
    def parse_version(self, stdout: str) -> str:
        return stdout.strip().removeprefix("zorp-agent").strip()

    def _host_binary(self, arch: str) -> Path:
        """The Linux binary to upload, for the container's architecture."""
        override_path = os.environ.get("ZORP_AGENT_BIN")
        if override_path:
            return Path(override_path).expanduser()
        return _REPO_ROOT / "target" / "harbor" / arch / "zorp-agent"

    def _roster(self) -> Path | None:
        """The host roles file, when this run is an ensemble run."""
        path = os.environ.get("ZORP_ENSEMBLE")
        return Path(path).expanduser() if path else None

    def _command(self, instruction: str, log_path: str) -> str:
        """The one shell line the container runs, plain or ensemble."""
        mode = "ensemble --yes" if self._roster() else "--yes"
        return (
            f"{REMOTE_BIN} {mode} {shlex.quote(instruction)} "
            f"2>&1 | stdbuf -oL tee {log_path}"
        )

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        uname = await self.exec_as_agent(environment, command="uname -m")
        machine = uname.stdout.strip()
        arch = _ARCH_DIRS.get(machine)
        if arch is None:
            raise RuntimeError(
                f"no zorp-agent build mapping for container arch '{machine}'"
            )

        binary = self._host_binary(arch)
        if not binary.is_file():
            raise FileNotFoundError(
                f"no zorp-agent binary at {binary}. Build one with "
                f"'evals/harbor/build-agent.sh linux/{arch}', or point "
                "ZORP_AGENT_BIN at a Linux build for that architecture."
            )

        await environment.upload_file(binary, REMOTE_BIN)
        await self.exec_as_root(environment, command=f"chmod 0755 {REMOTE_BIN}")

        roster = self._roster()
        if roster:
            if not roster.is_file():
                raise FileNotFoundError(f"ZORP_ENSEMBLE names no file: {roster}")
            await environment.upload_file(roster, REMOTE_ROSTER)

    @override
    @with_prompt_template
    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        env = self._zorp_env()
        log_path = f"{self.environment_logs_dir}/zorp-agent.txt"
        command = self._command(instruction, log_path)

        try:
            await self.exec_as_agent(
                environment, command=command, env=env, cwd=AGENT_CWD
            )
        except ApiError:
            # Reaching the model failed. That is a harness problem, and
            # --retry-include wants to see the specific type.
            raise
        except NonZeroAgentExitCodeError as exc:
            # zorp-agent exits 1 when it hits the step limit, fails its
            # verify gate or gives up. That is a scored zero, not a broken
            # trial, so let the verifier run and say so.
            self.logger.warning(f"zorp-agent exited non-zero: {exc}")

    def _zorp_env(self) -> dict[str, str]:
        """Map Harbor's resolved model connection onto zorp's own env vars."""
        access = self.model_connection
        env: dict[str, str] = {}

        model = self.model_name or ""
        if "/" in model:
            _, model = model.split("/", 1)
        if model:
            env["ZORP_MODEL"] = model

        if access.base_url:
            env["ZORP_BASE_URL"] = access.base_url
        if access.api_key:
            env["ZORP_API_KEY"] = access.api_key
        if access.provider == "anthropic":
            # zorp speaks two wire formats. Everything else is treated as
            # OpenAI-compatible, which is what its default already is.
            env["ZORP_PROVIDER"] = "anthropic"

        # The crate defaults for the retry bound, 4 sends and 30 seconds of
        # added waiting, are tuned for a person at a browser, where a minute
        # of spinner reads as broken. A batch run has nobody watching and
        # waits. These are defaults, not overrides: Harbor lays `--ae` values
        # over this dict (BaseEnvironment._merge_env, per-exec env under the
        # scoped env that carries `--ae`), so a value on the command line wins.
        env["ZORP_RETRY_ATTEMPTS"] = "20"
        env["ZORP_RETRY_BUDGET_SECS"] = "300"

        if self._roster():
            env["ZORP_ENSEMBLE"] = REMOTE_ROSTER
            # Reviewer transcripts and ensemble.json land beside
            # zorp-agent.txt, so a trial is read the way trials are read
            # today and nothing has to be downloaded afterwards.
            env["ZORP_ENSEMBLE_LOG_DIR"] = str(self.environment_logs_dir)

        return env
