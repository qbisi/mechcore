#!/usr/bin/env python3
"""Extract the prices a supply ledger needs into `config/`.

    python3 scripts/extract_prices.py [--build BUILD]

Everything is read through `scripts/build_data.py`, the build's typed export:

* `ConfigDataContainer`: `cardDatas` (a unit's purchase and unlock price, its
  member count and the technologies it may research), `mechExpDatas` (the
  supply and experience a level costs), the officers, the openings, the unit
  reinforcement cards, the blueprints, the energy tower skills and the maps.
* `TechnologyGroupData`: every technology's `supply`, the price of one.
* `CommanderSkillGroupData` and `EquipmentGroupData`: the commander skill and
  equipment cards a reinforcement can deal, their prices and, for a skill, its
  two cooldowns.
* `ContraptionGroupData`: what releasing each contraption costs.
* `Config`: the step a technology's price rises by per technology already
  researched.

It writes `unit_techs`, `unit_prices`, `unit_experience`, `commander_skills`,
`reinforce_items`, `unit_reinforcements`, `advance_teams`, `officers` and
`economy`, all under `config/`.
"""
import pathlib
import re
import sys

import build_data

CONTRAPTION_TYPES = {10001: "shield", 20001: "missile", 30001: "interceptor"}
# `limitedScene` lists the modes an item can be offered in, and every card a
# ranked replay records carries this one.
STANDARD_SCENE = 1
# `ReinforceItemScope` of a card the reinforcement pool can deal. A blueprint's
# own skill and an opening's own officer carry other values, and no item
# carrying one was dealt in any recorded match.
DEALT_SCOPE = 1
# `ReinforceItemScope` of an officer a side can pick as its opening.
OPENING_SCOPE = 2
# The map `docs/spec/document/layout.md` treats as the 1v1 reference, whose research centre
# offers what every versus map offers.
STANDARD_MAP = 1001
ROOT = pathlib.Path(__file__).resolve().parents[1]
CATALOG = ROOT / "crates/document/src/catalog.rs"
UNIT_TECHS = ROOT / "config/unit_techs.yaml"
REINFORCE = ROOT / "config/reinforce_items.yaml"
OFFICERS = ROOT / "config/officers.yaml"
UNIT_REINFORCEMENTS = ROOT / "config/unit_reinforcements.yaml"
ADVANCE_TEAMS = ROOT / "config/advance_teams.yaml"
ECONOMY = ROOT / "config/economy.yaml"
UNIT_PRICES = ROOT / "config/unit_prices.yaml"
UNIT_EXPERIENCE = ROOT / "config/unit_experience.yaml"
COMMANDER_SKILLS = ROOT / "config/commander_skills.yaml"


def group_rows(name):
    """Every row of every list of a level0 group object, by id."""
    rows = {}
    for table in build_data.level0(name).values():
        if isinstance(table, list):
            for row in table:
                rows.setdefault(row["id"], row)
    return rows


def config_numbers(structure):
    """The match-wide supply numbers.

    `upgradeTechnologyCostIncreaseDelta` is `Config`'s. What declining an
    ordinary reinforcement pays is the standard map's `giveUpSupply`, which
    build 2.0 moved onto `MatchSetting` from `Config.noReinforcementSupply`.
    """
    config = build_data.level0("Config")
    if config["reinforceItemCount"] != 4:
        raise SystemExit(f"Config deals {config['reinforceItemCount']} offers a round, not four")
    standard = next(row for row in structure["matchSettings"] if row["id"] == STANDARD_MAP)
    return {
        "upgrade_technology_cost_increase_delta": config["upgradeTechnologyCostIncreaseDelta"],
        "reinforce_item_count": config["reinforceItemCount"],
        "no_reinforcement_supply": standard["giveUpSupply"],
    }


def contraption_prices():
    """What releasing each contraption costs, by `ContraptionData` id."""
    rows = group_rows("ContraptionGroupData")
    return {identifier: (rows[identifier]["name"], rows[identifier]["supply"])
            for identifier in sorted(CONTRAPTION_TYPES)}


def reinforce_items(group):
    """Every card of a group object: a commander skill or an equipment a reinforcement can deal."""
    rows = {}
    for row in group_rows(group).values():
        if "scope" not in row or not 100_000 <= row["id"] < 100_000_000:
            continue
        rows[row["id"]] = {
            "id": row["id"], "name": row["name"], "supply": row["supply"], "scope": row["scope"],
            "level": row["level"], "scenes": row.get("limitedScene") or [],
            # What wearing an equipment adds to its side's income each round,
            # and takes off the price of upgrading its formation.
            "round_supply": row.get("roundSupply", 0),
            "upgrade_supply": row.get("upgradeSupplyChangeValue", 0),
        }
    return rows


def commander_skill_cooldowns():
    """Every commander skill's two cooldowns, in rounds.

    `initialCoolDown` is where a slot starts when the skill joins the panel,
    and `releaseInterval` is where it goes once the skill is spent.
    """
    return {
        row["id"]: {"id": row["id"], "name": row["name"],
                    "initial_cooldown": row["initialCoolDown"], "cooldown": row["releaseInterval"]}
        for row in group_rows("CommanderSkillGroupData").values()
        if 100_000 <= row["id"] < 100_000_000 and build_data.in_standard(row)
    }


def write_commander_skills():
    rows = commander_skill_cooldowns()
    lines = ["schema: mechcore.commander_skills", f"game_build: {build_data.build()}", "",
             "# Every commander skill and its two cooldowns, in rounds.",
             "# `initial_cooldown` is `initialCoolDown`, where a slot starts when",
             "# the skill joins the panel. `cooldown` is `releaseInterval`, where it",
             "# goes when a round spends the skill. docs/rules/commander_skills.md",
             "# says how a round counts it down.",
             "", "skills:"]
    for row in sorted(rows.values(), key=lambda row: row["id"]):
        lines.append(f"  - {{id: {row['id']}, name: {yaml_scalar(row['name'])}, "
                     f"initial_cooldown: {row['initial_cooldown']}, "
                     f"cooldown: {row['cooldown']}}}")
    COMMANDER_SKILLS.write_text("\n".join(lines) + "\n")
    print(f"commander skills: {len(rows)}")


def technology_prices(identifiers):
    rows = group_rows("TechnologyGroupData")
    return {identifier: rows[identifier]["supply"] for identifier in identifiers if identifier in rows}


def unit_names():
    """The public type name of each unit ID, from the crate's own table."""
    pattern = re.compile(r"^\s+(\d+) => Some\(\(\"(\w+)\"", re.MULTILINE)
    source = CATALOG.read_text()
    section = source[source.index("pub const fn unit_type_from_id") :]
    return {
        int(match.group(1)): match.group(2)
        for match in pattern.finditer(section[: section.index("\n}")])
    }


def yaml_scalar(text):
    """Quotes a name only where flow syntax would otherwise take it apart."""
    if not text or text != text.strip() or any(
        character in text for character in ",{}[]:#&*!|>'\"%@`"
    ):
        return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'
    return text


def yaml_units(rows, body):
    lines = [f"schema: {rows}", f"game_build: {build_data.build()}", "", "units:"]
    lines.extend(body)
    return "\n".join(lines) + "\n"


# The four a layout can name, in the crate's own words.
CONSTRUCTION_TYPES = {
    1: "defensive_wall",
    2: "anti_armor_turret",
    3: "rapid_fire_turret",
    4: "magnetic_barrier",
}

GRANTS = (
    ("commander_skills", "commanderSkillIds"),
    ("equipment", "equipmentIds"),
)

MODIFIERS = (
    ("unit_supply", "supplyChangeValue"),
    ("shop_unit_level", "shopUnitLevel"),
    ("unlock_supply", "unlockSupplyChangeValue"),
    ("technology_supply", "technologySupply"),
    ("upgrade_supply", "upgradeSupplyChangeValue"),
    ("round_supply", "roundSupply"),
    ("first_round_supply", "firstRoundSupply"),
    ("granted_supply", "addSupply"),
    ("kill_bounty", "destroyHugeMechSupply"),
)


# The multiplier of each level's experience over the first level's, in
# `docs/rules/unit_experience.md`. They are fitted rather than read: each is a
# value every standard row admits once rounded half up.
EXPERIENCE_FACTORS = ("1", "2.25321", "3.5618", "4.49028", "5.21046", "5.79888",
                      "6.29639", "6.72735")


def write_unit_experience(names, levels):
    """Writes each unit's experience per level, and checks the formula on it.

    The table is read verbatim. The formula is only checked, so a row that
    breaks it is still written as the build has it, and named here.
    """
    from decimal import Decimal, ROUND_HALF_UP

    factors = [Decimal(factor) for factor in EXPERIENCE_FACTORS]
    lines = ["schema: mechcore.unit_experience", f"game_build: {build_data.build()}", "",
             "# `upgrade_exp` is `mechExpDatas.upgradeLv2` through `upgradeLv9`. Its",
             "# n-th entry fills the bar of a formation at level n; level 9 fills",
             "# at the last entry. docs/rules/unit_experience.md states why, and the",
             "# formula the rows follow.",
             "", "units:"]
    breaking = []
    for unit_id in sorted(names):
        row = levels.get(unit_id)
        if row is None:
            continue
        table = [row[f"upgradeLv{level}"] for level in range(2, 10)]
        predicted = [int((table[0] * factor).quantize(Decimal(1), ROUND_HALF_UP))
                     for factor in factors]
        if predicted != table:
            breaking.append(names[unit_id])
        values = ", ".join(str(value) for value in table)
        lines.append(f"  - {{type: {names[unit_id]}, unit_id: {unit_id}, "
                     f"upgrade_exp: [{values}]}}")
    UNIT_EXPERIENCE.write_text("\n".join(lines) + "\n")
    print(f"units whose experience breaks the formula: {breaking}")


def write_unit_reinforcements(structure, by_level):
    """The cards that hand a side units, and what they hand it.

    `unitID` names one unit per squad, and no card in this build mixes two, so
    a row states the unit, how many squads of it, and the level they arrive at.
    """
    lines = ["schema: mechcore.unit_reinforcements", f"game_build: {build_data.build()}", "",
             "# A card that hands out units: which unit, how many squads of it,",
             "# the level they arrive at, and the first round it can be offered.",
             "", "cards:"]
    count = 0
    for row in sorted(structure["unitReinforceDatas"], key=lambda row: row["id"]):
        scenes = row.get("limitedScene") or []
        if scenes and STANDARD_SCENE not in scenes:
            continue
        squads = row["unitID"]
        if len(set(squads)) != 1:
            raise SystemExit(f"card {row['id']} hands out more than one kind of unit")
        supply = row.get("supply", 0)
        count += 1
        lines.append(
            f"  - {{id: {row['id']}, name: {yaml_scalar(row.get('name') or '')}, "
            f"supply: {by_level.get(row.get('level'), 0) if supply < 0 else supply}, "
            f"unit: {squads[0]}, squads: {len(squads)}, "
            f"level: {row['extraUnitLevel']}, from_round: {row['activeRound']}}}")
    UNIT_REINFORCEMENTS.write_text("\n".join(lines) + "\n")
    print(f"unit reinforcement cards: {count}")


def write_advance_teams(structure):
    """What a side can pick in round 0, and what picking it does.

    Two kinds share the choice: a team of units, and a specialist officer. Both
    move the reactor core, which is the price of the stronger openings.
    """
    lines = ["schema: mechcore.advance_teams", f"game_build: {build_data.build()}", "",
             "# The round 0 opening. A `units` row hands out that force, and the",
             "# round 1 roster of every side in the local set is exactly one of",
             "# them. An `officer` row grants the officer its own ID names, whose",
             "# effects config/officers.yaml states. Either way the reactor core",
             "# moves by the stated amount, which is the only price an opening",
             "# has: none of them costs supply.",
             "", "teams:"]
    teams = specialists = 0
    for row in sorted(structure["advanceTeamDatas"], key=lambda row: row["id"]):
        scenes = row.get("limitedScene") or []
        if scenes and STANDARD_SCENE not in scenes:
            continue
        teams += 1
        units = ", ".join(str(unit) for unit in row["units"])
        core = f", reactor_core: {row['reactorCore']}" if row.get("reactorCore") else ""
        lines.append(f"  - {{id: {row['id']}, name: {yaml_scalar(row.get('name') or '')}, "
                     f"kind: units, units: [{units}]{core}}}")
    for row in sorted(structure["officerDatas"], key=lambda row: row["id"]):
        if row.get("scope") != OPENING_SCOPE:
            continue
        scenes = row.get("limitedScene") or []
        if scenes and STANDARD_SCENE not in scenes:
            continue
        specialists += 1
        core = f", reactor_core: {row['reactorCore']}" if row.get("reactorCore") else ""
        lines.append(f"  - {{id: {row['id']}, name: {yaml_scalar(row.get('name') or '')}, "
                     f"kind: officer{core}}}")
    ADVANCE_TEAMS.write_text("\n".join(lines) + "\n")
    print(f"openings: {teams} teams and {specialists} specialists")


def write_officers(structure):
    """The officers that change what a side pays or earns."""
    lines = ["schema: mechcore.officers", f"game_build: {build_data.build()}", "",
             "# An officer that changes a price or an income, and the units its",
             "# discount applies to. An empty scope applies to every unit.",
             "", "officers:"]
    count = 0
    for row in sorted(structure["officerDatas"], key=lambda row: row["id"]):
        if not build_data.in_standard(row):
            continue
        present = [(name, row[field]) for name, field in MODIFIERS if row.get(field)]
        granted = [(name, row[field]) for name, field in GRANTS if row.get(field)]
        opening = row.get("extraUnitLevel") and row.get("unitID")
        if not present and not granted and not opening:
            continue
        count += 1
        lines.append(f"  - id: {row['id']}")
        lines.append(f"    name: {yaml_scalar(row.get('name') or '')}")
        for name, value in present:
            lines.append(f"    {name}: {value}")
        for name, value in granted:
            lines.append(f"    {name}: [{', '.join(str(item) for item in value)}]")
        # An officer that hands something out names the round it does so, and
        # the round is absolute rather than counted from its arrival. Longbow
        # Specialist reads "在第2回合免费获得1个3级长弓" and carries
        # `activeRound: 2`, while Rhino Specialist waits until 4. Only an
        # officer with something to hand out states one.
        if (granted or opening) and row.get("activeRound"):
            lines.append(f"    active_round: {row['activeRound']}")
        if opening:
            # `unitUnlockRound` is a second round, when the unit joins the shop.
            # Every specialist in this build unlocks in round 1 and hands its
            # squad out later.
            lines.append(f"    opening_unit: {{unit: {row['unitID'][0]}, "
                         f"level: {row['extraUnitLevel']}, "
                         f"unlock_round: {row.get('unitUnlockRound', 0)}}}")
        scope = row.get("unitID") or []
        if scope and any(name in ("unit_supply", "unlock_supply", "upgrade_supply",
                                  "shop_unit_level", "technology_supply")
                         for name, _ in present):
            lines.append(f"    units: [{', '.join(str(unit) for unit in scope)}]")
    OFFICERS.write_text("\n".join(lines) + "\n")
    print(f"officers that change a price or an income: {count}")


def write_economy(structure, contraptions, config):
    """What the towers, the blueprints and the energy tower charge."""
    lines = ["schema: mechcore.economy", f"game_build: {build_data.build()}", "",
             "# Prices a round can pay that belong to no unit.",
             "",
             "# The blueprints a standard match can reach. A map lists what its",
             "# research centre offers, `PrepareBlueprint` drops every row that",
             "# needs research because no standard rule enables it, and a chain's",
             "# second level is reached by activating its first.",
             "", "blueprints:"]
    catalogue = {row["id"]: row for row in structure["blueprints"]}
    standard = next(row["blueprints"] for row in structure["matchSettings"]
                    if row["id"] == STANDARD_MAP)
    offered = {identifier for identifier in standard
               if not catalogue[identifier].get("researchTime")}
    offered |= {catalogue[identifier]["nextID"] for identifier in list(offered)
                if catalogue[identifier].get("nextID")}
    for row in sorted((catalogue[identifier] for identifier in offered),
                      key=lambda row: row["id"]):
        # A chain blueprint's mapID names the officer it produces; any other
        # blueprint's names the commander skill it puts on the panel.
        grants = ("grants_officer" if row.get("bpType") == 1 else "grants_skill")
        lines.append(f"  - {{id: {row['id']}, name: {yaml_scalar(row.get('name') or '')}, "
                     f"supply: {row.get('supply', 0)}, {grants}: {row.get('mapID', 0)}}}")
    lines += ["", "tower_strengthen:"]
    for row in sorted(structure["towerStrengthenDatas"], key=lambda row: row["level"]):
        if not build_data.in_standard(row):
            continue
        lines.append(f"  - {{level: {row['level']}, supply: {row.get('supply', 0)}}}")
    lines += ["", "# `supply` is what activating costs, `granted` what it pays back",
              "# at once, and `owed` what it takes from the next round's income.",
              "# `shop_unit_level` is how much higher a unit bought after it",
              "# arrives, which is a price as well as a level.",
              "energy_tower_skills:"]
    for row in sorted(structure["energyTowerSkillDatas"], key=lambda row: row["id"]):
        level = row.get("shopUnitLevelChangeValue", 0)
        raised = f"shop_unit_level: {level}, " if level else ""
        lines.append(
            f"  - {{id: {row['id']}, name: {yaml_scalar(row.get('name') or '')}, "
            f"supply: {row.get('supply', 0)}, granted: {row.get('supplyChangeValue', 0)}, "
            f"{raised}owed: {-row.get('nextRoundSupplyChangeValue', 0)}}}")
    lines += ["", "# What each technology already researched on a unit adds to the",
              "# next one's price. `UnitUtility.CalculateUpgradeTechnologyCost`",
              "# computes a technology's price as this times the count already",
              "# active plus the technology's own supply, capped by the unit's",
              "# `techUpgradeMaxSupplyLimit` when that is above zero. The step it",
              "# uses is the unit's own `techUpgradeIncreaseSupplyPerCount`, and",
              "# when that is zero or less it falls back to the match-wide",
              "# `Config.upgradeTechnologyCostIncreaseDelta`. Every standard unit",
              "# ships zero, so every standard match uses the fallback, which is",
              "# what this reads.",
              f"technology_repeat_step: {config['upgrade_technology_cost_increase_delta']}"]
    lines += ["", "# What declining an ordinary round's reinforcement pays; a unit",
              "# round pays its pool's `decline_supply` in reinforcements.yaml",
              "# instead. Declining is",
              "# itself an item: `ReinforcementManager.GetGiveUpReinforce`",
              "# returns an `AddSupplyReinforceItem` built per round rather than",
              "# read from a card table. This is the standard map's",
              "# `MatchSetting.giveUpSupply` (`Config.noReinforcementSupply` before",
              "# build 2.0), a shipped field whose name and value match what a",
              "# decline pays. The getter is inlined where it is read, so the",
              "# decompilation records no call to cite: the match is by name and",
              "# value rather than by call chain.",
              f"reinforce_decline: {config['no_reinforcement_supply']}"]
    lines += ["", "# What releasing a contraption costs. The config data container",
              "# does not carry these; they come from the `constraptionDatas`",
              "# object of `level0`.",
              "contraptions:"]
    for identifier, (name, supply) in sorted(contraptions.items()):
        lines.append(f"  - {{type: {CONTRAPTION_TYPES[identifier]}, id: {identifier}, "
                     f"name: {yaml_scalar(name)}, supply: {supply}}}")
    lines += ["", "# What recovering a construction pays back. Unlike a unit's,",
              "# this is a property of the construction and not of its history.",
              "constructions:"]
    seen = set()
    for row in sorted(structure["constructionDatas"], key=lambda row: row["id"]):
        name = CONSTRUCTION_TYPES.get(row["id"])
        if name is None or name in seen:
            continue
        seen.add(name)
        lines.append(f"  - {{type: {name}, id: {row['id']}, "
                     f"recovers: {row.get('sellSupply', 0)}}}")
    # Every map a standard match plays hands out the same income, so the rule
    # is one row rather than one per map. The maps that differ are the survival
    # ones, which cap at 2000, and two others that cap at 1400; this format
    # does not describe either.
    standard_supply = next(row for row in structure["matchSettings"]
                           if row["id"] == STANDARD_MAP)
    versus = [row for row in structure["matchSettings"]
              if 1000 <= row["id"] < 3000 and row["id"] not in (1051, 2051)
              and row["serverSubType"] not in build_data.EXPEDITION_SCENES]
    fields = ("firstRoundSupply", "roundSupplyIncreaseValue", "maxRoundSupply")
    for row in versus:
        if any(row[field] != standard_supply[field] for field in fields):
            raise SystemExit(f"map {row['id']} pays out differently: {row['name']}")
    lines += ["",
              f"# The income rule every versus map shares, checked across the",
              f"# {len(versus)} of them. A survival map caps at 2000 instead, and",
              "# the two roguelike maps at 1400.",
              "round_supply:",
              f"  first: {standard_supply['firstRoundSupply']}",
              f"  increase: {standard_supply['roundSupplyIncreaseValue']}",
              f"  max: {standard_supply['maxRoundSupply']}"]
    lines += ["", "# A card with no price of its own is sold at its level's price.",
              "reinforce_levels:"]
    for row in sorted(structure["reinforceItemPrices"], key=lambda row: row["level"]):
        lines.append(f"  - {{level: {row['level']}, supply: {row.get('price', 0)}}}")
    ECONOMY.write_text("\n".join(lines) + "\n")


def main():
    build_data.arguments(__doc__)
    structure = build_data.container()
    cards = {row["id"]: row for row in structure["cardDatas"]}
    levels = {row["id"]: row for row in structure["mechExpDatas"]}
    names = unit_names()

    wanted = sorted({tech for unit in names if unit in cards for tech in cards[unit]["technologies"]})
    prices = technology_prices(wanted)
    missing = [tech for tech in wanted if tech not in prices]
    print(f"technologies the named units may research: {len(wanted)}, priced: {len(prices)}")
    if missing:
        raise SystemExit(f"technologies with no row: {missing}")

    techs, economy = [], []
    for unit_id in sorted(names):
        card = cards.get(unit_id)
        if card is None:
            continue
        techs.append(f"  - type: {names[unit_id]}")
        techs.append(f"    unit_id: {unit_id}")
        techs.append("    technologies:")
        for tech in sorted(card["technologies"]):
            techs.append(f"      - {{id: {tech}, supply: {prices[tech]}}}")
        economy.append(f"  - type: {names[unit_id]}")
        economy.append(f"    unit_id: {unit_id}")
        economy.append(f"    supply: {card['baseMoney']}")
        economy.append(f"    members: {card['mechCount']}")
        if card["unlockPrice"]:
            economy.append(f"    unlock_supply: {card['unlockPrice']}")
        economy.append(f"    upgrade_supply: {levels[unit_id]['upgradeSupplyLv2']}")

    # A card with no price of its own is sold at its level's price.
    by_level = {row["level"]: row.get("price", 0)
                for row in structure["reinforceItemPrices"]}
    items = reinforce_items("CommanderSkillGroupData")
    equipment = reinforce_items("EquipmentGroupData")
    equipment_ids = set(equipment)
    items.update(equipment)
    for row in items.values():
        if row["supply"] < 0:
            row["supply"] = by_level.get(row["level"], 0)
    # An empty `limitedScene` restricts nothing, which is how Field Recovery
    # and its weaker twin are listed. `scope` says whether the reinforcement
    # pool can offer the card at all: a skill a blueprint researches, or an
    # officer that belongs to an opening, carries another value and is never
    # dealt.
    offered = {row["id"]: row for row in items.values()
               if (not row["scenes"] or STANDARD_SCENE in row["scenes"])
               and row["scope"] == DEALT_SCOPE}
    print(f"cards in the two groups: {len(items)}, offered in standard play: {len(offered)}")
    for identifier in offered:
        offered[identifier]["kind"] = (
            "equipment" if identifier in equipment_ids else "commander_skill"
        )
    for row in structure["officerDatas"]:
        scenes = row.get("limitedScene") or []
        if scenes and STANDARD_SCENE not in scenes:
            continue
        if row.get("scope") != DEALT_SCOPE:
            continue
        supply = row.get("supply", 0)
        offered.setdefault(row["id"], {
            "id": row["id"],
            "name": row.get("name") or "",
            "supply": by_level.get(row.get("level"), 0) if supply < 0 else supply,
            "kind": "officer",
        })
    lines = ["schema: mechcore.reinforce_items", f"game_build: {build_data.build()}", "",
             "# Every card a standard match can offer, and what taking it costs.",
             "# A card whose limitedScene names other modes only is left out; an",
             "# empty list restricts nothing. A card with no price of its own is",
             "# sold at its level's price, which reinforce_levels in economy.yaml",
             "# states.",
             "#",
             "# Every card here grants the thing its own ID names. A unit card",
             "# and an advance team hand out units instead, and each has its",
             "# own table. A skill a blueprint researches and an officer that",
             "# belongs to an opening are left out: the pool never deals them.",
             "", "items:"]
    for row in sorted(offered.values(), key=lambda row: row["id"]):
        # An equipment may change an amount rather than a stat: what it adds to
        # a side's income each round, or takes off its formation's upgrades.
        extra = "".join(
            f", {name}: {row[name]}"
            for name in ("round_supply", "upgrade_supply")
            if row.get(name)
        )
        lines.append(f"  - {{id: {row['id']}, name: {yaml_scalar(row['name'])}, "
                     f"kind: {row['kind']}, supply: {row['supply']}{extra}}}")
    REINFORCE.write_text("\n".join(lines) + "\n")
    print(f"cards a standard match can offer: {len(offered)}")

    write_commander_skills()
    write_unit_reinforcements(structure, by_level)
    write_advance_teams(structure)
    write_officers(structure)
    write_economy(structure, contraption_prices(), config_numbers(structure))

    UNIT_TECHS.write_text(yaml_units("mechcore.unit_techs", techs))
    UNIT_PRICES.write_text(yaml_units("mechcore.unit_prices", economy))
    sold = {row["id"] for row in structure["cardDatas"]}
    write_unit_experience({unit: name for unit, name in names.items() if unit in sold},
                          levels)
    for path in (UNIT_TECHS, UNIT_PRICES, UNIT_EXPERIENCE, COMMANDER_SKILLS, REINFORCE,
                 UNIT_REINFORCEMENTS, ADVANCE_TEAMS, OFFICERS, ECONOMY):
        print(f"wrote {path.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
