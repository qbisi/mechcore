---
name: decompile
description: Decompile the installed Mechabellum build into work/decomp/<build>, publish it to mechcore-decomp, and diff it against another build. Use when the game updates, when a build's dump or config is missing locally, or when asked what changed between two builds.
---

# Decompile a build

Only a session on the machine that has the game installed can make a new
build. Every other session fetches one with `scripts/decomp.py sync`.

## Make and publish

```bash
python3 scripts/decompile.py
python3 scripts/decomp.py publish <build>
```

- `decompile.py` reads the build number and Unity version from the game's
  `Info.plist`, downloads any missing tool into `work/tools/` (Cpp2IL and
  AssetRipper, pinned by version and SHA-256), and writes
  `work/decomp/<build>/`: `cpp2il/IsilDump`, `cpp2il/DiffableCs`,
  `config-data-container.json`, `game-manifest.json`, `index.sqlite`.
  It takes about six minutes, most of it AssetRipper loading the game.
- A step whose output exists is skipped. `--force isil,cs,config,...` redoes
  named steps; `--game PATH` or `MECHABELLUM_APP` points at another install.
- `publish` commits the build directory to `qbisi/mechcore-decomp`, pushes,
  and releases `index.sqlite.gz` as `index/<build>`. Rerun it if a push fails.

Do not change the fixed choices the script's docstring lists (the x86_64
slice, the Cpp2IL processors, the stripped attribute lines, loading the
whole `.app`). They keep two builds' dumps comparable line by line.

## See what changed

```bash
python3 scripts/decomp-diff.py <old-build> <new-build>            # declarations, GRFight GRCore GRUtility
python3 scripts/decomp-diff.py <old-build> <new-build> --config   # config tables, row by row
```

Rows whose `limitedScene` is only 8 and 9 belong to Interstellar Expedition
(`matchSettings` with `serverSubType` 8 and 9). mechcore does not model that
mode, so leave those rows out of any review.
