# Native GRBR fixtures

These are unmodified Mechabellum build-2259 replays copied incrementally from
the local Steam installation. Existing destinations are never
overwritten. Do not rewrite or normalize these files; native loadability and
hash identity are part of the fixture contract.

| Repository file | Original Steam basename | Size | SHA-256 |
| --- | --- | ---: | --- |
| `2259_26-09-03__13-25-40-032_[crower]VS[电脑].grbr` | same | 85695 | `3757b76ac8789e8271ce8bb108c5e9bc29d9efd8b4088a1a85cee84273682c1d` |
| `2259_20260901--201562557_[crower]VS[[BORK]  Caine].grbr` | same | 604429 | `90baee4232b812e012fd2e309a54b0bb4a8f8642731436d35622b0b67779c335` |
| `2259_20260823--67294111_[你是蓬莱花仙]VS[crower].grbr` | same | 483172 | `d30eea2b61a7afdc101ba6391a94d67ee79cf574b7b0f46c0124f4131d7e4630` |
| `2259_20260901--201562374_[crower]VS[[TUFF]MARLFAUX].grbr` | same | 485258 | `f16db1bd8ee2ba4b9d144d5ef314081764b64cec01f61b1d8b065624cb84ba64` |
| `2259_20260901--67344528_[crower]VS[Charon].grbr` | same | 490505 | `c0468f0b3c63512d683fe96a3f8d1e0894018e00ff835212b5c5bc0e0d47be55` |

`2259_20260901--201562557_[crower]VS[[BORK]  Caine].grbr` is the integration sample exercised by
`record_replay_round`. Its serialized player records contain indices `0..=9`;
native replay capture accepts battle rounds `1..=9` unchanged and verifies the
same value through `Match.get_RoundCount()`.
