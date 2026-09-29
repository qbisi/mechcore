# Configuration

Every file here is generated from the build the repository describes, the one
[`GAME_VERSION`](../GAME_VERSION) names, and none is edited by hand. A value
that looks wrong is a claim about the script that wrote it or about the build:
change the script and run it again, and the file follows. The scripts read the
build's typed export through [`scripts/build_data.py`](../scripts/build_data.py),
so they need the decompilation under `work/decomp/<build>/`
([`scripts/README.md`](../scripts/README.md) says how to fetch it).

| File | Written by |
| --- | --- |
| `advance_teams.yaml` | `extract_prices.py` |
| `commander_skill_effects.yaml` | `extract-commander-skill-effects.py` |
| `commander_skills.yaml` | `extract_prices.py` |
| `constructions.yaml` | `extract-constructions.py` |
| `contraptions.yaml` | `extract-contraptions.py` |
| `economy.yaml` | `extract_prices.py` |
| `energy_tower_skill_effects.yaml` | `extract-energy-tower-skill-effects.py` |
| `equipment_effects.yaml` | `extract-equipment-effects.py` |
| `localization.yaml` | `extract_names.py` |
| `maps.yaml` | `extract-maps.py` |
| `names.yaml` | `extract_names.py` |
| `officer_effects.yaml` | `extract-officer-effects.py` |
| `officers.yaml` | `extract_prices.py` |
| `opening.yaml` | `extract_opening.py` |
| `reactor_damage.yaml` | `extract_prices.py` |
| `reinforce_items.yaml` | `extract_prices.py` |
| `reinforcements.yaml` | `extract_reinforcements.py` |
| `super_deployment.yaml` | `extract-super-deployment.py` |
| `technology_effects.yaml` | `extract-technology-effects.py` |
| `towers.yaml` | `extract-towers.py` |
| `unit_experience.yaml` | `extract_prices.py` |
| `unit_prices.yaml` | `extract_prices.py` |
| `unit_reinforcements.yaml` | `extract_prices.py` |
| `unit_techs.yaml` | `extract_prices.py` |
| `units/*.yaml` | `extract-units.py` |

Each script is under [`scripts/extract/`](../scripts/extract/), and a file
may be read by scripts other than the one that writes it. Run one with
`uv run --with pyyaml python3 scripts/extract/<script>`; a script that takes
`--check` compares instead of writing. Moving to a new game version is running
every one of them again, and the diff is what the version changed.
`scripts/check/check-docs.py` holds this table to the files: every file here
has a row, and every row names a script that exists.
