#!/usr/bin/env python3
"""Measure the battle-level claims of `docs/spec/document/battle.md`.

A battle is a match: what every round shares, and the turns in order. This
script measures both halves. `BattleInfo` and each `PlayerRecord.data` say what
is invariant across rounds, and consecutive `playerRoundRecords` say which side
quantities can only move one way.

`replay/README.md` explains which replays are usable and why the downloaded
ones are not.

    python3 scripts/battle_invariant_support.py
"""
import collections
import pathlib
import xml.etree.ElementTree as ET

REPLAYS = (pathlib.Path.home() / "Library/Application Support/Steam/steamapps"
           / "common/Mechabellum/Mechabellum.app/ProjectDatas/Replay")
XSI_TYPE = "{http://www.w3.org/2001/XMLSchema-instance}type"
BLUEPRINT_SUCCESSOR = {4: 401, 5: 501}
OFFICER_SUCCESSOR = {20300: 20301, 20310: 20311}


def battle_record(path):
    raw = path.read_bytes()
    start = raw.index(b"<?xml")
    end = raw.index(b"</BattleRecord>") + len(b"</BattleRecord>")
    return ET.fromstring(raw[start:end].decode("utf-8", "replace"))


def ranked_replays():
    for path in sorted(REPLAYS.glob("*.grbr")):
        try:
            root = battle_record(path)
        except ValueError:
            continue
        info = root.find("BattleInfo")
        if int(root.findtext("Seat")) >= 0 and info.find("matchType") is None:
            yield root


def ints(element):
    return [] if element is None else [int(word.text) for word in element]


def tech_loadout(record):
    """The player's custom tech selection, one row per unit, match level."""
    return {int(unit.findtext("id")):
            frozenset(int(tech.get("data")) for tech in unit.find("techs"))
            for unit in record.find("data").find("unitDatas")}


def side_state(round_record):
    data = round_record.find("playerData")
    techs = set()
    active = data.find("activeTechnologies")
    if active is not None:
        for unit in active:
            techs |= {int(tech.get("data")) for tech in unit.find("techs")}
    return {
        "unit_index": int(data.findtext("unitIndex")),
        "contraption_index": int(data.findtext("contraptionIndex")),
        "reactor_core": int(data.findtext("reactorCore")),
        "techs": techs,
        "panel": [(int(skill.findtext("index")), int(skill.findtext("id")))
                  for skill in data.find("commanderSkills")],
        "officers": set(ints(data.find("officers"))),
        "blueprints": set(ints(data.find("bluepints"))),
        "tower_levels": ints(data.find("towerStrengthenLevels")),
        "energy_tower_skills": set(ints(data.find("energyTowerSkills"))),
        "unlocked_units": set(ints(data.find("shop").find("unlockedUnits"))),
    }


def replaced_by_successor(before, after, successor):
    """Every id that disappeared was replaced by its own next level."""
    return all(successor.get(lost) in after - before
               for lost in before - after)


def net_actions(round_record):
    """Collapse the recorded undo stack onto the decisions that took effect."""
    taken = []
    undone = []
    for action in round_record.find("actionRecords"):
        kind = action.get(XSI_TYPE)
        if kind == "PAD_Undo":
            if taken:
                undone.append(taken.pop())
        elif kind == "PAD_Redo":
            if undone:
                taken.append(undone.pop())
        elif kind == "PAD_CancelReleaseCommanderSkill":
            undone.clear()
            skill = action.findtext("SkillIndex")
            for entry in reversed(taken):
                candidate, stands = entry
                if (stands
                        and candidate.get(XSI_TYPE)
                        == "PAD_ReleaseCommanderSkill"
                        and candidate.findtext("SkillIndex") == skill):
                    entry[1] = False
                    break
            taken.append([action, False])
        elif kind == "PAD_FinishDeploy":
            undone.clear()
        else:
            undone.clear()
            taken.append([action, True])
    return [action for action, stands in taken if stands]


def energy_tower_activations(round_record):
    return {int(action.findtext("SkillID"))
            for action in net_actions(round_record)
            if action.get(XSI_TYPE) == "PAD_ActiveEnergyTowerSkill"}


def main():
    header = collections.defaultdict(collections.Counter)
    loadouts = collections.Counter()
    contiguous = collections.Counter()
    from_loadout = collections.Counter()
    holds = collections.Counter()
    reactor_rises = []
    officer_replacements = debt = debt_rule = 0
    transitions = matches = 0

    for root in ranked_replays():
        matches += 1
        for field in root.find("BattleInfo"):
            header[field.tag][len(list(field)) if field.tag == "gameRules"
                              else (field.text or "").strip()] += 1
        header["Version"][root.findtext("Version")] += 1
        match_rounds = [int(data.findtext("round"))
                        for data in root.find("matchDatas")]

        for record in root.find("playerRecords"):
            rows = tech_loadout(record)
            loadouts[tuple(sorted((unit, tuple(sorted(techs)))
                                  for unit, techs in rows.items()))] += 1
            offered = set().union(*rows.values())

            round_records = list(record.find("playerRoundRecords"))
            rounds = [int(r.findtext("round")) for r in round_records]
            contiguous[rounds == match_rounds
                       == list(range(len(rounds)))] += 1

            states = [side_state(r) for r in round_records]
            for state in states:
                from_loadout[state["techs"] <= offered] += 1

            for index, (before, after) in enumerate(zip(states, states[1:])):
                transitions += 1
                holds["unit_index rises or holds"] += (
                    after["unit_index"] >= before["unit_index"])
                holds["contraption_index rises or holds"] += (
                    after["contraption_index"] >= before["contraption_index"])
                holds["reactor core falls or holds"] += (
                    after["reactor_core"] <= before["reactor_core"])
                holds["researched techs are kept"] += (
                    before["techs"] <= after["techs"])
                holds["the skill panel extends"] += (
                    after["panel"][:len(before["panel"])] == before["panel"])
                holds["unlocked units are kept"] += (
                    before["unlocked_units"] <= after["unlocked_units"])
                holds["tower levels rise or hold"] += all(
                    b <= a for b, a in zip(before["tower_levels"],
                                           after["tower_levels"]))
                holds["blueprints are kept or upgraded"] += (
                    replaced_by_successor(before["blueprints"],
                                          after["blueprints"],
                                          BLUEPRINT_SUCCESSOR))
                kept = before["officers"] <= after["officers"]
                holds["officers are kept or upgraded"] += (
                    kept or replaced_by_successor(before["officers"],
                                                  after["officers"],
                                                  OFFICER_SUCCESSOR))
                officer_replacements += not kept
                if after["reactor_core"] > before["reactor_core"]:
                    reactor_rises.append(
                        (index, after["reactor_core"] - before["reactor_core"]))

                owed = 1 in energy_tower_activations(round_records[index])
                debt += owed
                debt_rule += (1 in after["energy_tower_skills"]) == owed

    print(f"ranked matches {matches}, player slots {sum(loadouts.values())}")
    varies = {tag: len(counts) for tag, counts in header.items()
              if len(counts) > 1}
    print(f"  BattleInfo fields that differ between matches: {varies}")
    print(f"  distinct custom tech loadouts: {len(loadouts)}")
    print(f"  player-rounds whose researched techs come from the loadout: "
          f"{from_loadout[True]}/{sum(from_loadout.values())}")
    print(f"  round lists contiguous from 0 and aligned with matchDatas: "
          f"{contiguous[True]}/{sum(contiguous.values())}")
    print(f"  round transitions {transitions}")
    for claim, count in holds.items():
        print(f"    {claim}: {count}/{transitions}")
    print(f"    of which officer transitions that replaced rather than added: "
          f"{officer_replacements}")
    print(f"  reactor core rises: {len(reactor_rises)}, at round transitions "
          f"{sorted({index for index, _ in reactor_rises})}, by "
          f"{sorted(amount for _, amount in reactor_rises)}")
    print(f"  next round lists energy tower skill 1 exactly when this round "
          f"activated it: {debt_rule}/{transitions}, activations {debt}")


if __name__ == "__main__":
    main()
