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
documents, which is every tracked replay. Together they contain 334 deployment
rounds and 9,977 actions over maps 1001, 1011, 1021, 1031 and 1032. The opening
each side took is under `sides` rather than in a round of its own, so the 82
opening choices are not among those actions. Each opening carries `choose`, a
zero-based index into its four reconstructed `offers`; the selected team and
specialist are read from that entry.

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

Three converted documents carry a `concession`: `[Dre420]VS[[TUFF] Wumple
Doodle]`, `[NemoCoda]VS[Camilo.Y]` and `[Dr. crbN]VS[trevorism]`.

CI regenerates this directory on every push and pull request and fails when the
result differs, so a converter change that left the corpus behind cannot merge.

`mechcore verify tests/battle/*.yaml` checks both sides' complete opening offers
and initial construction lists against each battle's seed and map ID, and
refuses an out-of-range `choose`. It computes the reinforcement initialization
draws and independent map stream directly, without searching stream positions.
It also advances that stream through all 293 reinforcement rounds and checks
1,172 ordered card IDs, using round states and prior choices as inputs. The
regression test compares every incoming stream state and each available next
snapshot against the native replay. Missing offers, reordered ordinary or unit
cards, invalid choices and discontinuous rounds must fail.
Deployment seams are checked by the conversion's ledger and transition reports;
these deal checks do not verify combat or authenticate the player's choice
without the source replay.
