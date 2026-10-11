#!/usr/bin/env python3
"""A player that takes legal decisions at random, for `mechcore arena run`.

    mechcore arena run m.yaml --blue 'python3 scripts/arena/random-player.py' \
        --red 'python3 scripts/arena/random-player.py --seed 2'

It speaks the request stream of `docs/spec/mechcore/cli.md`'s `shell`: one
JSON request per line on standard output, one JSON result per line on
standard input. It knows nothing the binary does not tell it, so it is also
the smallest worked example of a player: each round it takes a random
opening or reinforcement card, buys units it can afford at random places in
its own half until the rules refuse a few in a row, and commits.

A decision is legal exactly when the rules settle it, so the player asks by
taking it and reads the refusal rather than knowing the rules itself.
"""

import argparse
import json
import random
import sys

# The side's own half, in its own coordinates: its territory is toward
# negative y. A place the board does not take is refused and drawn again; a
# formation an odd number of cells wide stands on a centre half a cell off the
# grid, so x is drawn in half cells.
X = range(-200, 201, 5)
Y = range(-260, -59, 10)

# How many refusals in a row end a round's buying.
PATIENCE = 8


def ask(request):
    print(json.dumps(request), flush=True)
    line = sys.stdin.readline()
    if not line:
        sys.exit(0)
    return json.loads(line)


def failed(answer):
    return answer.get("schema") == "mechcore.error"


def act(decision):
    answer = ask({"op": "match.act", "decision": decision})
    if failed(answer):
        print(f"refused {decision}: {answer['reason']}", file=sys.stderr)
    return answer


def opening(view, side, draw):
    offers = view["sides"][side]["offers"]
    index = draw.randrange(len(offers))
    act({
        "type": "choose_advance_team",
        "index": index,
        "name": offers[index]["team"],
        "specialist": offers[index]["specialist"],
    })


def reinforce(view, draw):
    offers = view.get("reinforce_offers") or []
    if not offers:
        return
    # Each card is tried in a random order, and the decline, the last offer,
    # is what is left when none is affordable.
    order = list(range(len(offers) - 1))
    draw.shuffle(order)
    for index in order + [len(offers) - 1]:
        card = offers[index]
        name = card if isinstance(card, str) else card["name"]
        answer = act({"type": "choose_reinforce_item", "index": index, "name": name})
        if not failed(answer):
            return


def deploy(view, side, draw):
    reinforce(view, draw)
    refused = 0
    while refused < PATIENCE:
        view = ask({"op": "match.show"})
        position = view["sides"][side]["position"]
        unlocked = position.get("unlocked_units") or []
        if not unlocked:
            return
        answer = act({
            "type": "buy_unit",
            "name": draw.choice(unlocked),
            "position": {"x": draw.choice(X), "y": draw.choice(Y)},
        })
        refused = refused + 1 if failed(answer) else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--seed", type=int, help="the draw's seed; drawn when not given")
    draw = random.Random(parser.parse_args().seed)

    while True:
        view = ask({"op": "match.show", "wait": True})
        if failed(view):
            print(f"cannot read the match: {view['reason']}", file=sys.stderr)
            return 1
        side = view["side"]
        phase = view["phase"]
        if phase == "over":
            return 0
        if view["sides"][side]["committed"] or phase not in ("opening", "deploy"):
            continue
        if phase == "opening":
            opening(view, side, draw)
        else:
            deploy(view, side, draw)
        answer = ask({"op": "match.commit"})
        if failed(answer):
            print(f"commit refused: {answer['reason']}", file=sys.stderr)
            return 1


if __name__ == "__main__":
    sys.exit(main())
