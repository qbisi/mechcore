#!/usr/bin/env python3
"""Measure the random-state claims of `docs/spec/document/state.md` over the local replay set.

Two streams are recorded. This script shows that the player stream is its seed
and that the match stream is the seed advanced by a match-dependent number of
draws.

`replay/README.md` explains which replays are usable and why the downloaded
ones are not.

    python3 scripts/random_state_support.py
"""
import collections
import pathlib
import xml.etree.ElementTree as ET

REPLAYS = (pathlib.Path.home() / "Library/Application Support/Steam/steamapps"
           / "common/Mechabellum/Mechabellum.app/ProjectDatas/Replay")
MASK = (1 << 64) - 1
SEARCH = 200_000


def rotl(value, count):
    return ((value << count) | (value >> (64 - count))) & MASK


class GrRandom:
    """Build-2259 `GRRandom` type 1, the xoshiro256** of crates/simulation."""

    def __init__(self, seed):
        self.state = [seed & MASK, 255, 0, 0]
        for _ in range(16):
            self.next()

    def next(self):
        s0, s1, s2, s3 = self.state
        result = (rotl((s1 * 5) & MASK, 7) * 9) & MASK
        t02 = s2 ^ s0
        t31 = s3 ^ s1
        self.state = [s0 ^ t31, (t02 ^ s1) & MASK,
                      (t02 ^ ((s1 << 17) & MASK)) & MASK, rotl(t31, 45)]
        return result


def battle_record(path):
    raw = path.read_bytes()
    start = raw.index(b"<?xml")
    end = raw.index(b"</BattleRecord>") + len(b"</BattleRecord>")
    return ET.fromstring(raw[start:end].decode("utf-8", "replace"))


def standard_replays():
    """Every locally recorded standard match in the native replay directory."""
    for path in sorted(REPLAYS.glob("*.grbr")):
        try:
            root = battle_record(path)
        except ValueError:
            continue
        info = root.find("BattleInfo")
        if int(root.findtext("Seat")) >= 0 and info.find("matchType") is None:
            yield path, root


def random_state(element):
    return [int(word.text)
            for word in element.find("randomStateData").find("randomStates")]


def main():
    rounds = matched = moved = 0
    distances = []

    for _, root in standard_replays():
        system_seed = int(root.find("BattleInfo").findtext("SystemSeed"))

        for record in root.find("playerRecords"):
            seed = int(record.findtext("seed"))
            expected = GrRandom(seed).state
            first = None
            for round_record in record.find("playerRoundRecords"):
                player = round_record.find("playerData")
                if player is None:
                    continue
                recorded = random_state(player)
                rounds += 1
                matched += recorded == expected
                first = first if first is not None else recorded
                moved += recorded != first

        # Where each round's match state sits in the stream GRRandom(SystemSeed)
        # generates, by walking that stream and looking the state up.
        walker = GrRandom(system_seed)
        seen = {tuple(walker.state): 0}
        for step in range(1, SEARCH + 1):
            walker.next()
            seen.setdefault(tuple(walker.state), step)
        distances.append([seen.get(tuple(random_state(snapshot)))
                          for snapshot in root.find("matchDatas")])

    print(f"ranked player-rounds {rounds}")
    print(f"  playerData.randomStateData == GRRandom(seed): {matched}/{rounds}")
    print(f"  rounds where the player stream moved:         {moved}")
    unreached = sum(step is None for match in distances for step in match)
    opening = sorted(match[0] for match in distances)
    first = sorted(match[1] - match[0] for match in distances)
    idle = sorted(match[2] - match[1] for match in distances)
    later = [match[step + 1] - match[step]
             for match in distances for step in range(2, len(match) - 1)]
    print(f"ranked matches {len(distances)}, states not reached in "
          f"{SEARCH} draws: {unreached}")
    print(f"  draws to the round 0 snapshot: {opening[0]} to {opening[-1]}")
    print(f"  round 0 to round 1:            {first[0]} to {first[-1]}")
    print(f"  round 1 to round 2:            {idle[0]} to {idle[-1]}")
    print(f"  each later round:              {min(later)} to {max(later)}, "
          f"mean {sum(later) / len(later):.2f}, n {len(later)}")
    print("  later-round histogram:",
          dict(sorted(collections.Counter(later).items())))


if __name__ == "__main__":
    main()
