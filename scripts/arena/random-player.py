#!/usr/bin/env python3
"""Random player for `mechcore arena` and `mechcore solve` (pure stdlib Python 3).

    mechcore arena new --blue 'python3 scripts/arena/game-ai-player.py --seed 1' \
        --red 'python3 scripts/arena/random-player.py' > m.yaml
"""

import argparse
import random
from bot_round import serve

# The side's own half, in its own coordinates: its territory is toward
# negative y. A place the board does not take is refused and drawn again; a
# formation an odd number of cells wide stands on a centre half a cell off the
# grid, so x is drawn in half cells.
X = range(-200, 201, 5)
Y = range(-260, -59, 10)

# How many refusals in a row end a round's buying.
PATIENCE = 8


def deploy(problem, draw):
    offers = problem.state.get("reinforce_offers") or []
    if offers:
        order = list(range(len(offers) - 1))
        draw.shuffle(order)
        for index in order + [len(offers) - 1]:
            if (
                problem.try_action(
                    {"type": "choose_reinforce_item", "index": index, "name": offers[index]["name"]},
                    keep=True,
                )
                is not None
            ):
                break
    refused = 0
    while refused < PATIENCE:
        unlocked = problem.state[problem.side].get("unlocked_units") or []
        if not unlocked:
            return
        state = problem.try_action(
            {
                "type": "buy_unit",
                "name": draw.choice(unlocked),
                "position": {"x": draw.choice(X), "y": draw.choice(Y)},
            },
            keep=True,
        )
        refused = refused + 1 if state is None else 0


def solve_round(problem, draw):
    if problem.number:
        deploy(problem, draw)
    else:
        offers = problem.state[problem.side]["offers"]
        problem.actions.append(problem.choose_team(problem.side, draw.randrange(len(offers))))


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument(
        "--seed",
        type=int,
        help="the bot's random seed; independent of the player seed, random when omitted",
    )
    args = parser.parse_args()
    draw = random.Random(args.seed)
    serve("random", lambda problem: solve_round(problem, draw))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
