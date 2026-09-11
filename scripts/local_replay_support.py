#!/usr/bin/env python3
"""Measure the state-document claims over the locally recorded replay set.

`tests/grbr` tracks 41 files. The Steam installation also keeps a downloaded
provenance class that does not agree with them, so this script uses only
the locally recorded ones and says how it tells them apart. See
`tests/grbr/README.md` for the rule and why the downloaded class is unusable.

    python3 scripts/local_replay_support.py
"""
import collections
import json
import pathlib
import xml.etree.ElementTree as ET

REPLAYS = (pathlib.Path.home() / "Library/Application Support/Steam/steamapps"
           / "common/Mechabellum/Mechabellum.app/ProjectDatas/Replay")
CONFIG = pathlib.Path("work/inputs/config-data-container-build2259.json")
CHAIN = {4: 20310, 401: 20311, 5: 20300, 501: 20301}


def battle_record(path):
    raw = path.read_bytes()
    start = raw.index(b"<?xml")
    end = raw.index(b"</BattleRecord>") + len(b"</BattleRecord>")
    return ET.fromstring(raw[start:end].decode("utf-8", "replace"))


def locally_recorded():
    """Ranked matches this machine recorded itself, newest naming scheme aside.

    A downloaded replay carries Seat -1 and a server-side name; its snapshots are
    rebuilt from PlayerDetail and disagree with a locally taken one.
    """
    for path in sorted(REPLAYS.glob("*.grbr")):
        root = battle_record(path)
        info = root.find("BattleInfo")
        if int(root.findtext("Seat")) >= 0 and info.find("matchType") is None:
            yield path, root


def main():
    structure = json.loads(CONFIG.read_text())["m_Structure"]
    blueprints = {b["id"]: b for b in structure["blueprints"]}
    officers = {o["id"]: o for o in structure["officerDatas"]}
    skill_to_blueprint = {b["mapID"]: i for i, b in blueprints.items()
                          if b["bpType"] == 2 and i != 1002}

    matches = rounds = panels = 0
    held = collections.Counter()
    tower = collections.Counter()
    queue = collections.Counter()
    chain_agrees = rebuilt = 0
    reordered = non_prefix = 0
    chain_spelling = collections.Counter()

    for _, root in locally_recorded():
        matches += 1
        for record in root.find("playerRecords"):
            previous = None
            for round_record in record.find("playerRoundRecords"):
                data = round_record.find("playerData")
                rounds += 1
                owned = sorted(int(x.text) for x in data.find("bluepints"))
                held.update(owned)
                tower.update(int(x.text) for x in data.find("energyTowerSkills"))
                queue[len(list(data.find("researchQueue")))] += 1

                listed = [int(x.text) for x in data.find("officers")]
                implied = {CHAIN[b] for b in owned if b in CHAIN}
                chain_agrees += implied == set(listed) & set(CHAIN.values())
                for lower, upper in ((4, 401), (5, 501)):
                    if upper in owned:
                        chain_spelling["keeps " + str(lower) if lower in owned
                                        else "drops " + str(lower)] += 1

                hub = [(int(s.findtext("index")), int(s.findtext("id")))
                       for s in data.find("commanderSkills")]
                granted = collections.Counter()
                for officer in listed:
                    granted.update(officers.get(officer, {}).get("commanderSkillIds") or [])
                remaining = []
                for _, skill in hub:
                    if granted[skill]:
                        granted[skill] -= 1
                    else:
                        remaining.append(skill)
                predicted = {skill_to_blueprint[s] for s in remaining
                             if s in skill_to_blueprint}
                for lower, upper in ((4, 401), (5, 501)):
                    if upper in owned:
                        predicted |= {upper}
                    elif lower in owned:
                        predicted |= {lower}
                rebuilt += sorted(predicted) == owned

                panels += 1
                indices = [i for i, _ in hub]
                reordered += indices != sorted(indices)
                if previous is not None and not (
                        len(hub) >= len(previous) and hub[:len(previous)] == previous):
                    non_prefix += 1
                previous = hub

    print(f"locally recorded ranked matches {matches}, player-rounds {rounds}")
    print("blueprints ever held:", dict(sorted(held.items())))
    print("energy tower skills ever held:", dict(sorted(tower.items())))
    print("researchQueue lengths:", dict(queue))
    print(f"chain officer agrees with blueprint list: {chain_agrees}/{rounds}")
    print(f"blueprint list rebuilt from the panel:    {rebuilt}/{rounds}")
    print("chain spelling at level two:", dict(chain_spelling))
    print(f"panels {panels}, index out of order {reordered}, "
          f"successor not a prefix extension {non_prefix}")


if __name__ == "__main__":
    main()
