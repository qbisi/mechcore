# Learning data

Deterministic preparation of model inputs from repository configuration.
These scripts do not train a policy or run battles. Generated tables live
under `work/learning/static/`, outside Git; configuration remains the source.

Run `uv run --with pyyaml python scripts/learning/static-tables.py` to write
`units.json`, `technologies.json` and `manifest.json`. Use `--output DIR` to
choose another directory and `--check` to compare without writing. Output is
byte-identical for the same input configuration, independent of timestamps.

The unit table includes all configured unit types, including spawned types
without a shop price. Its weapons retain separate main and technology-granted
rows, group sizes and release timing; damage is per member/weapon, not DPS.
Prices are base catalogue prices, not a quote for the current observation.
A technology's membership in this catalogue is not its player's loadout.

Technology parameters preserve the source's nested structure and rank arrays.
Explicitly listed Q32.32 fields are converted to ordinary numbers; ordinary
integers, enums and booleans stay unchanged. `parameter_encodings` records
source encodings by path (arrays use `[]`). Missing parameters remain absent,
not zero. A new unclassified parameter fails generation until its encoding is
declared. Consumers must carry presence masks and interpret rank lists using
the owning rule, not assume every list is a rank list. Mechanism kinds are
categorical labels; native IDs are join keys, not continuous model features.
The tables are structured input catalogues, not a final padded tensor schema.

The manifest hashes every source file and output table and records the game
version. It belongs with a training checkpoint. Regenerate after config
changes; `--check` detects stale tables. No game, recording or network is needed.

Run the focused generator checks with
`uv run --with pyyaml python scripts/learning/test_static_tables.py`.
