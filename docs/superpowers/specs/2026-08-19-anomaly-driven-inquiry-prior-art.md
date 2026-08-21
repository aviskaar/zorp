# Prior art pass: anomaly-driven inquiry

**Date:** 2026-08-19
**Subject:** [`2026-08-19-anomaly-driven-inquiry-design.md`](2026-08-19-anomaly-driven-inquiry-design.md)
**Verdict:** the novelty claim as written does not survive. A narrower one does.

## What this pass did

The design's prior art section was written from memory and said so. This
checks it. Every citation below was looked up rather than recalled, and
the searches also went looking for work the design did not cite, which is
where the useful findings came from.

Limits worth stating. This is a search pass, not a literature review. It
reads abstracts and paper pages, not full texts. It covers arXiv and the
open web, not paywalled venues or anything unindexed. Absence of a result
here is weak evidence of absence in the field.

## Citations: all correct

Nine attributions, none wrong.

| Claim in the design | Verdict |
|---|---|
| Schmidhuber, compression progress as curiosity | correct |
| Pathak et al. 2017, Intrinsic Curiosity Module | correct |
| Burda et al. 2018, Random Network Distillation (arXiv 1810.12894) | correct |
| Lindley 1956, "On a Measure of the Information Provided by an Experiment" | correct, and it is the origin of expected information gain as a utility |
| Chamberlin 1890, method of multiple working hypotheses | correct, in *Science*. The widely circulated PDF is the 1897 reprint, so cite the year you mean |
| Platt 1964, strong inference | correct, and Platt himself builds on Chamberlin, so grouping them is right |
| King et al., Robot Scientist | correct, *Nature* 2004, and Adam 2009 |
| Langley, BACON | correct, law rediscovery from data |
| Google AI Co-Scientist, Sakana AI Scientist | correct as examples of the current wave |

The noisy TV problem is correctly attributed to this literature, and the
design's account of it is accurate.

## Three findings that change the design

### 1. The architecture is published

**AutoDiscovery: Open-ended Scientific Discovery via Bayesian Surprise**
(arXiv 2507.00310) is the design's core loop, already built. It selects
which hypotheses to test without a human-specified research question. It
elicits the LLM's prior beliefs, gathers results, elicits posteriors, and
uses the shift between them as surprisal. Surprisal is the reward in a
Monte Carlo tree search over nested hypotheses. Two thirds of its
discoveries were rated surprising by domain experts.

That is observe, predict, compare, be surprised, pursue. The design's
diagram, minus the record.

What AutoDiscovery does **not** have, checked against the paper page:

- no pre-registration, so nothing stops a belief being stated after the
  result is known
- no calibration measurement, so the surprisal number is never validated
  against whether the beliefs were any good
- no re-run or reproducibility gate, so nothing separates a phenomenon
  from a defect

Those three are §2, §6 and §4 of the design. So the surviving
contribution is not the loop. It is the epistemic record underneath it.

### 2. The calibration question is largely answered, and the answer is bad

The design says the §6 measurement "appears unanswered". Two 2026 papers
say otherwise for the adjacent case.

**FermiEval** (arXiv 2510.26995), "LLMs are Overconfident: Evaluating
Confidence Interval Calibration". Nominal 99% intervals cover the true
answer **65%** of the time. The authors offer a "perception tunnel"
account: models sample from a truncated region of their own inferred
distribution and neglect the tails. Conformal prediction repairs coverage
after the fact.

**QuantSightBench** (arXiv 2604.15859) evaluates LLM numeric prediction
intervals against realized outcomes and reports empirical coverage
against stated confidence, which is precisely §6's quantity.

Both measure static or external questions. QuantSightBench states
explicitly that its tasks are "not time-series tasks or self-conducted
experiments". Neither has an agent predicting the outcome of an
experiment it chose and is about to run.

So the residual gap is real but thin: the same measurement moved to the
endogenous case. And a 65% coverage figure at a nominal 99% is a strong
prior on what the answer will be. The design's own stop sign at step 3
now looks more likely to fire than not, which is worth knowing before
building steps 1 and 2 rather than after.

The endogenous case is not merely a domain transfer though, for the
reason in the next finding.

### 3. Open question 5 has a name, and a literature

The design lists as unsolved: the agent writes its own predictions and
picks its own actions, so it can move its surprise rate at will.

In intrinsic-motivation RL this is the **action-dependent noisy TV**, the
agent handed a remote control to the noise source. It is a known failure
mode with a known mitigation family: separate reducible uncertainty from
irreducible. Representative work includes aleatoric uncertainty
estimation for curiosity (arXiv 2102.04399) and learning-progress
monitoring (arXiv 2509.25438). RND's trick of predicting features of the
current state rather than the next is in the same family.

This matters twice.

The design's §4 re-run gate, which admits an anomaly only if it
reproduces and rejects it if it is transient or volatile, **is** an
empirical aleatoric-versus-epistemic separator. It arrived at the right
family independently. It should cite that family rather than present
itself as ad hoc, and it should count its rejections, which it already
plans to.

And it means the endogenous calibration question is not just FermiEval
moved sideways. In the endogenous case the quantity being measured is
also manipulable by the thing being measured. That is a genuinely
different problem from a static benchmark, and it is the sharpest thing
in the design.

## Two more that need citing, not fixing

**Agentic AI Scientists Are Not Built For Autonomous Scientific
Discovery** (arXiv 2605.08956) is the 2026 critique the design responds
to. Its recommendations include a public preregistration repository for
AI-generated hypotheses before experimentation, and persistent world
models holding mutable epistemic state across investigations. The design
is building two things this paper asks for. That is good positioning and
bad novelty: the idea is in print, the implementation is not.

**Preregistration for Experiments with AI Agents** (arXiv 2606.11217) is
preregistration *of experiments on* agents, not an agent registering its
own predictions. Different object. Worth reading anyway, because its
integrity mechanism is an attestation section where a human confirms data
collection has not begun. The design's integrity rule refuses the insert
in code when an outcome already exists. That is the same idea enforced
rather than asserted, and it is a defensible distinction to draw
explicitly.

**Habituation.** The boredom detectors have a lineage: habituation-based
novelty detection in robotics, e.g. "Novelty Detection on a Mobile Robot
Using Habituation" (arXiv cs/0006007). That work habituates to sensory
input. The design habituates to its own research process, asking what a
line of investigation has stopped varying. Same mechanism, different
object. Not a collision, but not unprecedented either.

## Revised position

The design's current sentence is that the architecture is a
recombination and the defensible contribution is the §6 measurement.
After this pass, both halves need work.

The architecture is not a recombination. It is substantially
AutoDiscovery, which the design does not cite because it did not know
about it. That has to be cited and distinguished.

The §6 measurement is not unanswered. It is answered next door, with a
result that predicts failure, and the untouched part is the endogenous
case specifically.

What survives is narrower and, in my view, more interesting than what was
claimed:

> Prediction-error curiosity has been moved from RL agents to LLM agents
> doing open-ended discovery, and the move is being made without the
> guardrails the RL literature spent a decade building. AutoDiscovery
> uses elicited belief shift as a reward with no calibration check and no
> reproducibility gate. Nobody has measured whether an LLM's
> pre-registered interval about an experiment it is about to run is
> calibrated well enough to carry that weight, and the closest evidence,
> 65% coverage at a nominal 99%, says probably not. The endogenous case
> is also the action-dependent noisy TV, where the agent can move the
> signal it is rewarded on.

That is a paper about whether the current wave has a measurement problem,
supported by an artifact that measures it. The negative result stays
publishable, and it now has a named target to be negative about.

## What to do

1. Cite AutoDiscovery and distinguish against it in §1. This is the big
   one and it is not optional.
2. Rewrite the prior art section using the verified list above, and add
   FermiEval, QuantSightBench, 2605.08956, 2606.11217, cs/0006007.
3. Reframe §6 as the endogenous case, and say plainly what the adjacent
   evidence predicts. Do not claim the question is untouched.
4. Rewrite open question 5 as the action-dependent noisy TV, cite the
   mitigation family, and connect it to §4, which is already a member of
   that family.
5. Keep the step 3 stop sign. It is now the most valuable thing in the
   implementation order, because the prior evidence says it will probably
   fire, and firing early is cheap.

Nothing here changes the build order or kills the design. It changes what
the design can claim, which is what the pass was for.
