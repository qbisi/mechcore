#!/usr/bin/env python3
"""Extract native skill and unused-equipment artwork for the player HUD.

    uv run --with UnityPy==1.25.3 --with pillow --with pyyaml python scripts/player/arsenal.py [--check]

Requires the installed game and matching local build export. HUD thumbnails
are separate from battlefield vector artwork. --check compares saved output.
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
OUTPUT = ROOT / "crates/player/web/arsenal.json"


def arguments(parser):
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--data", type=Path, default=Path.home() / "Library/Application Support/Steam/steamapps/common/Mechabellum/Mechabellum.app/Contents/Resources/Data")


def main():
    args = build_data.arguments(__doc__, arguments)
    names = yaml.safe_load((ROOT / "config/names.yaml").read_text())
    result, candidates = {}, set()
    for group, key, table in [("skills", "commander_skills", "CommanderSkillGroupData"), ("equipment", "equipment", "EquipmentGroupData")]:
        rows = {r["id"]: r for values in build_data.level0(table).values()
                if isinstance(values, list) for r in values if isinstance(r, dict) and "id" in r}
        result[group] = {}
        for ident, kind in names[key].items():
            row = rows[ident]
            tables = [term.split('/')[1] for term, languages in build_data._terms().items()
                      if term.endswith(f"/name_{ident}") and languages[0] == row["name"]]
            effect = next((text for t in tables if (text := build_data.description(t, row))), "")
            effect = re.sub(r"<[^>]+>", "", effect).strip()
            if not effect or re.search(r"\{\d+\}", effect):
                sys.exit(f"{group} {ident} has no complete description")
            icon = row["iconName"]
            result[group][str(ident)] = {"kind": kind, "name": row["name"], "effect": effect, "icon": icon}
            candidates.update([icon, icon + "_f"])
    env = UnityPy.load(str(args.data / "sharedassets0.assets"))
    images = {}
    for obj in env.objects:
        if obj.type.name != "Sprite":
            continue
        sprite = obj.read()
        if sprite.m_Name not in candidates:
            continue
        image = sprite.image.convert("RGBA")
        image.thumbnail((96, 96), Image.Resampling.LANCZOS)
        stream = io.BytesIO()
        image.save(stream, format="WEBP", lossless=True, method=6)
        images[sprite.m_Name] = "data:image/webp;base64," + base64.b64encode(stream.getvalue()).decode()
    used = set()
    for group in ["skills", "equipment"]:
        for row in result[group].values():
            if group == "skills" and row["icon"] + "_f" in images:
                row["icon"] += "_f"
            if row["icon"] not in images:
                sys.exit(f"missing native sprite {row['icon']}")
            used.add(row["icon"])
    result["images"] = {name: images[name] for name in sorted(used)}
    text = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if args.check:
        if OUTPUT.read_text() != text:
            sys.exit(f"stale {OUTPUT.relative_to(ROOT)}")
    else:
        OUTPUT.write_text(text)
    print(f"{len(result['skills'])} skills, {len(result['equipment'])} equipment, {len(used)} images")


if __name__ == "__main__":
    main()
