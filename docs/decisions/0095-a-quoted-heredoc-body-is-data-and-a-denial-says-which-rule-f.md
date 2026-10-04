---
status: accepted
date: 2026-09-03
---

# a quoted heredoc body is data, and a denial says which rule fired

**Finding:** a 21-task Terminal-Bench run denied 18 `run_command` calls.
15 were Python heredocs with an apostrophe in a comment. The policy parsed
the body as shell words, saw an unclosed quote, and failed closed. The other
three were redirects to `/tmp`, which remain correctly denied.

**Decision:** a quoted or escaped heredoc delimiter makes its body literal
data, so the policy skips it. A bare delimiter permits shell expansion, so
the policy still scans its body for substitutions and backticks. A body
handed to `sh`, `bash`, `zsh`, `eval` or `source` is a script whatever its
delimiter looks like, so it gets the same reading as an `sh -c` payload.
The own-server check still reads the whole command, bodies included,
because a URL inside a Python heredoc is a request once Python runs it.
Each denial
now names the rule that fired, so a model can correct the command instead of
retrying the same shape.
