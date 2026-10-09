#!/usr/bin/env python3
"""Extract what a technology does to a fight into `config/technology_effects.yaml`.

`config/unit_techs.yaml` carries which technologies a unit may research and what
each costs. This carries the other half: the corrections one writes onto its
unit's numbers.

The fields are the ones `GameRiver.TechnologyData` answers
`ICommonMechDataChangeDataSource` with — the interface `OfficerData`,
`TechnologyData`, `EquipmentData` and `EnergyTowerSkillData` all implement,
which is why `config/officer_effects.yaml` has the same shape and why
`docs/rules/officer_effects.md`'s composition rule is this table's too.

**A technology's effect is a list indexed by rank.** `lifeChangeRate` and its
neighbours are `List<FPoint>` rather than one value, and Elite Marksman's
`+5` of range and `+0.25` of damage per rank arrive as nine ascending entries.
A technology whose effect does not grow carries a single entry.

The source is every list of `TechnologyGroupData` in `level0`, as
`scripts/build_data.py` reads the build's typed export; a subclass's row
(`BuffTechnologyData`, `SplashTechnologyData` and the rest) carries the same
fields. The technologies read are the ones `config/unit_techs.yaml` lists.

Every number the table states has to be in the technology's own English
description, as the build localizes it with its placeholders filled; and a
list that grows with rank has to be its first entry times the rank.

    python3 scripts/extract/extract-technology-effects.py [--build BUILD]
"""

import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import build_data  # noqa: E402
from buff_rows import SOURCE_CLIENT, SOURCE_CYCLE, SOURCE_OTHER, SOURCE_RANGE_ITEM, buff_lines  # noqa: E402

REPOSITORY = pathlib.Path(__file__).resolve().parents[2]
OUTPUT = REPOSITORY / "config/technology_effects.yaml"
UNIT_TECHS = REPOSITORY / "config/unit_techs.yaml"
ONE = 1 << 32

# The effect lists this table carries, by the build's field. A
# `speedChangeValue` is a plain integer for the same reason an officer's is:
# the build keeps it in `DataSet.intDatas`.
LISTS = (
    ("life_rate", "lifeChangeRate"),
    ("damage_rate", "damageChangeRate"),
    ("speed_value", "speedChangeValue"),
    ("min_attack_range_value", "minAttackRangeChangeValue"),
    ("attack_range_value", "attackRangeChangeValue"),
    ("attack_range_rate", "attackRangeChangeRate"),
    ("attack_interval_value", "attackIntervalChangeValue"),
    ("attack_interval_rate", "attackIntervalChangeRate"),
    ("splash_range_value", "splashRangeChangeValue"),
    ("projectile_speed_value", "projectileSpeedChangeValue"),
    ("projectile_life_rate", "projectileLifeChangeRate"),
    ("exp_rate", "expChangeRate"),
)
# What a subclass's row answers its own interface with, by the build's field
# and the list of `TechnologyGroupData` whose rows carry it: a rank list, as
# the corrections are, written only on the rows of that list.
SUBCLASS_LISTS = (
    ("lifesteal_multiplier", "lifestealMultiplier", "lifestealTechnologies"),
    ("recovery_duration", "recoveryDuration", "autoRecoveryTechnologies"),
    ("recovery_life_rate", "recoveryLifeRate", "autoRecoveryTechnologies"),
    ("reduce_damage_value", "reduceDamageValue", "armorStrengthenTechnologyDatas"),
    ("air_damage_change_rate", "airDamageChangeRate", "searchTargetSpecificDatas"),
    ("ground_damage_change_rate", "groundDamageChangeRate", "searchTargetSpecificDatas"),
    ("air_damage_change_rate", "airDamageChangeRate", "damageIntensifyTechnologies"),
    ("ground_damage_change_rate", "groundDamageChangeRate", "damageIntensifyTechnologies"),
    ("splash_range", "range", "splashTechnologies"),
    ("projectile_count_value", "countIncrease", "multiAttackTechnologies"),
    ("projectile_duration_value", "durationChangeValue", "multiAttackTechnologies"),
    ("projectile_random_range_value", "randomRangeChangeValue", "multiAttackTechnologies"),
    ("dead_line_value", "deadLineValue", "deadLineTechDatas"),
    ("share_distance", "distance", "damageShareTechnologies"),
    ("barrier_energy", "shieldValues", "advancedEnergyShieldTechnologies"),
    ("barrier_radius", "radius", "advancedEnergyShieldTechnologies"),
)
# The same for a field that is one value rather than a rank list.
SUBCLASS_SCALARS = (
    ("start_time", "startTime", "autoRecoveryTechnologies"),
    ("auto_recovery_state_type", "autoRecoveryStateType", "autoRecoveryTechnologies"),
    ("sweep_skill_id", "skillID", "sweepSkillIntensifyTechDatas"),
    ("sweep_width_value", "widthChangeValue", "sweepSkillIntensifyTechDatas"),
    ("sweep_length_value", "lengthChangeValue", "sweepSkillIntensifyTechDatas"),
    ("sweep_perpendicular", "isAttackDirectionPerpendicular", "sweepSkillIntensifyTechDatas"),
    ("sweep_reverse", "isReverse", "sweepSkillIntensifyTechDatas"),
    ("sweep_fixed_direction", "isDiableDirectionChange", "sweepSkillIntensifyTechDatas"),
    ("air_target_score_offset", "airTargetScoreOffset", "searchTargetSpecificDatas"),
    ("ground_target_score_offset", "groundTargetScoreOffset", "searchTargetSpecificDatas"),
    ("extra_skill_effect", "extraSkillEffect", "airAttackTechnologyDatas"),
    ("secondary_damage", "damage", "secondaryDamageIntensifyTechDatas"),
    ("secondary_splash_range", "splashRange", "secondaryDamageIntensifyTechDatas"),
    ("secondary_hits_main_target", "canMainTargetBeHit", "secondaryDamageIntensifyTechDatas"),
    ("secondary_buffed", "canBeAffectedByBuff", "secondaryDamageIntensifyTechDatas"),
    ("secondary_disables_technology", "canDisableTech", "secondaryDamageIntensifyTechDatas"),
    ("secondary_buff_id", "hitEMPBuffID", "secondaryDamageIntensifyTechDatas"),
    ("dead_line_ignores_shield", "ignoreEnergyShield", "deadLineTechDatas"),
    ("exit_time_rate", "exitTimeChangeRate", "moveAbilityAttackIntensifyTechDatas"),
    ("strike_trigger_count", "triggerCount", "moveAbilityAttackIntensifyTechDatas"),
    ("strike_damage_rate", "damageChangeRateInCondition", "moveAbilityAttackIntensifyTechDatas"),
    ("strike_splash_range", "splashRangeChange", "moveAbilityAttackIntensifyTechDatas"),
    ("strike_attack_point", "attackPointChange", "moveAbilityAttackIntensifyTechDatas"),
    ("range_item_time", "moveAbilityTimeType", "moveAbilityRangeItemTechDatas"),
    ("range_item_move_type", "moveAbilityType", "moveAbilityRangeItemTechDatas"),
    ("range_item_range", "rangeItemRange", "moveAbilityRangeItemTechDatas"),
    ("range_item_life_time", "lifeTime", "moveAbilityRangeItemTechDatas"),
    ("fog_attack_range_rate", "FogAttackRangeChangeRate", "moveAbilityRangeItemTechDatas"),
    ("reduce_damage_from_remote", "reduceDamageFromRemote", "moveAbilityRangeItemTechDatas"),
    ("group_purpose", "mechGroupPurpose", "damageShareTechnologies"),
    ("group_damage_rate", "floatRateValue", "damageShareTechnologies"),
    ("group_max_count", "maxCount", "damageShareTechnologies"),
    ("main_skill_effect", "mainSkillEffect", "damageShareTechnologies"),
    ("extra_skill_effect", "extraSkillEffect", "damageShareTechnologies"),
    ("reactive_armor_rate", "damageReduceRate", "reactiveArmorTechDatas"),
    ("reactive_armor_count", "damageReduceCount", "reactiveArmorTechDatas"),
)
# A field of one list's rows that is one rate, written only where it is set:
# an extra weapon's `allWeaponReduceDamageRate`, which
# `ExtraSkillProvider.EnableEffect` writes on the main skill as its
# `DamageReduceRateBase`.
SET_SCALARS = (
    ("all_weapon_reduce_damage_rate", "allWeaponReduceDamageRate", "extraWeaponTechnologies"),
)
# A flag every row carries, written only where it is set: whether the row
# turns its unit's skills' locking of their target over, which
# `SkillDataModifier.AddData` writes into each skill's
# `SkillDataChangeInt.IsLockTarget`.
FLAGS = (
    ("inverse_lock_target", "isInverseIsLockTarget"),
)
# The lists whose rows say in `special` what they set beyond the fields
# this table carries: the plain one, and each subclass's the simulator reads.
IMPLEMENTED = ("technologyDatas", "lifestealTechnologies", "autoRecoveryTechnologies",
               "energyShieldTechnologies", "sweepSkillIntensifyTechDatas",
               "armorStrengthenTechnologyDatas", "searchTargetSpecificDatas",
               "airAttackTechnologyDatas", "damageIntensifyTechnologies",
               "secondaryDamageIntensifyTechDatas", "buffTechnologies",
               "interceptMissileTechnologyDatas", "splashTechnologies",
               "mobilityIntensifyTechnologies", "multiAttackTechnologies", "stealthTechData",
               "deadLineTechDatas", "moveAbilityAttackIntensifyTechDatas",
               "moveAbilityRangeItemTechDatas",
               "damageShareTechnologies", "advancedEnergyShieldTechnologies",
               "reactiveArmorTechDatas", "siegeModeTechDatas")
# The list whose `BuffTech` adds a buff, and the fields its rows carry for
# `buff_lines` rather than as corrections.
BUFF = "buffTechnologies"
BUFF_SOURCE = {"buffID", "buffTechTrigger", "effectTargetTypes", "probability", "energyShieldDamage",
               "triggerRangeItemBuffId", *SOURCE_CYCLE, *SOURCE_OTHER, *SOURCE_CLIENT, *SOURCE_RANGE_ITEM}
# The list whose `InterceptMissileTech` makes its unit an interceptor, and
# what its rows answer `IInterceptData` with, by the field
# `config/contraptions.yaml`'s interceptor gives each and the build's: whole
# points, a count, a flag, or an FPoint raw integer.
INTERCEPT = "interceptMissileTechnologyDatas"
INTERCEPT_FIELDS = (
    ("attack", "attackNum"),
    ("range_max", "radiusRangeMax"),
    ("range_min", "radiusRangeMin"),
    ("prepare_time", "prepareTime"),
    ("interval", "interval"),
    ("cooling_time", "coolingTime"),
    ("rise_interval", "riseInterval"),
    ("decline", "decline"),
    ("lower_limit", "lowerLimit"),
    ("rise", "rise"),
    ("judgment_probability", "judgmentProbability"),
    ("weapon_count", "weaponCount"),
    ("preemptive", "isPreemptive"),
)
# The list whose `SupportUnitTech` runs a production line, as a production
# line item's `SupportUnitEquipment` does, and what its rows answer
# `ISupportDataSource` with, by the field this table gives each and the
# build's: a count, an enum, a flag, or an FPoint raw integer.
SUPPORT = "supportUnitTechnologies"
SUPPORT_FIELDS = (
    ("support_unit_id", "unitID"),
    ("unit_level", "unitLevel"),
    ("max_batch", "maxBatch"),
    ("max_alive", "maxCount"),
    ("create_count_per_time", "createCountPerTime"),
    ("start_time", "startTime"),
    ("appear_type", "appearType"),
    ("product_time", "productTime"),
    ("max_create_count", "maxCreateCount"),
    ("create_duration", "createDuration"),
    ("position_space", "positionSpace"),
    ("unit_life_rate", "unitLifeChangerate"),
    ("unit_damage_rate", "unitDamageChangeRate"),
    ("unit_attack_range_value", "unitAttackRangeChangeValue"),
    ("intensify_mode", "intensifyMode"),
    ("inherit_technology", "inheritTechnologyEffect"),
)
# The list whose `DeadSummonTech` summons units where its unit dies, and what
# its rows answer `IDeadSummon` with: the unit type, how many by the unit's
# level, and the `DynamicMechLevel` they take.
DEAD_SUMMON = "deadSummonTechnologies"
DEAD_SUMMON_FIELDS = (
    ("unit_id", "unitID"),
    ("unit_count", "unitCount"),
    ("unit_level", "unitLevel"),
)
# The list whose `MoveAbilitySummonTech` makes units as its unit's move ability
# reaches a time, and what its rows answer `IMoveAbilitySummon` and
# `ISupportDataSource` with.
MOVE_SUMMON = "moveAbilitySummonTechDatas"
MOVE_SUMMON_FIELDS = (
    ("summon_time", "moveAbilityTimeType"),
    ("support_unit_id", "unitID"),
    ("unit_level", "unitLevel"),
    ("max_batch", "maxBatch"),
    ("max_alive", "maxCount"),
    ("create_count_per_time", "createCountPerTime"),
    ("start_time", "startTime"),
    ("appear_type", "appearType"),
    ("product_time", "productTime"),
    ("create_duration", "createDuration"),
)
# The list whose `StealthTech` puts its unit in stealth once its life first
# falls to a share of its maximum, and what its rows answer
# `IStealthTechDataSource` with: that share and the seconds it lasts, each an
# FPoint raw integer.
STEALTH = "stealthTechData"
STEALTH_FIELDS = (
    ("trigger_life_rate", "triggerConditionValue"),
    ("duration", "duration"),
)
# The list whose `DamageShareTech` is an `IMechGroupSource`, and the field its
# rows carry that only the client reads: the link's effect between members.
MECH_GROUP = "damageShareTechnologies"
MECH_GROUP_CLIENT = {"effectName"}
# The list whose `SiegeModeTech` digs its unit in as the fight starts, and what
# its rows answer `ISiegeModeEffectDataSource` with, each an FPoint raw
# integer: the rates and values it writes on its unit's main skill and the
# rate on its unit's life while it is dug in, the seconds with no enemy in
# range after which it leaves, and the seconds before it moves again. The
# rate on its move speed no fight reads: the stopped motion holds the unit.
SIEGE = "siegeModeTechDatas"
SIEGE_FIELDS = (
    ("life_rate", "siegeModeLifeChangeRate"),
    ("attack_interval_rate", "siegeModeAttackIntevalChangeRate"),
    ("attack_interval_value", "siegeModeAttackIntevalChangeValue"),
    ("damage_rate", "siegeModeDamageChangeRate"),
    ("attack_range_value", "siegeModeAttackRangeChangeValue"),
    ("attack_range_rate", "siegeModeAttackRangeChangeRate"),
    ("splash_range_value", "siegeModeSplashRangeChangeValue"),
    ("projectile_speed_value", "siegeModeProjectileSpeedChangeValue"),
    ("duration", "siegeModeDuration"),
    ("animation_delay", "animationDelay"),
)
SIEGE_CLIENT = {"siegeModeMoveSpeedChangeRate"}
# The list of `TechnologyGroupData` a plain technology comes from. A row of any
# other list is a subclass (`BuffTechnologyData`, `SplashTechnologyData` and
# the rest) that does something beyond its unit's numbers.
PLAIN = "technologyDatas"
# The fields every row carries that say nothing a fight reads beyond its
# numbers: identity, text, cost, and which of the unit's skills the numbers
# reach.
DESCRIPTIVE = {
    "id", "name", "isTestData", "iconName", "description", "descParams", "story",
    "limitedScene", "supply", "previousTechID", "activeLevel", "unlockCost",
    "mainSkillEffect", "extraSkillEffect", "extraSkillNumericalEffect",
}
RATES = {"life_rate", "damage_rate", "attack_range_rate", "attack_interval_rate", "projectile_life_rate", "exp_rate",
         "lifesteal_multiplier", "recovery_life_rate", "air_damage_change_rate", "ground_damage_change_rate"}
INTEGERS = {"speed_value", "min_attack_range_value", "reduce_damage_value", "projectile_count_value",
            "dead_line_value", "barrier_energy", "barrier_radius"}


def raw(value):
    return value["m_rawValue"] if isinstance(value, dict) else value


def rows_by_id() -> dict[int, dict]:
    """Every technology row of every list, by id."""
    rows = {}
    for kind, table in build_data.level0("TechnologyGroupData").items():
        if isinstance(table, list):
            for row in table:
                effect = {"id": row["id"], "name": row["name"], "row": row, "kind": kind}
                for field, source in LISTS:
                    effect[field] = [raw(value) for value in row.get(source) or []]
                for field, source, owner in SUBCLASS_LISTS:
                    if kind == owner:
                        effect[field] = [raw(value) for value in row[source]]
                for field, source, owner in SUBCLASS_SCALARS + SET_SCALARS:
                    if kind == owner:
                        effect[field] = raw(row[source])
                for field, source in FLAGS:
                    effect[field] = bool(row.get(source))
                rows[row["id"]] = effect
    return rows


def special(row: dict) -> list[str]:
    """The fields of a plain technology's row that are set and are neither a
    number this table carries nor descriptive: what it does beyond numbers."""
    numeric = ({source for _, source in LISTS} | {source for _, source, _ in SUBCLASS_LISTS}
               | {source for _, source, _ in SUBCLASS_SCALARS + SET_SCALARS}
               | {source for _, source in FLAGS})
    source = (BUFF_SOURCE if row["kind"] == BUFF
              else {field for _, field in INTERCEPT_FIELDS} if row["kind"] == INTERCEPT
              else {field for _, field in STEALTH_FIELDS} if row["kind"] == STEALTH
              else MECH_GROUP_CLIENT if row["kind"] == MECH_GROUP
              else {field for _, field in SIEGE_FIELDS} | SIEGE_CLIENT if row["kind"] == SIEGE
              else set())
    return sorted(
        field
        for field, value in row["row"].items()
        if field not in numeric | DESCRIPTIVE | source
        and raw(value) not in (0, False, "", None, [], {})
    )


def described(row: dict) -> str | None:
    """A technology's English description, from whichever technology table localizes it."""
    for term in build_data._terms():
        match = re.fullmatch(r"ConfigData/(\w+)/description_(\d+)", term)
        if match and int(match.group(2)) == row["id"] and (
                "Tech" in match.group(1) or match.group(1) in ("SearchTargetSpecificData", "BurrowData")):
            return build_data.description(match.group(1), row)
    return None


def technologies() -> dict[int, str]:
    """Every technology the tracked table knows, with the unit that owns it."""
    owned = {}
    unit = None
    for line in UNIT_TECHS.read_text().splitlines():
        if line.startswith("  - type: "):
            unit = line.removeprefix("  - type: ").strip()
        else:
            entry = re.match(r"\s+- \{id: (\d+), supply: (\d+)\}", line)
            if entry and unit:
                owned[int(entry.group(1))] = unit
    return owned


def reading(field: str, value: int) -> str:
    if field in INTEGERS:
        return str(value)
    if field in RATES:
        return f"{value / ONE:+.6g}"
    return f"{value / ONE:+.6g}"


def crosscheck(rows: dict[int, dict]) -> tuple[int, int, list[str]]:
    """Two ways a misparse would have to survive to reach the table.

    **Every number this table states has to be in the technology's description.**
    The check runs in this direction rather than the officers' because a
    technology's text states effects this table does not carry: a
    damage rate against aerial units, damage received, a bombardment it
    summons. Those are its skill's, not its unit's, and they live in the skill
    the technology points at with `targetSkillID`.

    **Every rank list has to be its first entry times the rank.** A technology
    that grows stores one value per rank rather than a multiplier, and the
    build rounds each of them on its own, so this allows two raw units of
    drift and nothing more. Garbage read off the wrong offset is not
    arithmetic.
    """
    stated_by_text = {
        "life_rate",
        "damage_rate",
        "attack_range_value",
        "attack_range_rate",
        "attack_interval_value",
        "attack_interval_rate",
        "speed_value",
        "splash_range_value",
    }

    def claim(field: str, raw: int) -> str:
        if field in INTEGERS:
            return f"{abs(raw)}"
        value = abs(raw) / ONE
        return f"{round(value * 100)}" if field in RATES else f"{value:g}"

    checked, disagreed = 0, []
    for identifier, row in sorted(rows.items()):
        for field, _ in LISTS:
            values = row[field]
            if len(values) < 2 or not any(values):
                continue
            for rank, value in enumerate(values):
                grown = round(values[0] / ONE * (rank + 1) * ONE)
                if abs(value - grown) > 2:
                    disagreed.append(
                        f"technology {identifier} ({row['name']}) {field} rank"
                        f" {rank + 1} is {value}, and its first entry times the"
                        f" rank is {grown}"
                    )
                    break

        text = described(row["row"])
        if not text:
            continue
        numbers = set(re.findall(r"\d+(?:\.\d+)?", text))
        claims = {
            claim(field, next(value for value in row[field] if value))
            for field, _ in LISTS
            if field in stated_by_text and any(row[field])
        }
        if not claims:
            continue
        checked += 1
        if not claims <= numbers:
            disagreed.append(
                f"technology {identifier} ({row['name']}) states"
                f" {sorted(claims - numbers)}, which its effect text does not"
            )
    return checked, len(rows), disagreed


def main() -> int:
    build_data.arguments(__doc__)
    owners = technologies()
    every = rows_by_id()
    rows = {}
    for identifier, unit in sorted(owners.items()):
        if identifier not in every:
            print(f"error: technology {identifier} is not in the build", file=sys.stderr)
            return 1
        rows[identifier] = dict(every[identifier], unit=unit)

    checked, _parsed, disagreed = crosscheck(rows)
    for problem in disagreed:
        print(f"error: {problem}", file=sys.stderr)
    if disagreed:
        return 1

    lines = [
        "schema: mechcore.technology_effects",
        "",
        "# What a technology does to a fight, which is a correction it writes onto",
        "# the unit that researched it. `config/unit_techs.yaml` carries which",
        "# technologies a unit may research and what each costs.",
        "# `docs/rules/technology_effects.md` states what each field means and what",
        "# is not established.",
        "#",
        "# Every technology a unit may research has a row. Its `kind` is the list",
        "# of `TechnologyGroupData` it comes from: `technologyDatas` for a plain",
        "# technology, which does nothing but correct its unit's numbers, and a",
        "# subclass's list for one that does something more. A plain row, and",
        "# a lifesteal or repair row, that sets a field beyond what it carries",
        "# here names it in `special`.",
        "#",
        "# Every effect is a list indexed by the unit's rank: one entry for a",
        "# technology whose effect is flat, and one per rank for a technology that",
        "# grows with it. A rate is an FPoint Q32.32 raw integer, a value is an",
        "# FPoint in the number's own units, and speed is a plain integer.",
        "# `exp_rate` is a rate on what the unit gains, not a correction.",
        "#",
        "# A subclass's row also carries what it answers its own interface with:",
        "# a splash technology the FPoint metres of splash it adds its unit's",
        "# skill (`splash_range`),",
        "# a lifesteal technology its `lifesteal_multiplier`, the share of a hit's",
        "# damage its unit takes back as life, and a repair technology the",
        "# seconds hurt before it repairs, the state it repairs in (0 always,",
        "# 1 underground, 2 cloaked), the seconds between two repairs and the",
        "# share of maximum life each restores. A sweep technology names the",
        "# skill it changes and the metres it adds to the strip's width and",
        "# length, and sets whether the strip lies across the line to the",
        "# target, runs backwards, and keeps one direction. An armour technology",
        "# carries its `reduce_damage_value`, the damage each hit on its unit",
        "# loses, one entry per unit level. An extra weapon technology that",
        "# lowers the damage of its unit's skills carries its",
        "# `all_weapon_reduce_damage_rate`, a rate. A technology that changes",
        "# how its unit's skill searches carries the whole metres it reaches",
        "# further at, and its search counts off, an aerial and a ground target",
        "# (`air_target_score_offset`, `ground_target_score_offset`), and the",
        "# rate it adds to its damage on each (`air_damage_change_rate`,",
        "# `ground_damage_change_rate`), which a damage-intensify technology",
        "# carries alone. A technology that turns its unit's",
        "# skill on or off aircraft says whether it turns the extra skills too",
        "# (`extra_skill_effect`). A technology whose unit's hit deals a second",
        "# damage around it carries that damage, the FPoint metres it reaches,",
        "# whether it reaches what the first hit struck, whether the attacker's",
        "# and the target's buffs scale it, whether it disables the struck",
        "# units' technologies, and the buff it writes on them (`secondary_*`).",
        "# A buff technology carries what triggers its buff (`buff_trigger`, a",
        "# BuffTechListener: 1 is the fight's start), whom it reaches",
        "# (`buff_targets`, TargetTypes: 1 is the unit itself), how likely, and",
        "# the buffDatas row it adds, as a buff item does. A missile",
        "# interception technology carries what its unit intercepts with",
        "# (`intercept`), named as `config/contraptions.yaml`'s interceptor, with",
        "# how many interceptors it is and whether each locks its unit's main",
        "# skill while it intercepts (`weapon_count`, `preemptive`). A",
        "# technology that summons where its unit dies carries the unit type,",
        "# how many by its unit's level, and the DynamicMechLevel they take",
        "# (`dead_summon`: 0 the first level, 3 its unit's). A technology that",
        "# makes units as its unit's move ability reaches a time (a",
        "# MoveAbilityTimeType: 2 as it begins to surface) carries that time and",
        "# the line it makes them by (`move_summon`), as a production row does.",
        "# A technology that turns its unit's skills' locking of their target",
        "# over says so (`inverse_lock_target`).",
        "# A multi-attack technology carries how many more projectiles each of",
        "# its unit's attacks fires (`projectile_count_value`), and the FPoint",
        "# seconds it adds between two of them and metres it adds to how far",
        "# each lands from its target (`projectile_duration_value`,",
        "# `projectile_random_range_value`). A stealth technology carries the",
        "# share of its unit's maximum life its life first falling to puts it in",
        "# stealth, and the seconds that lasts (`stealth`). A dead-line",
        "# technology carries the life, by its unit's level, at or under which",
        "# its unit's main skill destroys what it hits (`dead_line_value`), and",
        "# whether it does so through a unit's own shield",
        "# (`dead_line_ignores_shield`). A move-ability attack technology",
        "# carries the rate on its unit's surfacing time (`exit_time_rate`), and",
        "# what its unit's first attacks after surfacing take: how many",
        "# (`strike_trigger_count`), the rate on their damage, and the FPoint",
        "# metres of splash and the attack point they add",
        "# (`strike_damage_rate`, `strike_splash_range`, `strike_attack_point`).",
        "# A move-ability terrain technology carries the time its unit's move",
        "# ability leaves it (`range_item_time`, a MoveAbilityTimeType: 3 as it",
        "# ends a surfacing), the MechMoveType it needs (`range_item_move_type`:",
        "# 1 underground), and the terrain's FPoint metres and seconds",
        "# (`range_item_range`, `range_item_life_time`) and its rates on the",
        "# attack range of what stands in it and on the remote hits it takes",
        "# (`fog_attack_range_rate`, `reduce_damage_from_remote`).",
        "# A grouping technology carries the FPoint metres within which its",
        "# units link into a group (`share_distance`), by its unit's level, and",
        "# what the group is for (`group_purpose`, a MechGroupPurpose): 0 shares",
        "# the damage any of them takes; 1 raises each member's skill damage by",
        "# the rate (`group_damage_rate`) for every other member, counting no",
        "# more than `group_max_count` members when that is above zero, on its",
        "# main skill and its extra skills as `main_skill_effect` and",
        "# `extra_skill_effect` say.",
        "# A barrier technology carries the energy and the whole metres of",
        "# radius of the battlefield shield its unit carries (`barrier_energy`,",
        "# `barrier_radius`), by its unit's level.",
        "# A reactive armor technology carries the FPoint rate on the damage its",
        "# unit takes (`reactive_armor_rate`) and how many hits dealing it damage",
        "# the rate lasts (`reactive_armor_count`).",
        "",
        "technologies:",
    ]
    written = plain = 0
    buffs = {buff["id"]: buff for buff in build_data.container()["buffDatas"]}
    for identifier, row in sorted(rows.items()):
        held = [
            (field, row[field])
            for field, _ in LISTS
            if any(value != 0 for value in row.get(field, []))
        ]
        written += 1
        lines.append(f"  - id: {identifier}")
        lines.append(f"    name: {row['name']}")
        lines.append(f"    unit: {row['unit']}")
        lines.append(f"    kind: {row['kind']}")
        # A plain row, and a subclass's whose mechanism the simulator reads,
        # name what else they set, which the simulator refuses.
        extra = special(row) if row["kind"] in IMPLEMENTED else []
        if extra:
            lines.append(f"    special: [{', '.join(extra)}]")
        elif row["kind"] == PLAIN:
            plain += 1
        held += [
            (field, row[field])
            for field, _, owner in SUBCLASS_LISTS
            if row["kind"] == owner and row[field]
        ]
        for field, source, owner in SUBCLASS_SCALARS:
            if row["kind"] == owner:
                value = row[field]
                lines.append(f"    {field}: {str(value).lower() if isinstance(value, bool) else value}")
        for field, _ in FLAGS:
            if row[field]:
                lines.append(f"    {field}: true")
        for field, source, owner in SET_SCALARS:
            if row["kind"] == owner and row[field]:
                lines.append(f"    {field}: {row[field]}  # {reading(field, row[field])}")
        if row["kind"] == BUFF:
            lines += buff_lines(row["row"], buffs)
        if row["kind"] == INTERCEPT:
            lines.append("    intercept:")
            for field, source in INTERCEPT_FIELDS:
                value = row["row"][source]
                if isinstance(value, bool):
                    lines.append(f"      {field}: {str(value).lower()}")
                elif isinstance(value, dict):
                    point = value["m_rawValue"]
                    lines.append(f"      {field}: {point}  # {point / ONE:.6g}")
                else:
                    lines.append(f"      {field}: {value}")
        if row["kind"] == STEALTH:
            lines.append("    stealth:")
            for field, source in STEALTH_FIELDS:
                point = row["row"][source]["m_rawValue"]
                lines.append(f"      {field}: {point}  # {point / ONE:.6g}")
        if row["kind"] == SIEGE:
            lines.append("    siege_mode:")
            for field, source in SIEGE_FIELDS:
                point = row["row"][source]["m_rawValue"]
                lines.append(f"      {field}: {point}  # {point / ONE:.6g}")
        if row["kind"] == DEAD_SUMMON:
            fields = ", ".join(
                f"{field}: {row['row'][source]}".replace("'", "") for field, source in DEAD_SUMMON_FIELDS
            )
            lines.append(f"    dead_summon: {{{fields}}}")
        if row["kind"] in (SUPPORT, MOVE_SUMMON):
            block, fields = ("production", SUPPORT_FIELDS) if row["kind"] == SUPPORT else ("move_summon", MOVE_SUMMON_FIELDS)
            lines.append(f"    {block}:")
            for field, source in fields:
                value = row["row"][source]
                if isinstance(value, bool):
                    lines.append(f"      {field}: {str(value).lower()}")
                elif isinstance(value, dict):
                    point = value["m_rawValue"]
                    lines.append(f"      {field}: {point}  # {point / ONE:.6g}")
                else:
                    lines.append(f"      {field}: {value}")
            offsets = ", ".join(
                f"{{x: {p['x']['m_rawValue']}, z: {p['y']['m_rawValue']}}}" for p in row["row"]["positions"]
            )
            lines.append(f"      positions: [{offsets}]")
        for field, values in held:
            raw = ", ".join(str(value) for value in values)
            if field in INTEGERS:
                lines.append(f"    {field}: [{raw}]")
                continue
            readings = ", ".join(reading(field, value) for value in values)
            lines.append(f"    {field}: [{raw}]  # {readings}")

    OUTPUT.write_text("\n".join(lines) + "\n")
    print(
        f"{written} technologies, {plain} of them plain ->"
        f" {OUTPUT.relative_to(REPOSITORY)}; every number {checked} of them state"
        f" is in their own description, and every rank list is its first entry"
        f" times the rank"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
