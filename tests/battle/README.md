# Battle document fixtures

Each YAML file here is `mechcore convert` run over the replay of the same
basename in `tests/grbr`, with nothing edited afterwards. They are regenerated,
never hand-corrected: a claim that needs a different value is a claim about the
converter, and belongs in `crates/document/src/convert.rs`.

```bash
mechcore convert "tests/grbr/<name>.grbr" "tests/battle/<name>.yaml" --force
```

The scalable corpus entry point is:

```bash
python3 scripts/export-replay-corpus.py --offline-only --force-battles
```

[SHA256SUMS](SHA256SUMS) records the exact identity of all 41 generated
documents, which is every tracked replay. Each is a stream of segments: a
header, the round-zero action segment holding the 82 opening choices, and then
334 deployment rounds, each a state segment followed by an action segment,
holding 9,980 actions over maps 1001, 1011, 1021, 1031 and 1032. Each opening
choice names a zero-based `offer` into its side's four reconstructed header
`offers`, together with the team and specialist that entry holds.

Every document ends on its last round's action segment. A replay records a
fight's result only in the snapshot that opens the next round, and the fight
that ends a match opens none, so no converted battle states it.

Three documents carry a retained Shield Airdrop, which the converter reads from
the releasing skill's own `rangeItems` rather than from any object list:
`[Dre420]VS[[TUFF] Wumple Doodle]` round 7 on both sides,
`[elRAKAMAKAFON]VS[p站智慧官叫馆]` rounds 3 and 4 red, and
`[Thorrrin]VS[占星]` round 3 red. The second of those stands one shield across two
rounds, which is what distinguishes a retained object from a restatement of one
round's release.

The supply ledger closes all 668 of 668 seams, which is each side's opening onto
its first round and then every round onto the next. The nine-field turn
transition closes 6,010 of 6,012 comparisons. The two open comparisons are
equipment deliveries in `[kulinichstas1985]VS[Menschlein]` round 4 blue and
`[🐙Noname🐙]VS[Rievin]` round 5 blue; the exact missing IDs are pinned by the
transition tests rather than hidden from the corpus.

Three converted documents end with a `concede` decision, and those three are
among the 9,980 actions: `[Dre420]VS[[TUFF] Wumple Doodle]`,
`[NemoCoda]VS[Camilo.Y]` and `[Dr. crbN]VS[trevorism]`. The other 38 matches
ended in a fight whose result no replay records.

CI regenerates this directory on every push and pull request and fails when the
result differs, so a converter change that left the corpus behind cannot merge.

`mechcore verify tests/battle/*.yaml` first reads each document as a stream and
refuses one whose segments are misordered, misnumbered, or continue past a
concession or a destroyed reactor core. It then checks both sides' complete
opening offers and initial construction lists against each battle's seed and
map ID, and refuses an opening choice whose `offer` is out of range or whose
team and specialist are not what that offer holds. It computes the
reinforcement initialization draws and independent map stream directly, without
searching stream positions.
It also advances that stream through all 293 reinforcement rounds and checks
1,172 ordered card IDs, using round states and prior choices as inputs. The
regression test compares every incoming stream state and each available next
snapshot against the native replay. Missing offers, reordered ordinary or unit
cards, invalid choices and discontinuous rounds must fail.
These deal checks do not verify combat or authenticate the player's choice
without the source replay.

`verify` then measures each transition: from a round's state and decisions it
predicts the next round's opening, and puts every leaf of the recorded opening
in one of four classes, equal, unequal, unimplemented or decided by the fight,
as [`battle.md`](../../docs/spec/document/battle.md#transition-coverage)
defines. A battle verifies only when no leaf is unequal or unimplemented, and
every document here verifies: each round's next opening, round zero's
included, is predicted in every leaf the fight does not decide. The summary
over this directory, by field group and by document with every unequal leaf
listed, is:

```bash
cargo build --release -p mechcore
python3 scripts/verify-battles.py
```

CI runs it after regenerating this directory. The counts by field group are
also pinned by `crates/document/src/coverage.rs`, so a change in what the
transition predicts fails the tests.
