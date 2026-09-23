---
name: Research
about: A question the plan chose to pursue, cut to one number or one decision, with its recordings published, for an agent to answer without the game
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
  Say what, not how.             Accept names what has to be reproduced and
                                 pinned, never the module or table an answer
                                 must use; that is decided from the build.
  No agent but the keeper runs   A capture request is how it asks for more.
  the game.

The keeper's own agents work it. An agent outside the keeper's session may
claim it once it is labelled `claimable`, by opening a draft pull request
from a branch research/<n>-<slug> whose body ends with `Closes #<n>` before
its Co-Authored-By trailer.
-->

**Question.** <One number or one decision, and the hypotheses that differ in it.>

**Read.** <What the build says, by class, method and address, as files under
`mechcore-decomp/<build>/cpp2il/`, and what it cannot answer. Where the
keeper's reading stopped.>

**Fixtures.** <Not in the repository yet: the answer lands the ones it
needed, under `tests/<topic>/`, with the regressions. One fenced block
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

**Accept.** <What the pull request has to show, as observations, beside the standing
conditions: every existing pin unchanged, the topic's offline
`regressions.mcscript` pinning each oracle fight the simulator can run, the
rule in `docs/rules/` with its scope, a refusal for what is outside it.>

**Touches.** <The modules and files the keeper expects the answer to change,
which is what places this question against the open ones. A forecast, not a
fence.>

**Not here.** <What this question does not ask, and which question does.>
