#!/usr/bin/env python3
"""Print a default technology loadout from the current build's replay corpus.

Run: uv run --with pyyaml python scripts/corpus/default-loadout.py <corpus-root>
Redirect stdout to crates/mechcore/default-loadout.yaml to update the preset.
The corpus root is a mechcore-replay checkout, with matches/ and tournaments/.
Only headers matching GAME_VERSION count. Each match contributes each side
once, regardless of repeated spectator captures or tournament appearances.
Technologies are ranked by loadout appearances, not combat activations; ties
use ascending technology ID. War Factory, Abyss and Mountain get six choices,
other units four. Output follows the document's unit and technology ID order.
Counts go to stderr so the measurement can be kept with the change's evidence.
Requires PyYAML.
"""

import argparse
from collections import Counter, defaultdict
from pathlib import Path
import re
import sys

import yaml


ROOT = Path(__file__).resolve().parents[2]
LOADER = getattr(yaml, "CSafeLoader", yaml.SafeLoader)


def header(path):
    lines = []
    with path.open() as source:
        for line in source:
            if line.strip() == "---":
                break
            lines.append(line)
    return yaml.load("".join(lines), Loader=LOADER)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("corpus", type=Path)
    args = parser.parse_args()
    version = (ROOT / "GAME_VERSION").read_text().strip()
    names = yaml.load((ROOT / "config/names.yaml").read_text(), Loader=LOADER)
    units = yaml.load((ROOT / "config/unit_techs.yaml").read_text(), Loader=LOADER)["units"]
    counts = defaultdict(Counter)
    seen = set()
    for path in sorted([*args.corpus.glob("matches/**/*.yaml"), *args.corpus.glob("tournaments/**/*.yaml")]):
        match = header(path)
        if match.get("build") != version:
            continue
        identity = re.sub(r"-r\d+$", "", path.stem)
        if identity in seen:
            continue
        seen.add(identity)
        for side in ("blue", "red"):
            for unit, technologies in match[side]["tech_loadout"].items():
                counts[unit].update(set(technologies))
    if not seen:
        raise ValueError("the corpus holds no matches for GAME_VERSION")
    rows = []
    for unit in sorted(units, key=lambda row: row["unit_id"]):
        name = unit["type"]
        technologies = names["technologies"][name]
        legal = {row["id"] for row in unit["technologies"]}
        ranked = sorted(legal, key=lambda tech: (-counts[name][technologies[tech]], tech))
        slots = 6 if name in ("war_factory", "abyss", "mountain") else 4
        chosen = sorted(ranked[:slots])
        if len(chosen) != slots or any(not counts[name][technologies[tech]] for tech in chosen):
            raise ValueError(f"not enough observed technologies for {name}")
        rows.append(f"{name}: [{', '.join(technologies[tech] for tech in chosen)}]")
        print(f"{name}: " + ", ".join(f"{technologies[tech]}={counts[name][technologies[tech]]}" for tech in chosen), file=sys.stderr)
    print(f"{len(seen)} matches, {2 * len(seen)} player loadouts", file=sys.stderr)
    print("# Default match loadout, ranked from replay player choices.")
    print("# Regenerate with scripts/corpus/default-loadout.py; --loadout overrides it.")
    print("\n".join(rows))


if __name__ == "__main__":
    main()
