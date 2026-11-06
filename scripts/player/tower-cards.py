#!/usr/bin/env python3
"""Extract native Energy Tower and Research Center icons for the player HUD.

    uv run --with UnityPy==1.25.3 --with pillow --with pyyaml python scripts/player/tower-cards.py [--check]

Requires the installed game and matching local build export. Each icon is
embedded once in the offline page, shared by both teams and every round.
"""
import base64
import io
import json
from pathlib import Path
import sys

import UnityPy
from PIL import Image
import yaml

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_data

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "crates/player/web/tower-cards.json"


def arguments(parser):
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--data", type=Path, default=Path.home() / "Library/Application Support/Steam/steamapps/common/Mechabellum/Mechabellum.app/Contents/Resources/Data")


def main():
    args = build_data.arguments(__doc__, arguments)
    names = yaml.safe_load((ROOT / "config/names.yaml").read_text())
    container = build_data.container()
    result, wanted = {}, set()
    for group, key, table in [("energy", "energy_tower_skills", "energyTowerSkillDatas"),
                              ("blueprints", "blueprints", "blueprints")]:
        rows = {row["id"]: row for row in container[table]}
        result[group] = {str(ident): rows[ident]["iconName"] for ident in names[key]}
        wanted.update(result[group].values())
    env = UnityPy.load(str(args.data / "sharedassets0.assets"))
    images = {}
    for obj in env.objects:
        if obj.type.name != "Sprite":
            continue
        sprite = obj.read()
        if sprite.m_Name not in wanted:
            continue
        image = sprite.image.convert("RGBA")
        image.thumbnail((64, 64), Image.Resampling.LANCZOS)
        stream = io.BytesIO()
        image.save(stream, format="WEBP", lossless=True, method=6)
        images[sprite.m_Name] = "data:image/webp;base64," + base64.b64encode(stream.getvalue()).decode()
    if missing := wanted - images.keys():
        sys.exit(f"missing native sprites: {sorted(missing)}")
    result["images"] = {name: images[name] for name in sorted(images)}
    text = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if args.check:
        if OUTPUT.read_text() != text:
            sys.exit(f"stale {OUTPUT.relative_to(ROOT)}")
    else:
        OUTPUT.write_text(text)
    print(f"{len(result['energy'])} tower skills, {len(result['blueprints'])} blueprints, {len(images)} images")


if __name__ == "__main__":
    main()
