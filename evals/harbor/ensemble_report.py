#!/usr/bin/env python3
"""Join ensemble records with harbor rewards and say which lens earned its keep.

    python3 evals/harbor/ensemble_report.py jobs/<job> [jobs/<job> ...]

Reads, per trial: result.json for the task name and reward, the verifier's
stdout for checks passed and failed, and agent/ensemble.json, the record
zorp-agent ensemble wrote. Prints one table per task and one per lens.

The rule this script lives under: it selects on code-derived columns only.
Lens, severity, status, whether a finding was corroborated, whether it was
addressed by a hash change, why a reviewer was pruned, and request counts.
It never reads claim_model_authored, and it does not read locus either,
because a locus is also the reviewer's own words. The roster gets decided
by a person reading these tables, not by this script.
"""

from __future__ import annotations

import json
import re
import sys
from collections import defaultdict
from pathlib import Path

_PASSED = re.compile(r"(\d+) passed")
_FAILED = re.compile(r"(\d+) (?:failed|errors?)")


def checks(test_stdout: str) -> tuple[int, int]:
    """Verifier checks passed and not passed, from pytest's summary line."""
    passed = sum(int(n) for n in _PASSED.findall(test_stdout))
    failed = sum(int(n) for n in _FAILED.findall(test_stdout))
    return passed, failed


def _read_json(path: Path) -> dict | None:
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return None


def trials(job_dirs: list[Path]) -> list[dict]:
    """One row per trial directory found under the given jobs."""
    rows = []
    for job in job_dirs:
        for result_path in sorted(job.glob("*/result.json")):
            trial = result_path.parent
            result = _read_json(result_path) or {}
            task = (result.get("task_id") or {}).get("name") or trial.name.split("__")[0]
            reward = ((result.get("verifier_result") or {}).get("rewards") or {}).get("reward")
            stdout_path = trial / "verifier" / "test-stdout.txt"
            stdout = stdout_path.read_text() if stdout_path.is_file() else ""
            record = _read_json(trial / "agent" / "ensemble.json")
            row = {
                "task": task,
                "trial": trial.name,
                "reward": reward,
                "checks": checks(stdout),
                "record": record,
                "rounds": len(record["rounds"]) if record else 0,
                "requests": sum(record["requests"].values()) if record else 0,
                "prunes": [p["kind"] for p in record["prunes"]] if record else [],
                "stopped": record["stopped"] if record else "",
            }
            rows.append(row)
    return rows


def per_task(rows: list[dict]) -> str:
    lines = ["task | reward | checks | rounds | corroborated | addressed | open | prunes | requests | stopped"]
    for r in sorted(rows, key=lambda r: r["task"]):
        passed, failed = r["checks"]
        rec = r["record"]
        corroborated = sum(len(x["corroborated"]) for x in rec["rounds"]) if rec else 0
        addressed = sum(x["addressed"] for x in rec["rounds"]) if rec else 0
        open_at_end = len(rec["open_at_end"]) if rec else 0
        lines.append(
            f"{r['task']} | {r['reward']} | {passed}/{passed + failed} | {r['rounds']} | "
            f"{corroborated} | {addressed} | {open_at_end} | {','.join(r['prunes']) or '-'} | "
            f"{r['requests']} | {r['stopped'] or '-'}"
        )
    return "\n".join(lines)


def per_lens(rows: list[dict]) -> str:
    """Per lens: findings raised, corroborated, addressed, and in how many
    passing trials each happened. Which lens's findings preceded a pass is
    the question the roster gets decided on."""
    raised: dict[str, int] = defaultdict(int)
    corroborated: dict[str, int] = defaultdict(int)
    addressed: dict[str, int] = defaultdict(int)
    in_passing: dict[str, int] = defaultdict(int)
    reviewed: dict[str, int] = defaultdict(int)
    dropped: dict[str, int] = defaultdict(int)
    for r in rows:
        rec = r["record"]
        if not rec:
            continue
        passing = (r["reward"] or 0) > 0
        for rnd in rec["rounds"]:
            for rv in rnd["reviewers"]:
                lens = rv["lens"]
                raised[lens] += len(rv["findings"])
                if rv["status"] == "reviewed":
                    reviewed[lens] += 1
                if rv["status"] == "dropped":
                    dropped[lens] += 1
            for f in rnd["corroborated"]:
                for lens in f["raised_by"]:
                    corroborated[lens] += 1
                    if f["status"] == "addressed":
                        addressed[lens] += 1
                        if passing:
                            in_passing[lens] += 1
    lenses = sorted(set(raised) | set(reviewed) | set(dropped))
    lines = ["lens | reviews | dropped | raised | corroborated | addressed | addressed in passing trials"]
    for lens in lenses:
        lines.append(
            f"{lens} | {reviewed[lens]} | {dropped[lens]} | {raised[lens]} | "
            f"{corroborated[lens]} | {addressed[lens]} | {in_passing[lens]}"
        )
    return "\n".join(lines)


def main(argv: list[str]) -> int:
    if not argv:
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 2
    rows = trials([Path(a) for a in argv])
    if not rows:
        print("no trials found", file=sys.stderr)
        return 1
    print(per_task(rows))
    print()
    print(per_lens(rows))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
