---
status: accepted
date: 2026-08-23
---

# the calibration sample is nested, so a bigger run extends the smaller one

**Decision:** `evidence_calibration` fixes an order over the eligible
directories that does not depend on how many it wants, and takes the
first `ZORP_CAL_N` of it. The order is by a keyed hash of each
directory's path, seeded by `ZORP_CAL_SEED`, default 0. The `corpus:`
line now prints the root, the eligible count, a digest of the eligible
paths, the seed and how many were taken, which is everything needed to
run the same sample again. Nesting and determinism are tests, not
comments, and both fail against the sampler they replaced.

**Why: two published calibration numbers from this harness were not
comparable, and nobody could have told from the output.** The old
sampler took every kth eligible directory with k chosen so the stride
landed on n, so k moved when n did. Two real runs printed:

```
corpus: 3457 eligible directories, sampling every 17th for 200
corpus: 3457 eligible directories, sampling every 13th for 250
```

Of the 76 directories scored in the first and the 33 scored in the
second, exactly 1 was shared. The first reported a calibration gap of
0.107 and the second 0.000, and none of that difference can be
attributed to the model, because the two runs asked almost entirely
different questions. Raising n did not extend the sample, it replaced
it, so evidence could not accumulate and the reported number was partly
a property of the stride. That is precisely the confusion this
subsystem exists to prevent.

**What it rules out.** A plain sorted order is deterministic and nested
and still wrong: sorted paths group by crate name and therefore by
ecosystem and vintage, so the first n are one letter's worth of crates.
A seeded shuffle of the list gives an unclumped fixed order too, and was
rejected for what happens when the tree grows: a shuffle is a property
of the whole list, so one new crate in `~/.cargo/registry/src` reorders
everything and no earlier run can be extended. A key per path is a
property of that path, so a new directory takes its own place and leaves
every other pair in the same relative order. Reaching for a hash from a
crate, or for `DefaultHasher`, is ruled out for the same reason the seed
is printed: the number chooses the sample, so it has to be the same in
next year's build, and neither of those promises that.

**What is honestly not fixed.** A run reproduces against the same
corpus and no other. A directory added to the registry can still land
inside the prefix and push the last one out, which is why the count and
the digest are on the `corpus:` line: two runs over different corpora
are visibly different rather than silently so. Nothing about what is
measured changed, only which directories are chosen: the prompt, the
scoring, the discard categories and the calibration report are
untouched. `ollama_calibration` samples by truncation, which is
alphabetically biased but already nested, and is left alone.
