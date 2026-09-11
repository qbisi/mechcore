#!/usr/bin/env python3
"""Measure the reinforcement-pool log claims of `docs/spec/document/state.md`.

`matchDatas.poolOPs` is `ReinforcePool.m_ReinforceOperation`, a list of
`(op, id)` pairs that `ApplayOperation` replays to rebuild the pool. This script
shows how the recorded lists behave: append-only across rounds, no id twice, and
no id both added and removed in one match.

`tests/grbr/README.md` explains which replays are usable and why the downloaded
ones are not.

    python3 scripts/reinforce_pool_support.py
"""
import collections
import pathlib
import xml.etree.ElementTree as ET

REPLAYS = (pathlib.Path.home() / "Library/Application Support/Steam/steamapps"
           / "common/Mechabellum/Mechabellum.app/ProjectDatas/Replay")
TRAINING = ("13-25-40", "13-54-16")
OFFICER_RANGE = range(30000, 40000)


def battle_record(path):
    raw = path.read_bytes()
    start = raw.index(b"<?xml")
    end = raw.index(b"</BattleRecord>") + len(b"</BattleRecord>")
    return ET.fromstring(raw[start:end].decode("utf-8", "replace"))


def ranked_replays():
    for path in sorted(REPLAYS.glob("*.grbr")):
        if any(marker in path.name for marker in TRAINING):
            continue
        try:
            root = battle_record(path)
        except ValueError:
            continue
        if int(root.findtext("Seat")) >= 0:
            yield root


def pool_edits(snapshot):
    ops = snapshot.find("poolOPs")
    if ops is None:
        return []
    return [tuple(int(word.text) for word in pair) for pair in ops]


def main():
    matches = snapshots = extends = rewrites = 0
    operations = collections.Counter()
    lengths = []
    repeated_ids = both_operations = officer_ids = final_entries = 0

    for root in ranked_replays():
        matches += 1
        previous = None
        for snapshot in root.find("matchDatas"):
            current = pool_edits(snapshot)
            snapshots += 1
            operations.update(op for op, _ in current)
            if previous is not None and current[:len(previous)] != previous:
                rewrites += 1
            else:
                extends += 1
            previous = current

        lengths.append(len(previous))
        seen = collections.Counter(item for _, item in previous)
        repeated_ids += sum(1 for count in seen.values() if count > 1)
        by_id = collections.defaultdict(set)
        for op, item in previous:
            by_id[item].add(op)
        both_operations += sum(1 for ops in by_id.values() if ops == {0, 1})
        officer_ids += sum(1 for _, item in previous if item in OFFICER_RANGE)
        final_entries += len(previous)

    print(f"ranked matches {matches}, round snapshots {snapshots}")
    print(f"  snapshots extending the previous list: {extends}, rewriting: "
          f"{rewrites}")
    print(f"  operation counts: {dict(sorted(operations.items()))}")
    print(f"  final list lengths: {sorted(lengths)}")
    print(f"  ids appearing twice in one final list: {repeated_ids}")
    print(f"  ids both added and removed in one match: {both_operations}")
    print(f"  final entries in the officer range 30000..39999: "
          f"{officer_ids}/{final_entries}")


if __name__ == "__main__":
    main()
