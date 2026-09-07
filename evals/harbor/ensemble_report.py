#!/usr/bin/env python3
"""Join ensemble records with harbor rewards and say which lens earned its keep.

    python3 evals/harbor/ensemble_report.py jobs/<job> [jobs/<job> ...]

Reads, per trial: result.json for the task name and reward, the verifier's
stdout for checks passed and failed, and agent/ensemble.json, the record
zorp-agent ensemble wrote. Prints one table per task and one per lens.

The rule this script lives under: it selects on code-derived columns only.
Lens, status, whether a finding was corroborated, whether it was addressed
by a hash change, why a reviewer was pruned, and request counts. It never
reads claim_model_authored, and it does not read locus either, because a
locus is also the reviewer's own words. The roster gets decided by a
person reading these tables, not by this script.
"""

from __future__ import annotations

import json
import re
import sys
from collections import defaultdict
from pathlib import Path

_PASSED = re.compile(r"(\d+) passed")
_FAILED = re.compile(r"(\d+) (?:failed|errors?)")

# Every status a reviewer can carry, in the order record.rs documents them.
_STATUSES = ("reviewed", "reused", "unusable", "dropped", "skipped")


def checks(test_stdout: str) -> tuple[int, int]:
    """Verifier checks passed and not passed, from pytest's summary line."""
    passed = sum(int(n) for n in _PASSED.findall(test_stdout))
    failed = sum(int(n) for n in _FAILED.findall(test_stdout))
    return passed, failed


def _read_json(path: Path) -> dict | None:
    """Read a JSON file. A trial that never wrote one returns None quietly;
    a file that exists but fails to parse also returns None, but says so
    on stderr first, so the two cases do not look the same to a reader."""
    if not path.is_file():
        return None
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError) as e:
        print(f"warning: {path}: unreadable, treating as no record ({e})", file=sys.stderr)
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
        # newly_corroborated and addressed are counts of findings, not of
        # lens credits: summing the per-lens maps instead would double a
        # finding two lenses raised. Not len(rounds[].corroborated) either,
        # since that list is re-derived every round, so a locus still open
        # from an earlier round would otherwise be counted again in every
        # round it survives.
        corroborated = sum(x["newly_corroborated"] for x in rec["rounds"]) if rec else 0
        addressed = sum(x["addressed"] for x in rec["rounds"]) if rec else 0
        open_at_end = len(rec["open_at_end"]) if rec else 0
        lines.append(
            f"{r['task']} | {r['reward']} | {passed}/{passed + failed} | {r['rounds']} | "
            f"{corroborated} | {addressed} | {open_at_end} | {','.join(r['prunes']) or '-'} | "
            f"{r['requests']} | {r['stopped'] or '-'}"
        )
    return "\n".join(lines)


def per_lens(rows: list[dict]) -> str:
    """Per lens: what each reviewer status accounts for, findings raised,
    newly corroborated, and addressed, and in how many passing trials the
    addressed credit landed. Which lens's findings preceded a pass is the
    question the roster gets decided on.

    Every one of a reviewer's five statuses is counted, not just reviewed
    and dropped: a lens that only ever reused a prior verdict, or came
    back unusable, or was skipped when a run was cancelled, still did
    something, and a table that shows it doing nothing is wrong."""
    raised: dict[str, int] = defaultdict(int)
    corroborated: dict[str, int] = defaultdict(int)
    addressed: dict[str, int] = defaultdict(int)
    in_passing: dict[str, int] = defaultdict(int)
    status_counts: dict[str, dict[str, int]] = defaultdict(lambda: defaultdict(int))
    for r in rows:
        rec = r["record"]
        if not rec:
            continue
        passing = (r["reward"] or 0) > 0
        for rnd in rec["rounds"]:
            for rv in rnd["reviewers"]:
                lens = rv["lens"]
                raised[lens] += len(rv["findings"])
                status_counts[lens][rv["status"]] += 1
            for lens, n in rnd.get("newly_corroborated_by_lens", {}).items():
                corroborated[lens] += n
            for lens, n in rnd.get("addressed_by_lens", {}).items():
                addressed[lens] += n
                if passing:
                    in_passing[lens] += n
    lenses = sorted(set(raised) | set(status_counts) | set(corroborated) | set(addressed))
    lines = ["lens | " + " | ".join(_STATUSES) + " | raised | corroborated | addressed | addressed in passing trials"]
    for lens in lenses:
        counts = status_counts[lens]
        lines.append(
            f"{lens} | " + " | ".join(str(counts[s]) for s in _STATUSES) +
            f" | {raised[lens]} | {corroborated[lens]} | {addressed[lens]} | {in_passing[lens]}"
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
