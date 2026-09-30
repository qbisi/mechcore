# Design

The project is in agile development: a refactor does not keep backward
compatibility and does not keep old code around.

# Directory rules

Before changing or creating a file, find the nearest `README.md` walking up
from its directory and read it; only when none exists up to the root is there
no rule. Readmes here are always upper case. They state what the directory
admits, which the files themselves do not show and which often forbids exactly
what an agent would do by default. A readme outranks your own judgment: follow
it, or say why it should change, but do not route around it. A change that
makes the nearest readme untrue updates the readme in the same pull request.

Issues are GitHub issues, not files. [`.github/CONTRIBUTING.md`](.github/CONTRIBUTING.md)
says what qualifies as one; read it before opening one.

# Plan

The plan is a task graph, kept in Chinese. Its structure is in
`plan/README.md`: the goal, the metric of how far off it is, the lanes (one per
sub-goal), their order, the edges between them, and what a lane file holds.
Each lane's stack and parking lot are in `plan/<lane>.md`.

- **Three kinds of edge.** blocks: B cannot finish before A, and only this
  kind forms a stack. conflicts: both change the same module or both need
  the game, so they do not run at once, in either order. enables: A makes B
  cheaper, and only affects order.
- **Depth-first along blocks within a lane.** A discovery is pushed on the
  stack only when it blocks the current node; when it is done, return to the
  parent explicitly or write why the parent is void. One that does not block
  goes to the parking lot with a decidable `reopen_when`. A bottleneck with no
  blocks or conflicts edge to the lane is another lane.
- **Lanes are reordered only after a merge**, by which lane the metric is
  stuck on and by what the enables edges point at. A lane is not switched
  mid-node; a stopped lane stops on a merged node with its stack intact.
- **State is derived, not written.** A node in progress is its open pull
  request, a finished one is its merge commit, numbers live in pull request
  bodies and `tests/<topic>/README.md`, and a block is `Blocked by #n`.
  Neither `plan/README.md` nor a lane file says how far along anything is.
- **One writer for structure.** Edges and lane order are changed by the main
  session after a merge. A parallel session that finds a cross-lane relation
  writes it in its own pull request body. A lane file is changed only by that
  lane's pull requests. Two pull requests that conflict on the plan reveal a
  missing conflicts edge: serialize them, do not auto-resolve.

# Building and CI

The Adapter is `mechcore`'s default feature `adapter` and the only macOS code.
Everything else builds and tests on any platform with `--no-default-features`,
which is how CI's Linux jobs build; code that only holds on macOS goes behind
`cfg(target_os = "macos")`. What each CI job checks is in the comments of
`.github/workflows/ci.yml`.

Run `cargo fmt --all` before committing; CI checks it. Install the pre-commit
hook once per clone, as a repository setting because a global
`core.hooksPath` would override `.git/hooks`:

```
git config core.hooksPath .githooks
```

Master requires the `gate` status, which `.github/workflows/gate.yml` posts
when every check is green. Pull requests merge as merge commits, the only
method allowed: `gh pr merge <n> --auto --merge`, set only after the last
push. Branch commits land on master as they are, so tidy them before opening
the pull request, and catch a branch up with master by rebase, not by merging
master into it.

# Research

Research runs in the session that holds the game, one question at a time,
on a branch of this repository. These rules hold whoever does it:

- **One game, one recorder.** Only the session that holds the game records:
  `convert --backend game`, `verify --backend game`, `game record`,
  `scripts/record-fights.py`, and a run script that declares `game:`
  (`mechcore run <script> --check` says whether one does). CI verifies every
  fight document under `tests/`, and `tests/README.md` says where one lives:
  one home by where it came from, cited by every other topic it bears on.
- **A pinned hash comes from a recording made where the game runs**, never
  from the simulator. A pin that moves is a finding to explain, not a number
  to edit. The repository keeps what reproduces a recording, the fixtures and
  scripts under `tests/<topic>/` and the hashes they pin, never the recording.
- **The hash is the scorer, and the owner admits what it reads.** What the
  MCFR content hash reads is listed in `crates/mcfr/hashed-content.txt` and
  admitted by `docs/spec/mcfr/mcfr.md`'s rules. An agent adds instrument
  channels on its own. A pull request that changes that file says which
  admission condition each changed line meets, and the owner merges it; an
  agent never sets it to auto-merge.
- **Mirror the build.** Each new function mirrors a build method, and two
  functions mirroring one method are a divergence. What differs between two
  owners reaches shared code only through the interface the build uses
  (`ISkillOwner`, `IAttacker`). A branch on a kind of object in shared code
  points to where the build branches: an override or a type test. A new
  variant of a shared type is handled by shared code or refused by name, not
  handled only on the new kind's path. A divergence the build does not have is
  a step back even when every recording agrees.
- **What counts as evidence** is `work/research/README.md`'s to say, and a
  rule lands in `docs/rules/` as `docs/README.md` says.

The evidence lives outside this repository's history. The decompilation is in
the private `mechcore-decomp`, put under `work/decomp/<build>/` by
`scripts/decomp/decomp.py sync`, which builds `index.sqlite` from the dump locally.
The native replays are in the public `mechcore-replay`, fetched at master to
`work/replay/` by `scripts/corpus/replay.py sync`. Recordings exist only on the
machine that made them.

**The game version is written only in `GAME_VERSION`**, one line, the game's
own `Application.version`. The server matches games and spectators by it, so
one version is one rule set; a Steam update that keeps the version (a new
buildid) is not a new version. Crates embed it, `scripts/build_data.py` reads
it, and the corpus and decompilation directories are named by it; `config/`,
tests and docs never write a version of their own. Changing version is
changing this line on a branch and moving the decompilation, the extraction,
the recordings and pins in `tests/`, and the corpus to it, then merging when
all is green. `scripts/decomp/rules-anchors.py --since <old>` lists the rules whose
anchors moved, each reread on the new version (see `docs/README.md`).

For a new version, the session with the game runs `scripts/decomp/decompile.py`,
which reads the build from the installed game and fetches its pinned tools
into `work/tools/`, then `scripts/decomp/decomp.py publish <build>`.
`scripts/decomp/decomp-diff.py <old> <new>` compares two builds, and `--config`
their tables.

# Commits

Follow [Conventional Commits](https://www.conventionalcommits.org/), in
English. A pull request's title and body become its merge commit; its branch
commits land too, and each follows the same form. One pull request is one
purpose: split by what caused a change, not by layer or by what happened to
be found together, and land a mechanical move on its own.

The body says what the diff cannot: before and after, with numbers; what
disproved the old belief and what evidence did; which numbers were read from
the game and which were measured; what was verified and how, and what was
not. Every sentence will later be quoted as fact.

The corpus distance is not one of the numbers an author measures. The corpus
workflow (`.github/workflows/corpus.yml`) measures every pull request against
the master commit it is based on and keeps the numbers in a comment on it; a
body leaves them to that comment and builds no master baseline locally. What
only the machine with the game has, its recordings, is still verified there.

Write an issue or pull request number only for a dependency: `Closes #n`,
`Blocked by #n`, or a decision that moves another item. A tracked file never
carries an issue number.

Every commit and pull request an agent writes ends with a `Co-Authored-By`
trailer naming its real model, as the last paragraph with nothing after it:

```text
Co-Authored-By: GPT-6 <noreply@openai.com>
Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
```
