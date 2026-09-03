#!/usr/bin/env python3
"""Checks for the Harbor adapter in zorp_agent.py.

These run against the real Harbor SDK on purpose. The version of this file
that stubbed the SDK out passed for a year while the adapter it tested could
not be imported, because the stub described an interface Harbor never had.

    pip install harbor
    python -m unittest discover -s evals/harbor -t .
"""

from __future__ import annotations

import os
import unittest
from pathlib import Path
import asyncio
from unittest.mock import AsyncMock, MagicMock, patch

# Skip only when Harbor itself is absent. An adapter that fails to import
# against an installed Harbor is a drifted interface, which is the one
# thing these tests exist to catch, so that import is deliberately not
# guarded: it must fail, not skip. Harbor 0.18 had no
# `harbor.agents.model_connection`, and a guarded import read that as
# seven passes.
try:
    import harbor  # noqa: F401

    HARBOR_MISSING = ""
except ImportError as exc:  # pragma: no cover - depends on the environment
    HARBOR_MISSING = str(exc)

if not HARBOR_MISSING:
    from evals.harbor.zorp_agent import ZorpAgent


@unittest.skipIf(HARBOR_MISSING, f"harbor not installed: {HARBOR_MISSING}")
class TestZorpAgentAdapter(unittest.TestCase):
    def agent(self, model: str | None = None) -> ZorpAgent:
        # Instantiating at all is the point: BaseInstalledAgent is abstract,
        # so this fails if the adapter misses or misspells a method.
        return ZorpAgent(logs_dir=Path("/tmp"), model_name=model)

    def test_name(self):
        self.assertEqual(ZorpAgent.name(), "zorp-agent")

    def test_import_path_is_the_one_the_docs_use(self):
        self.assertEqual(
            ZorpAgent.import_path(), "evals.harbor.zorp_agent:ZorpAgent"
        )

    def test_anthropic_model_selects_the_anthropic_wire_format(self):
        with patch.dict(os.environ, {"ANTHROPIC_API_KEY": "test-key"}, clear=False):
            env = self.agent("anthropic/claude-opus-5")._zorp_env()
        self.assertEqual(env["ZORP_MODEL"], "claude-opus-5")
        self.assertEqual(env["ZORP_PROVIDER"], "anthropic")
        self.assertEqual(env["ZORP_API_KEY"], "test-key")

    def test_openai_compatible_model_leaves_the_provider_alone(self):
        overrides = {"OPENAI_BASE_URL": "http://host.docker.internal:11434/v1"}
        with patch.dict(os.environ, overrides, clear=False):
            env = self.agent("openai/gemma4:e4b")._zorp_env()
        self.assertEqual(env["ZORP_MODEL"], "gemma4:e4b")
        self.assertEqual(env["ZORP_BASE_URL"], overrides["OPENAI_BASE_URL"])
        self.assertNotIn("ZORP_PROVIDER", env)

    def test_a_keyless_local_endpoint_sends_no_key(self):
        with patch.dict(os.environ, {}, clear=True):
            env = self.agent("openai/gemma4:e4b")._zorp_env()
        self.assertNotIn("ZORP_API_KEY", env)

    def test_the_agent_runs_from_the_filesystem_root(self):
        # The path policy's root is the working directory, and a task may
        # write anywhere in its disposable container.
        agent = self.agent("openai/gemma4:e4b")
        agent.exec_as_agent = AsyncMock()
        asyncio.run(agent.run("do the task", MagicMock(), MagicMock()))
        self.assertEqual(agent.exec_as_agent.call_args.kwargs["cwd"], "/")

    def test_binary_path_follows_the_container_architecture(self):
        with patch.dict(os.environ, {}, clear=True):
            path = self.agent()._host_binary("amd64")
        self.assertEqual(path.parts[-3:], ("harbor", "amd64", "zorp-agent"))

    def test_zorp_agent_bin_overrides_the_built_path(self):
        with patch.dict(os.environ, {"ZORP_AGENT_BIN": "/opt/zorp-agent"}):
            path = self.agent()._host_binary("amd64")
        self.assertEqual(path, Path("/opt/zorp-agent"))


if __name__ == "__main__":
    unittest.main(verbosity=2)
