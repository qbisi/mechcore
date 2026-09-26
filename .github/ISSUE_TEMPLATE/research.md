---
name: Research
about: A question the plan chose to pursue, cut to one number or one decision, with fixtures anyone with the game can record
title: ""
labels: "research"
assignees: ""
---

<!--
The Research section of .github/CONTRIBUTING.md states the rules. The short
form:

  One number or one decision.    "How is +0.3 composed", not "how do officers
                                 work". Hypotheses have to differ in it.
  Predict before you read.       Every hypothesis names the value it predicts.
  Reproducible, not uploaded.    The fixtures are a layout, a battle or a
                                 replay (.grbr) and the script that records
                                 them, inline or in the repository. Nothing
                                 is published: whoever has the game records
                                 them again.
  Say what, not how.             Accept names what has to be reproduced and
                                 pinned, never the module or table an answer
                                 must use; that is decided from the build.
  Verified where the game runs.  A recording, and every hash pinned from it,
                                 is made on a machine with the game.
-->

**Question.** <One number or one decision, and the hypotheses that differ in it.>

**Read.** <What the build says, by class and method, and what it cannot
answer. Where the reading stopped.>

**Fixtures.** <What reproduces the fights the hypotheses differ in: a layout,
a battle or a native replay, with the reason each exists, and the script that
records them. Inline until the answer lands the ones it needed under
`tests/<topic>/`; a replay already in the corpus is named by its file.>

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

**Reproduce.** <On a machine with the game, game version <version>: the
command that records the fixtures, and what each recording shows, with its
tick count and physics hash when it was recorded before opening.>

```
mechcore run <topic>/<script>.mcscript
```

**Accept.** <What the pull request has to show, as observations, beside the standing
conditions: every existing pin unchanged, the topic's offline
`regressions.mcscript` pinning each fight the simulator can run, recorded on a
machine with the game, the rule in `docs/rules/` with its scope, a refusal for
what is outside it.>

**Touches.** <The modules and files expected to change, which is what places
this question against the open ones. A forecast, not a fence.>

**Not here.** <What this question does not ask, and which question does.>
