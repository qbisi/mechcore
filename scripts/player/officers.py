#!/usr/bin/env python3
"""Export native officer names and effect text for the offline player.

    uv run --with pyyaml python scripts/player/officers.py [--check]

Reads the local build export selected by GAME_VERSION, resolves description
parameters, and removes Unity rich-text tags. The output is presentation
text only; simulation and economy configuration remain separate.
"""
import json
from pathlib import Path
import re
import sys

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_data

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "crates/player/web/officers.json"


def arguments(parser):
    parser.add_argument("--check", action="store_true")


def main():
    args = build_data.arguments(__doc__, arguments)
    admitted = yaml.safe_load((ROOT / "config/names.yaml").read_text())["officers"]
    rows = build_data.container()["officerDatas"]
    catalog = {}
    for row in rows:
        if row["id"] not in admitted:
            continue
        text = re.sub(r"<[^>]+>", "", build_data.description("OfficerData", row)).strip()
        if not text or re.search(r"\{\d+\}", text):
            sys.exit(f"officer {row['id']} has no complete description")
        catalog[str(row["id"])] = {"id": row["id"], "name": row["name"], "effect": text}
    missing = set(map(str, admitted)) - catalog.keys()
    if missing:
        sys.exit(f"missing officers: {sorted(missing)}")
    result = json.dumps(catalog, ensure_ascii=False, indent=2) + "\n"
    if args.check:
        if OUTPUT.read_text() != result:
            sys.exit(f"stale {OUTPUT.relative_to(ROOT)}")
    else:
        OUTPUT.write_text(result)
    print(f"{len(catalog)} officer descriptions")


if __name__ == "__main__":
    main()
