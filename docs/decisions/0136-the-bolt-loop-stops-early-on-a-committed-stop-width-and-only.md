---
status: accepted
date: 2026-10-03
---

# the bolt loop stops early on a committed stop width, and only from three attempts up

**Decision:** a pre-registration may carry one optional `stop_width`, in
the metric's units. When one is committed, the bolt loop ends once the
spread of this press's attempt values, max minus min, is inside it, with
at least three attempts recorded. With none committed, the loop runs a
fixed budget exactly as it does today. Answers #145. This entry is the
decision; no code lands with it.

**Why a width and not a pass mark.** The 2026-09-01 bolt entry ruled out
a success threshold, and that still holds. Stopping on the first attempt
that crosses a pass mark ends the loop on its luckiest draw, which is
optional stopping at its worst. A width is a statement about how much
the attempts agree, not about whether the answer is the one somebody
wanted, so it does not hand anyone a pass mark. It is also one number
rather than two, so it does not bring back the burden `prereg_infer`
took away (2026-09-01).

**Committed with the rest of the pre-registration.** The width is a
`Stop width: <n>` line in `prereg.md`, written only when it is set, so
the SHA-256 and the git commit cover it the way they cover direction
(2026-08-14). A file without the line parses to no width, and its bytes
and hash do not change. `preregistrations` gets a nullable column.
`investigate::run` treats a different width on a later attempt as a
mismatch, like any other pre-registered field.

**A model may propose it, a person commits it.** `prereg_infer` may
propose a width under the same `MIN_CONFIDENCE` floor, and the notice
before the run shows it. A model that is unsure leaves the width out
rather than escalating to the form, because no width is the safe state:
it is today's behavior.

**Enforced in code.** A pure function in `zorp-track`,
`stopping::evaluate(width, values)`, decides. It has no model and does
not read the checkpoint mode, so auto-approve cannot change the answer,
the same split `rerun` and the kill threshold use. It refuses NaN and
widths that are not positive. In the loop the order is kill, then
convergence, then stop-after. A breach always wins.

**Only this press's primary attempts count.** Re-run gate repeats exist
only after an outcome surprised its own forecast, so counting them would
select on the outcome. They are stored as ordinary `experiments` rows,
so the loop collects its own values rather than reading the track back.

**The floor is three, fixed in code.** It cannot be committed or
proposed. With two samples, a width of one standard deviation is met by
chance about half the time. Three is still weak, but it is not a coin
flip.

**The ceiling does not rise on its own.** `ZORP_BOLT_ATTEMPTS`, still
three by default and capped at ten, is the ceiling. So at the default
the rule can never fire. That is deliberate: what this buys is that
somebody who sets eight pays for three or four when the attempts agree,
and raising the budget stays an explicit choice rather than a side
effect of committing a width.

**The record says why it stopped.** Convergence writes a `checkpoints`
row with status `enforced-stop` and kind `investigate-converged`, beside
`enforced-kill`. Its text is composed in code ("spread 0.3 across
attempts 1 to 4, inside committed width 0.5"), never by a model.
`InvestigateDone` carries a stop reason from a closed set, `budget`,
`converged`, `stop-after` or `killed`, and the page picks the words.
Without it, a converged run and an abandoned one look the same in the
ledger.

**What the write-up must not say.** Stopping when samples happen to
cluster makes the recorded spread look smaller than it is. The write-up
says the run stopped because the spread fell inside the committed width
at n attempts, and never that the result has that precision.

**Ruled out:** a success threshold (above, and 2026-09-01); a second
threshold opposite the kill mark, for the same reason; counting gate
repeats toward the spread; a floor below three; raising the default
ceiling when a width is committed; and the model choosing a width
without a person seeing it before the run.
