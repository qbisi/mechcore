---
name: read-decomp
description: Query local Mechabellum decompilation under work/decomp or decompile a game method when investigating game mechanics, simulator divergence, or simulation refusals.
---

# Decompilation queries

To inspect the game's actual implementation, classes, methods, or C# / C decompilation, look under `scripts/decomp/`.

Each script documents its CLI and usage in its own docstring (`--help` or read top of file). Key entry points:

- `scripts/decomp/ghidra.py`: Decompiles a specific method to C (`ghidra.py decompile Class.Method`).
- `scripts/decomp/decompile.py`: Generates the installed game's decompilation and `index.sqlite` locally under `work/decomp/<build>`. Requires the game on this machine; artifacts stay local.
- `scripts/decomp/decomp-diff.py`: Diffs C# declarations or config tables across builds.
- `scripts/decomp/rules-anchors.py`: Checks or updates decompilation anchors referenced by `docs/rules/`.
- `work/decomp/<build>/index.sqlite`: Symbol and source index (queryable via sqlite3).
- `work/decomp/<build>/cpp2il/`: C# stubs (`DiffableCs/`) and instruction dumps (`IsilDump/`).
