# Research

A research directory is one question, pursued deliberately, with the working
data kept. Scenario transcripts, decompilation addresses, provisional
hypotheses, replay hashes, first-divergence ticks and review outcomes all live
here and nowhere else.

Name it `<question>-<YYYYMMDD>`. The target build goes inside the directory
rather than in its name, because a question outlives a build and the record has
to say which build it answered.

Reproducing a replay is not proving a mechanism. This is the one sentence the
rest of the file exists to enforce. Byte-identical trajectories over several
independent replays support the main path they traverse, strongly. They say
nothing about branches nothing took, and on their own they cannot fix a single
number.

## Two gates

A mechanism and a number are held to different standards, and the difference is
deliberate.

**A mechanism** closes on the target build's decompiled code: a branch, a state
transition, a formula, a call chain. Some mechanisms branch too widely to close
exhaustively, and for those a risk-adjusted conclusion is allowed, but only with
all four of: a structure that is self-consistent, several mutually independent
native trajectories that match tick for tick inside a declared scope, no known
counterexample and no unexplained first divergence inside that scope, and a
written record of the unverified assumptions.

**A number** is any constant that reaches the code: a radius, a threshold, an
interval, a priority, a time horizon. Every one must trace to exactly one of
three sources.

1. The target build's decompiled code.
2. The target build's extracted resources or serialized config.
3. Direct runtime observation of a native field through the adapter.

Record the source, what it applies to, the units, and any precision conversion.
A number without one of the three is a hypothesis. It may not enter the real
code path and may not be written down as a fact about the game.

Four things look like evidence for a number and are not: a trajectory that fits,
an outcome that matches, an older project's config, and whatever value makes the
implementation convenient. The number gate never relaxes because the mechanism
around it was allowed a risk-adjusted conclusion.

## Status and confidence are separate axes

Recording them as one value is how a guess becomes a fact by accident.

| Axis | Values |
| --- | --- |
| `implementation_status` | `integrated`, `experimental`, `rejected` |
| `confidence` | `proven`, `strongly_supported`, `provisional`, `speculative` |

`integrated` says the code reached the real path. It does not say the mechanism
is understood. `proven` needs direct mechanism evidence plus independent
verification inside the declared scope. `strongly_supported` means the main path
has strong corpus backing while internal or boundary branches remain open, and
it is meaningless unless it names a build, a scope and the corpus that supports
it. Outside that scope the claim is simply unverified.

`provisional` and `speculative` stay in `experimental` code or here. `rejected`
never reaches the real path.

## The minimal record

Every question that passes review leaves at least this behind:

- the target game build;
- `implementation_status`, `confidence`, and the declared scope;
- the decompiled addresses or stable identifiers obtained, the call chain, the
  control flow that matters, and which part is explicitly not closed;
- for each number, its source, the arithmetic, the units and the precision
  conversion;
- the replay scenario, seed, MCFR hash and first-divergence result used to
  verify the implementation;
- the uncovered branches, the unverified assumptions, and a `reopen_when` that
  can actually be decided.

## Closing, and reopening

A review that passed is finished. Branches still uncovered are not a reason to
keep going, and re-researching a closed question because it feels incomplete is
the most expensive habit available here.

It reopens on an event, not on a feeling: a recorded `reopen_when` fires, a
counterexample turns up, the target build changes, or a new task widens the
declared scope.

## Leaving

A finding that reaches `proven`, or `strongly_supported` under the
risk-adjusted gate, is promoted out of this directory into `docs/rules/`. It
carries its build, scope, evidence class, unverified boundary and `reopen_when`.
Numbers travel only after passing the number gate on their own.

What stays behind is the process: the scenario, the discarded hypotheses, the
hashes, the scratch decompilation paths. A rules document states what the game
does. This directory remembers how anyone found out.

Something found by accident rather than pursued is not research. It becomes a
GitHub issue, and [CONTRIBUTING.md](../../.github/CONTRIBUTING.md) says what
qualifies.

A question published as an issue keeps its record elsewhere: the issue and
the pull request that answers it hold the hypotheses, the first divergences
and the ticks that decided, because an agent's worktree dies with its
session. The same file's Research section says how such a question
is opened, answered and accepted. The gates above apply to it unchanged.

## Tracking

This README is tracked. The research is not.

A tracked document must therefore never cite a research directory, because the
reader who follows the citation finds nothing. Promotion is what makes a finding
citable.

A commit message is the exception, and may name the directory its evidence sat
in. A commit records a moment rather than promising that the moment is still
there.
