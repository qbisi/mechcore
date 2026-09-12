#!/usr/bin/env python3
"""Extract the build-2259 unit and technology prices into `config/`.

The prices a supply ledger needs live in two Unity objects of `level0`:

* `ConfigDataContainer` at path id 160 carries `cardDatas` (a unit's purchase
  price, its shop unlock price, its member count and the technologies it may
  research; its `defaultTechnologies` are what a new account may unlock
  without paying, which is an account rule rather than a match one) and
  `mechExpDatas` (the cost of one level). It is read here from the JSON export
  in this directory, because reading it back out of the asset needs the type
  tree that a dummy-DLL build supplies.
* `TechnologyGroupData` at path id 184 carries every `TechnologyData`, and its
  `supply` field is the price of one technology. No type tree is needed: the
  entries are parsed directly, since Unity serializes a MonoBehaviour's fields
  base class first and in declaration order, and the declaration order is the
  decompiled one.

A technology entry begins with its ID and its name, and the fields up to the
price are four strings and four small integers:

    id, name, isTestData, iconName, description, descParams, story,
    targetSkillID, mainSkillEffect, extraSkillEffect,
    extraSkillNumericalEffect, supply

Every parse is checked against `docs/rules/unit_techs.md`, whose base supply column
was read out of build 2227 by other means, so a silent misparse would have to
agree with an independent extraction of an earlier build to pass.

    work/tools/asset-venv/bin/python scripts/extract_prices.py \\
        "<Mechabellum.app>/Contents/Resources/Data/level0"
"""
import json
import pathlib
import re
import struct
import sys

TECHNOLOGY_GROUP_PATH_ID = 184
CONFIG_PATH_ID = 136
COMMANDER_SKILL_PATH_ID = 167
EQUIPMENT_PATH_ID = 188
# The three contraptions a side can release, which the config data container
# does not carry. The object spells its own field `constraptionDatas`.
CONTRAPTION_PATH_ID = 159
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
CONFIG_JSON = ROOT / "work/inputs/config-data-container-build2259.json"
CATALOG = ROOT / "crates/document/src/catalog.rs"
UNIT_TECHS = ROOT / "config/unit_techs.yaml"
REINFORCE = ROOT / "config/reinforce_items.yaml"
OFFICERS = ROOT / "config/officers.yaml"
UNIT_REINFORCEMENTS = ROOT / "config/unit_reinforcements.yaml"
ADVANCE_TEAMS = ROOT / "config/advance_teams.yaml"
ECONOMY = ROOT / "config/economy.yaml"
UNIT_PRICES = ROOT / "config/unit_prices.yaml"
DOC = ROOT / "docs/rules/unit_techs.md"
BUILD = "1.11.1.3.2259"


def blobs(level0, path_ids):
    import UnityPy

    environment = UnityPy.load(str(level0))
    found = {}
    for obj in environment.objects:
        if obj.type.name == "MonoBehaviour" and obj.path_id in path_ids:
            found[obj.path_id] = obj.get_raw_data()
    for path_id in path_ids:
        if path_id not in found:
            raise SystemExit(f"{level0} has no MonoBehaviour {path_id}")
    return found


# Where `GameRiver.Config`'s two match-wide supply numbers sit in its
# serialized body. Unity writes a MonoBehaviour's own fields after the header
# in declaration order, and the declaration order is the decompiled one:
# `isClassicSurviveMode` as a padded bool, then the technology delta, then five
# eight-byte `FPoint`s, two ints, another `FPoint`, two twelve-byte
# `MechPositionChangeData`s, a twenty-byte `BuyUnitEffectData`, and the three
# reinforcement ints. `mineFlyHeight` is private and does not serialize.
CONFIG_FIELDS = {
    "upgrade_technology_cost_increase_delta": 4,
    "reinforce_item_count": 108,
    "no_reinforcement_supply": 116,
}


def config_numbers(blob):
    """The match-wide supply numbers `GameRiver.Config` carries.

    `reinforce_item_count` is read only to check the parse. Every round of
    every tracked replay deals exactly that many offers, so a body parsed at
    the wrong offset would have to put the right number in the right place by
    accident to pass.
    """
    body = monobehaviour_body(blob)
    values = {
        name: struct.unpack_from("<i", body, offset)[0]
        for name, offset in CONFIG_FIELDS.items()
    }
    if values["reinforce_item_count"] != 4:
        raise SystemExit(
            f"Config parses reinforce_item_count as {values['reinforce_item_count']}, "
            "and every tracked round deals four offers; the field offsets are wrong"
        )
    return values


def monobehaviour_body(blob):
    """One MonoBehaviour's own fields, past the header Unity writes first.

    The header is the owning GameObject pointer, the enabled flag, the script
    pointer and the name, and the name is followed to the next four-byte
    boundary.
    """
    offset = 28
    (length,) = struct.unpack_from("<i", blob, offset)
    offset += 4 + length
    return blob[(offset + 3) & ~3:]


def contraption_prices(blob):
    """What releasing each contraption costs.

    An entry is a `ContraptionData`, which is an `ItemData` followed by the
    price. Each one is found by its own ID rather than by sweeping, because
    there are three of them and the three subclasses put different fields after
    the price.
    """
    prices = {}
    for identifier in sorted(CONTRAPTION_TYPES):
        offset = blob.find(struct.pack("<ii", 1, identifier))
        if offset < 0:
            raise SystemExit(f"contraption {identifier} is absent from {CONTRAPTION_PATH_ID}")
        cursor = offset + 8
        name, cursor = read_string(blob, cursor)
        cursor += 4  # isTestData
        for _ in range(3):  # iconName, description, descParams
            _, cursor = read_string(blob, cursor)
        # One int stands between the last string and the price in every entry.
        _, supply = struct.unpack_from("<2i", blob, cursor)
        if supply <= 0 or supply % 50:
            raise SystemExit(f"contraption {identifier} priced {supply}, which is not a price")
        prices[identifier] = (name.decode("utf-8"), supply)
    return prices


def parse_reinforce_item(blob, offset):
    """Reads one `ReinforceItemData`, the base every drawable card shares.

    After the four `ItemData` strings it declares the card's level, its scope,
    its price, two more strings, a reactor core change, the modes it may be
    offered in, its round window and whether it can repeat.
    """
    if offset + 8 > len(blob):
        return None
    (identifier,) = struct.unpack_from("<i", blob, offset)
    name, cursor = read_string(blob, offset + 4)
    if not name:
        return None
    try:
        name = name.decode("utf-8")
    except UnicodeDecodeError:
        return None
    cursor += 4  # isTestData
    for _ in range(4):  # iconName, description, descParams, story
        text, cursor = read_string(blob, cursor)
        if text is None:
            return None
    if cursor + 12 > len(blob):
        return None
    level, scope, supply = struct.unpack_from("<3i", blob, cursor)
    cursor += 12
    pictures = []
    for _ in range(2):  # reinforcePicName, advancePicName
        text, cursor = read_string(blob, cursor)
        if text is None:
            return None
        pictures.append(text)
    # Every drawable card names the art its drop is shown with. Requiring it
    # keeps the sweep from mistaking an icon name or a parameter string for an
    # entry of its own.
    if not pictures[0] or not pictures[0].isascii():
        return None
    if cursor + 8 > len(blob):
        return None
    cursor += 4  # reactorCore
    (count,) = struct.unpack_from("<i", blob, cursor)
    cursor += 4
    if not 0 <= count < 32 or cursor + 4 * count > len(blob):
        return None
    scenes = list(struct.unpack_from(f"<{count}i", blob, cursor))
    # Every price in this build is a multiple of fifty, and -1 stands for the
    # level's own price, so anything else is a misread rather than a card.
    if not (0 <= level <= 6 and 0 <= scope <= 4):
        return None
    if supply != -1 and not (0 <= supply <= 2000 and supply % 50 == 0):
        return None
    if not (len(scenes) <= 12 and all(0 <= scene <= 15 for scene in scenes)):
        return None
    row = {"id": identifier, "name": name, "supply": supply, "scope": scope,
           "level": level, "scenes": scenes}
    row.update(equipment_prices(blob, cursor + 4 * count))
    return row


def equipment_prices(blob, cursor):
    """The two amounts an equipment changes rather than a stat.

    `roundSupply` is what wearing it adds to its side's income each round, and
    `upgradeSupplyChangeValue` is what it takes off the price of upgrading the
    formation wearing it. Only `EquipmentData` carries either, and one item
    uses each: Command Core pays 50 a round, Upgrade Kit discounts 100.

    Both sit past the shared base, so this walks the subclass's own declaration
    order: three flags, two integers, two fixed-point rates, a speed, eight more
    fixed-point fields, a flag, and the two lists that say which units the item
    may go on. Anything that does not read as a price is treated as absent,
    since the sweep also lands on commander skills.
    """
    def price(value, low, high):
        return value if low <= value <= high and value % 50 == 0 else 0

    try:
        # The rest of the shared base: two round bounds and the repeat flag.
        cursor += 4 * 3
        cursor += 4 * 3                                  # the three effect flags
        _, round_supply = struct.unpack_from("<2i", blob, cursor)
        cursor += 4 * 2 + 8 * 2 + 4 + 8 * 8 + 4
        for _ in range(2):  # mechType, unitID
            (count,) = struct.unpack_from("<i", blob, cursor)
            if not 0 <= count < 64:
                return {}
            cursor += 4 + 4 * count
        _, _, upgrade = struct.unpack_from("<3i", blob, cursor)
    except struct.error:
        return {}
    return {"round_supply": price(round_supply, 0, 500),
            "upgrade_supply": price(upgrade, -1000, 0)}


def reinforce_items(blob):
    """Every card in one catalogue object, found by sweeping for a valid entry."""
    rows = {}
    for offset in range(0, len(blob) - 4, 4):
        row = parse_reinforce_item(blob, offset)
        # Both catalogues number their entries above a hundred thousand, which
        # rules out a stray four or five digit integer that happens to be
        # followed by two readable strings.
        if row and 100_000 <= row["id"] < 100_000_000:
            rows.setdefault(row["id"], row)
    return rows


def read_string(blob, offset):
    """Unity writes a string as its length, its bytes, then padding to four."""
    if offset < 0 or offset + 4 > len(blob):
        return None, None
    (length,) = struct.unpack_from("<i", blob, offset)
    if not 0 <= length < 4096 or offset + 4 + length > len(blob):
        return None, None
    end = offset + 4 + length
    return blob[offset + 4 : end], end + (-end) % 4


def parse_technology(blob, offset):
    """Reads one `TechnologyData` whose ID starts at `offset`."""
    (identifier,) = struct.unpack_from("<i", blob, offset)
    name, cursor = read_string(blob, offset + 4)
    if not name:
        return None
    try:
        name = name.decode("utf-8")
    except UnicodeDecodeError:
        return None
    cursor += 4  # isTestData, one byte padded to four
    icon, cursor = read_string(blob, cursor)
    if not icon or not icon.isascii():
        return None
    for _ in range(3):  # description, descParams, story
        text, cursor = read_string(blob, cursor)
        if text is None:
            return None
        try:
            text.decode("utf-8")
        except UnicodeDecodeError:
            return None
    cursor += 4  # targetSkillID
    cursor += 12  # three bools, each one byte padded to four
    (supply,) = struct.unpack_from("<i", blob, cursor)
    return identifier, name, supply


def technology_prices(blob, identifiers):
    prices = {}
    for identifier in identifiers:
        needle = struct.pack("<i", identifier)
        start = 0
        while True:
            position = blob.find(needle, start)
            if position < 0:
                break
            parsed = parse_technology(blob, position)
            if parsed and parsed[0] == identifier and 0 <= parsed[2] <= 5000:
                prices[identifier] = parsed[2]
                break
            start = position + 1
    return prices


def documented_prices():
    """The base supply column of `docs/rules/unit_techs.md`, read from build 2227."""
    pattern = re.compile(r"^\| .(\d+). \| [^|]+ \| [^|]+ \| (\d+) \|")
    prices = {}
    for line in DOC.read_text().splitlines():
        match = pattern.match(line)
        if match:
            prices[int(match.group(1))] = int(match.group(2))
    return prices


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
    lines = [f"schema: {rows}", f"game_build: {BUILD}", "", "units:"]
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


def write_unit_reinforcements(structure, by_level):
    """The cards that hand a side units, and what they hand it.

    `unitID` names one unit per squad, and no card in this build mixes two, so
    a row states the unit, how many squads of it, and the level they arrive at.
    """
    lines = ["schema: mechcore.unit_reinforcements", f"game_build: {BUILD}", "",
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
    lines = ["schema: mechcore.advance_teams", f"game_build: {BUILD}", "",
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
    lines = ["schema: mechcore.officers", f"game_build: {BUILD}", "",
             "# An officer that changes a price or an income, and the units its",
             "# discount applies to. An empty scope applies to every unit.",
             "", "officers:"]
    count = 0
    for row in sorted(structure["officerDatas"], key=lambda row: row["id"]):
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
    lines = ["schema: mechcore.economy", f"game_build: {BUILD}", "",
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
    lines += ["", "# What declining the round's reinforcement pays. Declining is",
              "# itself an item: `ReinforcementManager.GetGiveUpReinforce`",
              "# returns an `AddSupplyReinforceItem` built per round rather than",
              "# read from a card table. This is `Config.noReinforcementSupply`,",
              "# a shipped field whose name and value both match what every",
              "# decidable decline in the local replay set pays. What is not",
              "# traced is the path from the field to the item: nothing in the",
              "# decompilation is recorded reading it, so the match is by name",
              "# and value rather than by call chain.",
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
              if 1000 <= row["id"] < 3000 and row["id"] not in (1051, 2051)]
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
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    raw = blobs(pathlib.Path(sys.argv[1]),
                (TECHNOLOGY_GROUP_PATH_ID, CONFIG_PATH_ID, COMMANDER_SKILL_PATH_ID,
                 EQUIPMENT_PATH_ID, CONTRAPTION_PATH_ID))
    blob = raw[TECHNOLOGY_GROUP_PATH_ID]
    structure = json.loads(CONFIG_JSON.read_text())["m_Structure"]
    cards = {row["id"]: row for row in structure["cardDatas"]}
    levels = {row["id"]: row for row in structure["mechExpDatas"]}
    names = unit_names()

    wanted = sorted({tech for card in cards.values() for tech in card["technologies"]})
    prices = technology_prices(blob, wanted)
    missing = [tech for tech in wanted if tech not in prices]

    documented = documented_prices()
    shared = sorted(set(documented) & set(prices))
    disagreements = [
        (tech, prices[tech], documented[tech])
        for tech in shared
        if prices[tech] != documented[tech]
    ]
    print(f"technologies in the catalogue: {len(wanted)}, priced: {len(prices)}")
    print(f"  also in docs/rules/unit_techs.md: {len(shared)}")
    print(f"  disagreeing with that build-2227 table: {len(disagreements)}")
    for tech, parsed, doc in disagreements:
        print(f"    {tech}: build 2259 {parsed}, build 2227 {doc}")
    if missing:
        print(f"  unpriced: {missing}")

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
    cards = reinforce_items(raw[COMMANDER_SKILL_PATH_ID])
    equipment = reinforce_items(raw[EQUIPMENT_PATH_ID])
    equipment_ids = set(equipment)
    cards.update(equipment)
    for row in cards.values():
        if row["supply"] < 0:
            row["supply"] = by_level.get(row["level"], 0)
    # An empty `limitedScene` restricts nothing, which is how Field Recovery
    # and its weaker twin are listed. `scope` says whether the reinforcement
    # pool can offer the card at all: a skill a blueprint researches, or an
    # officer that belongs to an opening, carries another value and is never
    # dealt.
    offered = {row["id"]: row for row in cards.values()
               if (not row["scenes"] or STANDARD_SCENE in row["scenes"])
               and row["scope"] == DEALT_SCOPE}
    print(f"reinforcement cards in the two catalogues: {len(cards)}, "
          f"offered in standard play: {len(offered)}, "
          f"priced: {sum(1 for row in offered.values() if row['supply'])}")
    # A skill or equipment card grants the thing its own ID names, so the two
    # catalogues above are told apart by which object holds the entry.
    for identifier in offered:
        offered[identifier]["kind"] = (
            "equipment" if identifier in equipment_ids else "commander_skill"
        )
    # The container carries the other kinds of card. Unit reinforcements have
    # their own table, since what they hand out is more than a price.
    for table, kind in (("officerDatas", "officer"),):
        for row in structure[table]:
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
                "kind": kind,
            })
    lines = ["schema: mechcore.reinforce_items", f"game_build: {BUILD}", "",
             "# Every card a standard match can offer, and what taking it costs.",
             "# A card whose limitedScene names other modes only is left out; an",
             "# empty list restricts nothing. A card with no price of its own is",
             "# sold at its level's price, which is 0, 50, 100 and 200 by level.",
             "#",
             "# Every card here grants the thing its own ID names. A unit card",
             "# and an advance team hand out units instead, and each has its",
             "# own table. A skill a blueprint researches and an officer that",
             "# belongs to an opening are left out: the pool never deals them.",
             "", "items:"]
    for row in sorted(offered.values(), key=lambda row: row["id"]):
        # Two items change an amount rather than a stat: Command Core pays a
        # side 50 a round, and Upgrade Kit discounts its formation's upgrades.
        extra = "".join(
            f", {name}: {row[name]}"
            for name in ("round_supply", "upgrade_supply")
            if row.get(name)
        )
        lines.append(f"  - {{id: {row['id']}, name: {yaml_scalar(row['name'])}, "
                     f"kind: {row['kind']}, supply: {row['supply']}{extra}}}")
    REINFORCE.write_text("\n".join(lines) + "\n")
    print(f"cards a standard match can offer: {len(offered)}")

    write_unit_reinforcements(structure, by_level)
    write_advance_teams(structure)
    write_officers(structure)
    write_economy(structure, contraption_prices(raw[CONTRAPTION_PATH_ID]),
                  config_numbers(raw[CONFIG_PATH_ID]))

    UNIT_TECHS.write_text(yaml_units("mechcore.unit_techs", techs))
    UNIT_PRICES.write_text(yaml_units("mechcore.unit_prices", economy))
    for path in (UNIT_TECHS, UNIT_PRICES, REINFORCE, UNIT_REINFORCEMENTS,
                 ADVANCE_TEAMS, OFFICERS, ECONOMY):
        print(f"wrote {path.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
