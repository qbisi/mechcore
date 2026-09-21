# Replays

The native replays this repository is held to, and the battle documents
converted from them, live in
[qbisi/mechcore-replay](https://github.com/qbisi/mechcore-replay), one
directory per game build, added to and never rewritten. This directory holds
what ties the two repositories together:

| File | What it is |
| --- | --- |
| `REPLAY_REV` | the mechcore-replay commit this checkout reads the corpus at |
| `record-standard-1v1.mcscript` | how new replays are made: it watches live standard 1v1 matches unattended and keeps each one; it needs the game, so CI only parses it |

`scripts/replay.py sync` fetches the pinned commit into the untracked
`work/replay/`, and everything here reads the corpus there, under
`work/replay/replays/<build>/{grbr,battle}`: `scripts/verify-battles.py`,
`scripts/fight-coverage.py`, and CI's `scripts` job. The test suite reads no
replay, so `cargo test` needs no fetch; what the corpus establishes is checked
by the scripts, over all of it.

**A replay is evidence.** It is copied from the Steam installation byte for
byte and never rewritten; hash identity is part of the corpus's contract. Only
a locally recorded replay is admitted: the downloaded class, named
`<version>\<id>.rep.grbr` and carrying `Seat` -1, is a server-side
reconstruction whose `IsSpecialSupply` and blueprint chains do not agree with
the game's state. The corpus's README says how the two are told apart.

**A battle document is generated and never hand-corrected.** The corpus's
workflow checks out mechcore at the commit its `MECHCORE_REV` names, converts
every replay with `mechcore replay convert`, verifies every document, and
commits the result. A value that looks wrong is a claim about the converter and
belongs in `crates/document/src/convert.rs`. When the converter changes, the
corpus's `MECHCORE_REV` moves to that change and `REPLAY_REV` here moves after
it; CI converts the pinned corpus again and requires the same bytes, so a
converter change that was not carried across cannot merge.

Nothing here is a test of its own. `scripts/verify-battles.py` runs
`mechcore doc verify` over every document and requires each next opening to
be predicted in every leaf the fight does not decide, which is a different
process from the tests under [`../tests/`](../tests/README.md). Layouts built
by hand, and the one captured live from a replay's round, are in
[`../layouts/`](../layouts/README.md).
