#!/usr/bin/env python3
"""Extract what a travelling unit's arrival takes into `config/super_deployment.yaml`.

    python3 scripts/extract/extract-super-deployment.py [--build BUILD] [--check]

The two numbers are `Config`'s, read through `scripts/build_data.py`:
`superDeploymentTravelTime`, the whole seconds a side's travelling units take
to arrive, and `superDeploymentLifeRate`, the share of its life a travelling
unit starts with. `docs/rules/super_deployment.md` states what they mean.
The rate is the FPoint raw integer the build stores, with a comment reading it.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_data  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]


def render():
    config = build_data.level0("Config")
    travel_time = config["superDeploymentTravelTime"]
    life_rate = config["superDeploymentLifeRate"]["m_rawValue"]
    reading = f"{life_rate / (1 << 32):+.4f}".rstrip("0").rstrip(".")
    return "\n".join([
        "schema: mechcore.super_deployment",
        "",
        "# What a travelling unit's arrival takes, read out of Config by",
        "# scripts/extract/extract-super-deployment.py.",
        "# docs/rules/super_deployment.md states what each field means.",
        "",
        f"travel_time: {travel_time}",
        f"life_rate: {life_rate}  # {reading}",
    ]) + "\n"


def main():
    arguments = build_data.arguments(__doc__, lambda parser: parser.add_argument("--check", action="store_true"))
    output = ROOT / "config/super_deployment.yaml"
    written = render()
    if arguments.check:
        if not output.exists() or output.read_text() != written:
            sys.exit("config/super_deployment.yaml differs from the build's export")
        print("super deployment agrees byte for byte")
    else:
        output.write_text(written)
        print(f"super deployment -> {output.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
