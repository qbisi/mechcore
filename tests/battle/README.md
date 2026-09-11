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

[SHA256SUMS](SHA256SUMS) records the exact identity of all 35 generated
documents. Together they contain 319 rounds and 8,555 actions over maps 1001,
1011, 1021, 1031 and 1032.

Six otherwise standard replays are deliberately absent. Each carries commander
skill `800001` into a later round, but a replay does not record the retained
Shield Airdrop objects needed to construct a truthful battle state:

- `2259_20260910--134504097_[Dre420]VS[[TUFF] Wumple Doodle].grbr`
- `2259_20260910--134505087_[Dr.Reading♠Ace]VS[Burned My Tongue].grbr`
- `2259_20260910--134505246_[NemoCoda]VS[Camilo.Y].grbr`
- `2259_20260910--201615750_[KN1FEAR]VS[Píngxí].grbr`
- `2259_20260910--67396921_[elRAKAMAKAFON]VS[p站智慧官叫馆].grbr`
- `2259_20260911--134508150_[Thorrrin]VS[占星].grbr`

The supply ledger closes all 568 of 568 convertible round transitions. The
nine-field turn transition closes 5,110 of 5,112 comparisons. The two open
comparisons are equipment deliveries in
`[kulinichstas1985]VS[Menschlein]` round 4 blue and
`[🐙Noname🐙]VS[Rievin]` round 5 blue; the exact missing IDs are pinned by the
transition tests rather than hidden from the corpus.

`[Dr. crbN]VS[trevorism]` is the only converted document with a `concession`.
Two more source replays contain concessions but belong to the retained-airdrop
refused subset above.

`mechcore verify` reads layout documents only, and refuses these by kind. There
is no battle verifier yet; the conversion's ledger and transition reports are
the checks.
