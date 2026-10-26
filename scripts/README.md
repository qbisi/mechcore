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
| [`decomp/`](decomp/) | `decompile.py`, `decomp-diff.py`, `rules-anchors.py`, `fight-structure.py`, `ghidra.py` and `ghidra/*.java` | the build's decompilation under `work/decomp/<build>/`: making it locally from the installed game, comparing two builds, holding `docs/rules/`'s anchors to it, reading the fight architecture out of its index, and decompiling a method to C with Ghidra |
| [`extract/`](extract/) | `extract-*.py`, `extract_*.py`, `name-tables.py`, `buff_rows.py` | `config/`, read out of the build's typed export; `name-tables.py` writes the name tables of `docs/rules/` and `docs/terminology/` from `config/localization.yaml`, and `buff_rows.py` is the buff source the equipment and technology extractions both write |
| [`corpus/`](corpus/README.md) | `replay.py`, `collect-replays.py`, `tournament-replays.py`, `export-replay-corpus.py`, `verify-matches.py`, `match-replays.py`, `divergence-issue.py` | the collected match corpus and local native replay studies |
| [`player/`](player/) | `model-views.py`, `export-models.mjs`, `pose-timing.py`, `rhino-slash.py`, `shot-effects.py`, `skill-icons.py`, `hud-font.py`, `officers.py`, `unit-panels.py` | the player's sprites in `crates/player/web/`: the game's models seen from above, which they are drawn after, also in the poses a recording shows; the sprites exported as [`assets/models/`](../assets/models/README.md); the attack timings read off a recording's poses; the Rhino's strike and its Whirlwind spin, keyed off their clips; the effects the game flies for each weapon; native skill icons traced into vectors; the HUD's numeric font subset; native officer names and descriptions; and unit portraits and technology icons for the side drawers |
| [`arena/`](arena/) | `random-player.py`, `game-ai-player.py` | players for `mechcore arena run`: one that takes legal decisions at random, the smallest worked example of the request stream, and the reference opponent, modelled on the game's own computer opponent |
| [`check/`](check/) | `check-docs.py`, `check-fixture-readers.py`, `check-scripts.sh`, `check-adapter-packaging.py` | the repository itself: CI runs the first three, `check-fixture-readers.py` holding that no crate's code reads a fixture, and the fourth checks the Adapter's packaging on a nightly toolchain |
| this directory | `build_data.py` | the build's typed export, which every script reading the build imports |
| this directory | `record-fights.py` | fight documents recorded with the game, keeping the recordings for a study, usually with instrument channels |

A script under a directory imports `build_data` from here; one whose file name
is not a module name is loaded by path. Scripts that write `config/` stamp the
path they were run from into the file's header, so moving one is regenerating
what it writes.

`player/skill-icons.py` extracts the twenty native deployment symbols' front-facing
high-resolution `*_f` variants from the installed game and traces their alpha contours with Potrace into
`crates/player/web/skill-icons.js`. Original images and intermediate traces
stay under `work/player/skill-icons/`. Its `--check` requires the game and is
a local asset-generation check, not a Linux CI prerequisite.

`player/hud-font.py` subsets Xolonium-Regular to the HUD's numeric glyphs,
retaining its font metadata. It writes `crates/player/web/hud-font.css`.
Its `--check` also needs the installed game and is a local generation check
rather than a CI prerequisite. The HUD's SVG borders and repeating texture
are designed in the player and are not extracted from game artwork.

`player/officers.py` extracts native English officer names and resolved effect
descriptions into `crates/player/web/officers.json`, limited to the officers
named by `config/names.yaml`. Its `--check` requires the local build export.
The player carries only each deployment's held officers into the offline page.

`player/unit-panels.py` reads the native CardData portraits and
TechnologyGroupData icons and descriptions for the player's closed side
drawers. It writes deduplicated WebP thumbnails and text into
`crates/player/web/unit-panels.json`; the page selects only assets its rounds
use. Its `--check` needs the installed game and local build export.

`check/check-scripts.sh` verifies every tracked fight fixture in parallel and
keeps a report for every fixture under a fresh run directory in
`work/check/fixtures/`, or `FIXTURE_REPORT_DIR` when set. It prints complete
JSON reports in fixture-path order after the workers finish. `results.jsonl`
holds every report, `failures.jsonl` the failed ones, and `status.tsv` and
`stderr.log` keep process failures that produced no verification report. The
per-fixture files preserve stdout, stderr and the exit code separately.
CI uploads the complete run as its `fixture-verify` artifact on success or
failure. A fight document can report every outcome, assert, tick-count and
hash difference; a hash alone cannot locate a differing tick. Comparing
per-tick fields still needs the game's recording and `mechcore diff`.
