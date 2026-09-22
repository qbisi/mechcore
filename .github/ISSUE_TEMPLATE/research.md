---
name: Research
about: A question the plan chose to pursue, cut to one number or one decision, with its fixtures on master and its recordings published, for one agent to answer without the game
title: ""
labels: "research"
assignees: ""
---

<!--
The Research section of .github/CONTRIBUTING.md states the rules. The short
form:

  Only the keeper opens one.     The session that holds the game records
                                 first, checks every hypothesis is separated
                                 by a recording, and publishes the oracle as
                                 the release oracle/issue-<n>; the layouts and
                                 the script are in the issue, not the repo.
  One number or one decision.    "How is +0.3 composed", not "how do officers
                                 work". Hypotheses have to differ in it.
  Predict before you read.       Every hypothesis names the value it predicts.
  The claimant never runs the    A capture request is how it asks for more.
  game.

The keeper labels it `claimable` once the oracle is published. Claim a
`claimable` issue by opening a draft pull request from a branch
research/<n>-<slug> whose body ends with `Closes #<n>` before its
Co-Authored-By trailer. The draft is the claim.
-->

**Question.** <One number or one decision, and the hypotheses that differ in it.>

**Read.** <What the build says, by class, method and address, as files under
`mechcore-decomp/<build>/cpp2il/`, and what it cannot answer. Where the
keeper's reading stopped.>

**Fixtures.** <Not in the repository yet: the claimant lands the ones the
answer needed, under `tests/<topic>/`, with the regressions. One fenced block
per layout, with the reason it exists, and one for the record script.>

`<topic>/<name>.yaml`:

```yaml
kind: layout
...
```

`<topic>/<script>.mcscript`:

```yaml
game: launch
...
```

**Predicted.**

| Hypothesis | Predicts |
| --- | --- |
| <A> | <value> |
| <B> | <value> |

**Oracle.** Release `oracle/issue-<n>` of this repository, `scripts/oracle.py
fetch <n>`; build <game build>.

| File | Profile | Ticks | Physics hash |
| --- | --- | --- | --- |
| `<topic>/<script>/<name>.mcfr` | — | <n> | `<hash>` |

**Accept.** <What the pull request has to show, beside the standing
conditions: every existing pin unchanged, the topic's offline
`regressions.mcscript` pinning each oracle fight the simulator can run, the
rule in `docs/rules/` with its scope, a refusal for what is outside it.>

**Touches.** <The modules and files the answer may change. Nothing else is
open on them while this is.>

**Not here.** <What this question does not ask, and which question does.>
