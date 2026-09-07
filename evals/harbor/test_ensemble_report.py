#!/usr/bin/env python3
"""Checks for the ensemble reader. Pure stdlib, no harbor needed."""

from __future__ import annotations

import contextlib
import io
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


# A locus text no code path would ever emit, standing in for a reviewer's
# own words the way "IGNORE ME" stands in for a raw claim. Both must be
# absent from every table: a locus is model-authored text exactly as a
# claim is, and the brief names it as the likelier place to leak one in.
LEAKY_LOCUS = "the marginal utility asymptotically approaches zero past round two"

# A real ledger never marks a corroborated finding "addressed" inside
# rounds[].corroborated: Ledger::finding_for sets Status::Open
# unconditionally, and a round's corroborated list is cloned before
# settle() runs. So this fixture uses "open", the only status the
# serializer can actually write there, and carries the addressed and
# newly-corroborated counts on the two per-lens maps instead, matching
# what zorp-agent/src/ensemble/record.rs now emits.
RECORD = {
    "stopped": "bound",
    "requests": {"main": 40, "reviewer-0": 12, "reviewer-1": 9, "reviewer-2": 4},
    "prunes": [{"kind": "tampered", "reviewer": 1, "model": "r1", "round": 1, "files": ["x"]}],
    "open_at_end": [],
    "rounds": [
        {
            "round": 1,
            "reviewers": [
                {"index": 0, "lens": "contract", "status": "reviewed", "findings": [
                    {"lens": "contract", "severity": "concern", "locus": LEAKY_LOCUS, "claim_model_authored": "IGNORE ME"}
                ]},
                {"index": 1, "lens": "reproduction", "status": "dropped", "findings": []},
                # A reused verdict still did work: it raised a finding, it
                # just did not re-run the reviewer to get it. The old table
                # only counted "reviewed" and "dropped", so this lens used
                # to show up with every column at zero.
                {"index": 2, "lens": "adversary", "status": "reused", "findings": [
                    {"lens": "adversary", "severity": "note", "locus": "b", "claim_model_authored": "looks fine"}
                ]},
            ],
            "corroborated": [
                {
                    "round": 1,
                    "raised_by": ["contract", "adversary"],
                    "severity": "blocking",
                    "status": "open",
                    "file": "a",
                    "locus": LEAKY_LOCUS,
                    "claims_model_authored": [],
                }
            ],
            "outputs_changed": ["a"],
            "addressed": 1,
            "newly_corroborated_by_lens": {"contract": 1, "adversary": 1},
            "addressed_by_lens": {"contract": 1},
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
        self.assertEqual(by_task["guided-wave"]["requests"], 65)
        self.assertEqual(by_task["guided-wave"]["prunes"], ["tampered"])
        self.assertIsNone(by_task["cilia"]["record"])

    def test_per_task_sums_the_per_lens_maps_not_the_raw_list(self):
        # Two lenses corroborated the one locus, so per-lens credit sums to
        # 2, not len(rounds[].corroborated), which is 1. Summing the raw
        # list is also what double-counts a locus still open in a later
        # round; this fixture only has one round, but the same sum call
        # is what protects the multi-round case too.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_trial(root, "guided-wave", 0.0, "== 16 passed, 1 failed ==", RECORD)
            rows = trials([root])
        task_table = per_task(rows)
        self.assertIn(
            "guided-wave | 0.0 | 16/17 | 1 | 2 | 1 | 0 | tampered | 65 | bound",
            task_table,
        )

    def test_per_lens_counts_every_status_and_the_per_lens_maps(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_trial(root, "guided-wave", 0.0, "== 16 passed, 1 failed ==", RECORD)
            rows = trials([root])
        lens_table = per_lens(rows)
        # header: lens | reviewed | reused | unusable | dropped | skipped | raised | corroborated | addressed | addressed in passing trials
        self.assertIn("contract | 1 | 0 | 0 | 0 | 0 | 1 | 1 | 1 | 0", lens_table)
        self.assertIn("reproduction | 0 | 0 | 0 | 1 | 0 | 0 | 0 | 0 | 0", lens_table)
        # adversary only ever reused a verdict. It still raised a finding
        # and still got corroboration credit; none of that is a "review".
        self.assertIn("adversary | 0 | 1 | 0 | 0 | 0 | 1 | 1 | 0 | 0", lens_table)

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
            self.assertNotIn(LEAKY_LOCUS, table)

    def test_corrupt_record_warns_and_reads_as_absent(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_trial(root, "broken", 0.0, "== 1 passed ==", None)
            (root / "broken__abc" / "agent" / "ensemble.json").write_text("{not json")
            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                rows = trials([root])
        self.assertIsNone(rows[0]["record"])
        self.assertIn("broken", stderr.getvalue())

    def test_a_trial_with_no_ensemble_json_warns_of_nothing(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_trial(root, "clean", 1.0, "== 1 passed ==", None)
            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                trials([root])
        self.assertEqual(stderr.getvalue(), "")


if __name__ == "__main__":
    unittest.main()
