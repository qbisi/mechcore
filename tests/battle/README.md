# Battle document fixtures

Each file here is `mechcore convert` run over the replay of the same basename in
`tests/grbr`, with nothing edited afterwards. They are regenerated, never
hand-corrected: a claim that needs a different value is a claim about the
converter, and belongs in `crates/document/src/convert.rs`.

```bash
mechcore convert "tests/grbr/<name>.grbr" "tests/battle/<name>.yaml" --force
```

The four are the locally recorded ranked matches of `tests/grbr`; the two
`VS[电脑]` files are practice matches against the computer and are not tracked
here. `docs/spec/document/battle.md` describes the document and says which of its fields the
converter rebuilds rather than copies.

| File | Map | Seed | Rounds | Actions | Size | SHA-256 |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| `2259_20260823--67294111_[你是蓬莱花仙]VS[crower].yaml` | 1001 | 1204807678 | 9 | 260 | 64239 | `18a03c52d8dc6d55fdbba1894fb881e4fa9a96c1d8c6f31370226a9b56b35080` |
| `2259_20260901--201562374_[crower]VS[[TUFF]MARLFAUX].yaml` | 1021 | 31103914 | 9 | 260 | 66220 | `ac84b4246474124c0726e963436ed8fd77ce99d25ecadb2c36298367d64b952d` |
| `2259_20260901--201562557_[crower]VS[[BORK]  Caine].yaml` | 1001 | 2038621361 | 10 | 332 | 85013 | `a02bd58b0d64dd1a35b4089bd7cff4477809cdff89bec53cfa3d45cb38d47d98` |
| `2259_20260901--67344528_[crower]VS[Charon].yaml` | 1032 | 403681099 | 9 | 287 | 65142 | `f5de58c9517cc2191879e9ad3e7cf097708515e1bd1495ea51732e832bf4d86d` |

Every one of them converts with both of the converter's checks closing
completely, which is what makes them usable as fixtures. A transition count is
nine field comparisons per round transition per side, so it moves whenever
`docs/spec/document/turn.md` gains a field:

| File | Supply ledger | Turn transition |
| --- | --- | --- |
| `2259_20260823--67294111_[你是蓬莱花仙]VS[crower].yaml` | 16 of 16 | 144 of 144 |
| `2259_20260901--201562374_[crower]VS[[TUFF]MARLFAUX].yaml` | 16 of 16 | 144 of 144 |
| `2259_20260901--201562557_[crower]VS[[BORK]  Caine].yaml` | 18 of 18 | 162 of 162 |
| `2259_20260901--67344528_[crower]VS[Charon].yaml` | 16 of 16 | 144 of 144 |

None is paid by the fight and none is unpriced, so the whole corpus is priced
supply that the ledger reproduces from the previous round.

`mechcore verify` reads layout documents only, and refuses these by kind. There
is no battle verifier yet; the conversion's own two checks are the check.
