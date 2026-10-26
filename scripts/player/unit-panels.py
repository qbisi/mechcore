#!/usr/bin/env python3
"""Extract native unit portraits and technology icons for player side panels.

    uv run --with UnityPy==1.25.3 --with pillow --with pyyaml python scripts/player/unit-panels.py [--check]

Reads the installed game's sharedassets0 and the matching local build export.
Writes deduplicated WebP thumbnails and native text to web/unit-panels.json.
Only HUD images use these assets; battlefield SVG exports remain vectors.
"""
import base64
import io
import json
from pathlib import Path
import re
import sys

import UnityPy
from PIL import Image
import yaml

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_data

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "crates/player/web/unit-panels.json"


def arguments(parser):
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--data", type=Path, default=Path.home() / "Library/Application Support/Steam/steamapps/common/Mechabellum/Mechabellum.app/Contents/Resources/Data")


def main():
    args = build_data.arguments(__doc__, arguments)
    cards = {r["id"]: r for r in build_data.container()["cardDatas"]}
    technologies = {r["id"]: r for rows in build_data.level0("TechnologyGroupData").values()
                    if isinstance(rows, list) for r in rows if isinstance(r, dict) and "id" in r}
    groups = yaml.safe_load((ROOT / "config/unit_techs.yaml").read_text())["units"]
    units, techs, wanted = {}, {}, set()
    for group in groups:
        row = cards[group["unit_id"]]
        portrait = row["advancePortraitName"]
        units[group["type"]] = {"id": group["unit_id"], "name": row["name"], "portrait": portrait}
        wanted.add(portrait)
        for tech in group["technologies"]:
            row = technologies[tech["id"]]
            name, icon = row["name"], row["iconName"]
            tables = [term.split('/')[1] for term, languages in build_data._terms().items()
                      if term.endswith(f"/name_{row['id']}") and languages[0] == name]
            descriptions = [build_data.description(table, row) for table in tables]
            effect = next((text for text in descriptions if text), "")
            effect = re.sub(r"<[^>]+>", "", effect).strip()
            if not effect or re.search(r"\{\d+\}", effect):
                sys.exit(f"technology {row['id']} has no complete description")
            techs[str(row["id"])] = {"name": name, "effect": effect, "icon": icon}
            wanted.add(icon)
    env = UnityPy.load(str(args.data / "sharedassets0.assets"))
    found = {}
    for obj in env.objects:
        if obj.type.name != "Sprite":
            continue
        sprite = obj.read()
        if sprite.m_Name not in wanted:
            continue
        image = sprite.image.convert("RGBA")
        # UI thumbnails retain the native artwork and alpha, at 2x their display size.
        image.thumbnail((160, 160) if sprite.m_Name.startswith("Mech_") else (64, 64), Image.Resampling.LANCZOS)
        stream = io.BytesIO()
        image.save(stream, format="WEBP", lossless=not sprite.m_Name.startswith("Mech_"), quality=90, method=6)
        found[sprite.m_Name] = "data:image/webp;base64," + base64.b64encode(stream.getvalue()).decode()
    if missing := wanted - found.keys():
        sys.exit(f"missing native sprites: {sorted(missing)}")
    result = json.dumps({"units": units, "techs": techs, "images": found}, ensure_ascii=False, indent=2) + "\n"
    if args.check:
        if OUTPUT.read_text() != result:
            sys.exit(f"stale {OUTPUT.relative_to(ROOT)}")
    else:
        OUTPUT.write_text(result)
    print(f"{len(units)} portraits, {len(techs)} technologies, {len(found)} unique images")


if __name__ == "__main__":
    main()
