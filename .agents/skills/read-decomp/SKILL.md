---
name: read-decomp
description: Query the decompilation under work/decomp/<build> or decompile a game method when investigating game mechanics, simulator divergence, or simulation refusals.
---

# Decompilation queries

To inspect the game's actual implementation, classes, methods, or C# / C decompilation, look under `scripts/decomp/`.

Each script documents its CLI and usage in its own docstring (`--help` or read top of file). Key entry points:

- `scripts/decomp/ghidra.py`: Decompiles a specific method to C (`ghidra.py decompile Class.Method`).
- `scripts/decomp/decomp.py`: Fetches or verifies the decompilation under `work/decomp/<build>` and builds `index.sqlite`.
- `scripts/decomp/decomp-diff.py`: Diffs C# declarations or config tables across builds.
- `scripts/decomp/rules-anchors.py`: Checks or updates decompilation anchors referenced by `docs/rules/`.
- `work/decomp/<build>/index.sqlite`: Symbol and source index (queryable via sqlite3).
- `work/decomp/<build>/cpp2il/`: C# stubs (`DiffableCs/`) and instruction dumps (`IsilDump/`).
