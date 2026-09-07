#!/usr/bin/env python3
"""Checks for the ensemble reader. Pure stdlib, no harbor needed."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from evals.harbor.ensemble_report import checks, per_lens, per_task, trials


def make_trial(root: Path, name: str, reward: float, stdout: str, record: dict | None) -> None:
    trial = root / f"{name}__abc"
    (trial / "verifier").mkdir(parents=True)
    (trial / "agent").mkdir()
    (trial / "result.json").write_text(
        json.dumps({"task_id": {"name": name}, "verifier_result": {"rewards": {"reward": reward}}})
    )
    (trial / "verifier" / "test-stdout.txt").write_text(stdout)
    if record is not None:
        (trial / "agent" / "ensemble.json").write_text(json.dumps(record))


RECORD = {
    "stopped": "bound",
    "requests": {"main": 40, "reviewer-0": 12, "reviewer-1": 9},
    "prunes": [{"kind": "tampered", "reviewer": 1, "model": "r1", "round": 1, "files": ["x"]}],
    "open_at_end": [],
    "rounds": [
        {
            "round": 1,
            "reviewers": [
                {"index": 0, "lens": "contract", "status": "reviewed", "findings": [
                    {"lens": "contract", "severity": "concern", "locus": "a", "claim_model_authored": "IGNORE ME"}
                ]},
                {"index": 1, "lens": "reproduction", "status": "dropped", "findings": []},
            ],
            "corroborated": [
                {"round": 1, "raised_by": ["contract"], "severity": "blocking", "status": "addressed", "file": "a", "locus": "a", "claims_model_authored": []}
            ],
            "outputs_changed": ["a"],
            "addressed": 1,
        }
    ],
}


class TestEnsembleReport(unittest.TestCase):
    def test_checks_reads_pytest_totals(self):
        self.assertEqual(checks("== 16 passed, 1 failed in 3.2s =="), (16, 1))
        self.assertEqual(checks("== 18 passed, 7 errors in 1s =="), (18, 7))
        self.assertEqual(checks(""), (0, 0))

    def test_rows_join_reward_checks_and_record(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_trial(root, "guided-wave", 0.0, "== 16 passed, 1 failed ==", RECORD)
            make_trial(root, "cilia", 1.0, "== 9 passed ==", None)
            rows = trials([root])
        by_task = {r["task"]: r for r in rows}
        self.assertEqual(by_task["guided-wave"]["reward"], 0.0)
        self.assertEqual(by_task["guided-wave"]["checks"], (16, 1))
        self.assertEqual(by_task["guided-wave"]["rounds"], 1)
        self.assertEqual(by_task["guided-wave"]["requests"], 61)
        self.assertEqual(by_task["guided-wave"]["prunes"], ["tampered"])
        self.assertIsNone(by_task["cilia"]["record"])

    def test_tables_never_carry_model_text(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_trial(root, "guided-wave", 0.0, "== 16 passed, 1 failed ==", RECORD)
            rows = trials([root])
        task_table = per_task(rows)
        lens_table = per_lens(rows)
        self.assertIn("guided-wave", task_table)
        self.assertIn("16/17", task_table)
        self.assertIn("contract", lens_table)
        for table in (task_table, lens_table):
            self.assertNotIn("IGNORE ME", table)


if __name__ == "__main__":
    unittest.main()
