# The four capabilities

zorp's research work is four capabilities, each a clearly bounded layer
on top of the track foundation, all behind `zorp-agent`'s `research`
feature.

## validate

Is this question worth investigating? A novelty and feasibility check
that searches for existing evidence before scoring the question.

It requires a search-capable tool: one whose name carries a search verb
(search, fetch, query, browse, find, lookup, retrieve), connected over
MCP or provided by the built-in `web_search` tool behind the `search`
feature. Without one it fails fast rather than scoring a question
against nothing. A tool that searches your own saved notes deliberately
does not count.

## investigate

Gather evidence through staged, pre-registered attempts. Every attempt
is recorded, and every attempt records the conditions it ran under.
With `ZORP_FORECAST` set, the agent is asked for a forecast before each
attempt runs, and that is recorded too. Both happen before the attempt,
which is not a detail: a condition recorded afterwards describes a
different run, and an expectation recorded afterwards is a postdiction.

`investigate` is the only thing that writes to the
[aryabhatta ledger](aryabhatta.md).

## co-write

zorp drafts the artifact from the track's recorded evidence. A human is
always the author of record. Between co-write and deliver sits
[critique](critique.md), a gate that audits the draft against the
evidence record.

## deliver

Match a finished draft against real venues (conferences and journals,
via live huiban search) and write a ranked shortlist for a human to
review. It requires a huiban-prefixed MCP tool and fails fast without
one.

## What is not a capability

[aryabhatta](aryabhatta.md) is a record plus readers, not a fifth
capability. [critique](critique.md) is a gate. [panel](panel.md) is a
reader. Each one is documented on its own page, and each one is smaller
than a capability on purpose.
