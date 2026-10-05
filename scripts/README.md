# Scripts

What the `mechcore` binary does not: reading the build, keeping the replay
corpus, checking the repository, and loops over the binary that keep what
they make. A job the binary can do in one command belongs in the binary, and a
script that only calls it once per file is retired for that command.

Each script says in its own docstring how to run it. Scripts are Python 3 from
the standard library unless the docstring names the packages to run it with
through `uv run --with`. Two exceptions: `player/export-models.mjs` is Node,
because what it exports is the player's own JavaScript, and `decomp/ghidra/` is
Java, the language Ghidra runs a headless script in without a Python of its own.

| Directory | Scripts | What they work on |
| --- | --- | --- |
| [`decomp/`](decomp/) | `decompile.py`, `decomp.py`, `decomp-diff.py`, `rules-anchors.py`, `fight-structure.py`, `ghidra.py` and `ghidra/*.java` | the build's decompilation under `work/decomp/<build>/`: making it from the installed game, fetching and publishing it, comparing two builds, holding `docs/rules/`'s anchors to it, reading the fight architecture out of its index, and decompiling a method to C with Ghidra |
| [`extract/`](extract/) | `extract-*.py`, `extract_*.py`, `name-tables.py`, `buff_rows.py` | `config/`, read out of the build's typed export; `name-tables.py` writes `docs/rules/`'s name tables from `config/localization.yaml`, and `buff_rows.py` is the buff source the equipment and technology extractions both write |
| [`corpus/`](corpus/README.md) | `replay.py`, `collect-replays.py`, `export-replay-corpus.py`, `verify-matches.py`, `match-replays.py`, `fight-coverage.py`, `distance-report.py` | the native replay corpus and the matches converted from it |
| [`player/`](player/) | `model-views.py`, `export-models.mjs`, `pose-timing.py`, `record-poses.mcscript`, `rhino-slash.py` | the player's sprites in `crates/player/web/`: the game's models seen from above, which they are drawn after, also in the poses a recording shows; the sprites exported as [`models/`](../models/README.md); the attack timings read off a recording's poses; and the Rhino's strike, keyed off its clip |
| [`check/`](check/) | `check-docs.py`, `check-scripts.sh`, `check-adapter-packaging.py` | the repository itself: CI runs the first two, and the third checks the Adapter's packaging on a nightly toolchain |
| this directory | `build_data.py` | the build's typed export, which every script reading the build imports |
| this directory | `record-fights.py` | fight documents recorded with the game, keeping the recordings for a study, usually with instrument channels |

A script under a directory imports `build_data` from here; one whose file name
is not a module name is loaded by path. Scripts that write `config/` stamp the
path they were run from into the file's header, so moving one is regenerating
what it writes.
