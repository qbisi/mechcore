#!/usr/bin/env python3
"""A player modelled on the game's own computer opponent, for `mechcore arena run`.

    mechcore arena run m.yaml --blue 'python3 scripts/arena/game-ai-player.py --seed 1' \
        --red 'python3 scripts/arena/random-player.py'

`--name` prints `game-ai` and exits without reading a match.

Its random seed comes from `--seed`, independently of the match's player
seed; when omitted, Python initializes it from system randomness.

It is the arena's reference opponent: what the build's `SimpleDecision` does
each round, read from the decompilation, reimplemented over the request
stream. It is a reference and not a copy. The game's own random streams,
its first-round formations from the advance team's position table and its
commander skills are left out, and the board's own refusals stand in for the
build's availability checks, as in `random-player.py`.

Each round, in the build's order:

- the opening takes a random offer;
- a reinforcement card is drawn among those it can take, leaving out commander
  skills and the Dominion Core, and the decline is what is left;
- shopping: an unlock with probability `min(1, w/100)`, one purchase, then up
  to 20 actions drawn by weight among buying, researching, upgrading a unit
  whose experience bar is full, fitting equipment and activating a blueprint,
  each weight moving linearly from its first-round to its tenth-round value;
- units the round brought that it did not buy are moved beside older ones;
- from round 2 (with probability 0.3 there, always later) one or two units
  that may move are sent to the flank the other side holds more of.

A unit type is drawn by its cost: up to 100 weighs `30 - 5R`, up to 200
`18 - 2R` and dearer ones `5R - 10`, so cheap units early and dear ones late.
A side with nothing that hits air facing flyers buys only what does.
"""

import argparse
import json
import pathlib
import random
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
CONFIG = ROOT / "config"

# The side's own half, in its own coordinates, its territory toward
# negative y; the board refuses what does not fit and a nearby place is tried.
X_MIN, X_MAX, Y_MIN, Y_MAX = -200, 200, -260, -60
CENTRE = ((X_MIN + X_MAX) // 2, (Y_MIN + Y_MAX) // 2)
WIDTH, HEIGHT = X_MAX - X_MIN, Y_MAX - Y_MIN

# Units the AI keeps behind its front line: marksman, wasp, overlord, stormcaller.
BACK_LINE = {"marksman", "wasp", "overlord", "stormcaller"}
# What the AI marks as difficult for itself and never takes.
SHUNNED_EQUIPMENT = {"dominion_core"}
# The weighted actions of the shopping controller: first-round and
# tenth-round weights.
WEIGHTS = {
    "buy": (80, 20),
    "research": (20, 60),
    "upgrade": (20, 50),
    "equip": (10, 80),
    "blueprint": (0, 30),
}
UNLOCK_WEIGHTS = (60, 0)
MAX_ROUND = 10
SHOPPING_STEPS = 20


def sections(path):
    """`names.yaml`'s `id: name` rows by section."""
    tables, current = {}, None
    for line in path.read_text().splitlines():
        if match := re.match(r"^([a-z_]+):\s*$", line):
            current = tables.setdefault(match.group(1), {})
        elif current is not None and (match := re.match(r"^\s+(\d+):\s*(\w+)", line)):
            current[int(match.group(1))] = match.group(2)
    return tables


def catalogue():
    """Each unit type's price, footprint, domain and whether it hits air."""
    units = {}
    price = None
    for line in (CONFIG / "unit_prices.yaml").read_text().splitlines():
        if match := re.match(r"^\s+- type: (\w+)", line):
            price = units.setdefault(match.group(1), {})
        elif price is not None and (match := re.match(r"^\s+supply: (\d+)", line)):
            price["supply"] = int(match.group(1))
    for name, unit in units.items():
        text = (CONFIG / "units" / f"{name}.yaml").read_text()
        footprint = re.search(r"footprint: \{width: (\d+), depth: (\d+)\}", text)
        unit["width"], unit["depth"] = (int(footprint.group(1)), int(footprint.group(2)))
        unit["air"] = re.search(r"^domain: air", text, re.M) is not None
        targets = re.search(r"^attack:.*?targets: \{ground: \w+, air: (\w+)\}", text, re.M | re.S)
        unit["hits_air"] = targets is not None and targets.group(1) == "true"
    return units


def blueprints():
    """The blueprints that grant an officer rather than a commander skill."""
    text = (CONFIG / "economy.yaml").read_text()
    names = sections(CONFIG / "names.yaml")["blueprints"]
    return [names[int(match.group(1))]
            for match in re.finditer(r"\{id: (\d+), name: [^,]+, supply: \d+, grants_officer", text)]


def opening_state(view):
    return next(segment for segment in reversed(view["document"]) if segment["kind"] == "state")


class Player:
    def __init__(self, seed):
        self.draw = random.Random(seed)
        self.units = catalogue()
        self.blueprints = blueprints()
        self.skills = set(sections(CONFIG / "names.yaml")["commander_skills"].values())

    # The stream.

    def ask(self, request):
        print(json.dumps(request), flush=True)
        line = sys.stdin.readline()
        if not line:
            sys.exit(0)
        return json.loads(line)

    def take(self, decision):
        """Takes a decision, answering whether the board took it."""
        answer = self.ask({"op": "match.act", "decision": decision})
        if answer.get("schema") == "mechcore.error":
            print(f"refused {decision}: {answer['reason']}", file=sys.stderr)
            return None
        self.seen = answer
        self.position = answer["position"]
        return answer

    def legal(self, decision):
        answer = self.ask({"op": "match.act", "decision": decision, "dry_run": True})
        return answer.get("schema") != "mechcore.error"

    def view(self):
        self.seen = self.ask({"op": "match.show"})
        return self.position

    # The opening.

    def opening(self, view):
        offers = view["document"][0][self.side]["offers"]
        pick = self.draw.randrange(len(offers))
        self.take({
            "type": "choose_advance_team",
            "index": pick,
            "name": offers[pick]["team"],
            "specialist": offers[pick]["specialist"],
        })

    # A deployment round.

    def deploy(self, view):
        self.round = view["round"]
        self.seen = view
        position = self.position = opening_state(view)[self.side]
        self.old = {unit["index"] for unit in position.get("units", [])}
        self.bought = set()
        self.arrived = set()
        self.reinforce(view)
        self.shop()
        self.settle_arrivals()
        if self.round >= 3 or (self.round == 2 and self.draw.random() < 0.3):
            self.defend()

    def reinforce(self, view):
        offers = opening_state(view).get("reinforce_offers") or []
        if not offers:
            return
        cards = []
        for index, name in enumerate(offers[:-1]):
            if name in self.skills or name in SHUNNED_EQUIPMENT:
                continue
            decision = {"type": "choose_reinforce_item", "index": index, "name": name}
            if self.legal(decision):
                cards.append(decision)
        decline = {"type": "choose_reinforce_item", "index": len(offers) - 1,
                   "name": offers[-1]}
        answer = self.take(self.draw.choice(cards) if cards else decline)
        self.note(answer)

    def note(self, answer):
        """Remembers the units a decision that was not a purchase brought."""
        for event in (answer or {}).get("events", []):
            if "created" in event:
                self.arrived.add(event["created"]["index"])

    def weight(self, start_end, executed, most):
        start, end = start_end
        progress = min((self.round - 1) / MAX_ROUND, 1)
        weight = start + progress * (end - start)
        if most > 0:
            weight += start + min(executed / most, 1) * (end - start)
        return int(weight)

    def shop(self):
        if self.draw.randrange(100) < self.weight(UNLOCK_WEIGHTS, 0, 1):
            self.unlock()
        self.buy()
        executed = {action: 0 for action in WEIGHTS}
        done = set()
        for _ in range(SHOPPING_STEPS):
            choices = [(action, self.weight(WEIGHTS[action], executed[action],
                                            1 if action == "blueprint" else SHOPPING_STEPS))
                       for action in WEIGHTS if action not in done]
            choices = [(action, weight) for action, weight in choices if weight > 0]
            if not choices:
                return
            action = self.draw.choices([a for a, _ in choices], [w for _, w in choices])[0]
            executed[action] += 1
            if not getattr(self, action)():
                done.add(action)

    # Shopping actions; each answers whether it took a decision.

    def counts(self):
        position = self.view()
        other = opening_state(self.seen)["red" if self.side == "blue" else "blue"]
        flyers = sum(1 for unit in other.get("units", [])
                     if self.units.get(unit["name"], {}).get("air"))
        anti_air = sum(1 for unit in position.get("units", [])
                       if self.units.get(unit["name"], {}).get("hits_air"))
        return position, flyers, anti_air

    def target(self, names, flyers, anti_air):
        """The recruitment target among `names`, by cost and the air threat."""
        names = [name for name in names if name in self.units]
        if anti_air == 0 and flyers > 0:
            only = [name for name in names if self.units[name]["hits_air"]]
            names = only or names
        elif anti_air < flyers:
            names = names + [name for name in names if self.units[name]["hits_air"]
                             and self.draw.randrange(flyers) < flyers - anti_air]
        weights = []
        for name in names:
            cost = self.units[name]["supply"]
            band = (30 - 5 * self.round if cost <= 100
                    else 18 - 2 * self.round if cost <= 200
                    else 5 * self.round - 10)
            weights.append(max(band, 0))
        if not names:
            return None
        if sum(weights) == 0:
            return self.draw.choice(names)
        return self.draw.choices(names, weights)[0]

    def unlock(self):
        position, flyers, anti_air = self.counts()
        unlocked = set(position.get("unlocked_units", []))
        locked = [name for name in self.units if name not in unlocked
                  and self.legal({"type": "unlock_unit", "name": name})]
        name = self.target(locked, flyers, anti_air)
        return name is not None and self.take({"type": "unlock_unit", "name": name}) is not None

    def buy(self):
        position, flyers, anti_air = self.counts()
        supply = position.get("supply", 0)
        names = [name for name in position.get("unlocked_units", [])
                 if self.units.get(name, {}).get("supply", supply + 1) <= supply]
        name = self.target(names, flyers, anti_air)
        if name is None:
            return False
        for place in self.places(name, position):
            decision = {"type": "buy_unit", "name": name, "position": place}
            if self.legal(decision):
                answer = self.take(decision)
                for event in (answer or {}).get("events", []):
                    if "created" in event:
                        self.bought.add(event["created"]["index"])
                return answer is not None
        return False

    def research(self):
        position = self.view()
        loadout = self.seen["document"][0][self.side]["tech_loadout"]
        held = {unit: set(techs) for unit, techs in (position.get("techs") or {}).items()}
        types = {}
        for unit in position.get("units", []):
            types[unit["name"]] = types.get(unit["name"], 0) + 1
        candidates, weights = [], []
        for name, count in types.items():
            if count < 2 and self.round == 1:
                continue
            techs = [tech for tech in loadout.get(name, []) if tech not in held.get(name, set())
                     and self.legal({"type": "upgrade_technology", "unit": name, "tech": tech})]
            if techs:
                candidates.append((name, techs))
                weights.append(30 * self.round if count < 2
                               else max(4 * count - 2 * len(held.get(name, set())), 1))
        if not candidates:
            return False
        name, techs = self.draw.choices(candidates, weights)[0]
        return self.take({"type": "upgrade_technology", "unit": name,
                          "tech": self.draw.choice(techs)}) is not None

    def upgrade(self):
        position = self.view()
        units = [unit for unit in position.get("units", [])
                 if self.full(unit) and self.legal({"type": "upgrade_unit", "index": unit["index"]})]
        if not units:
            return False
        unit = self.draw.choice(units)
        return self.take({"type": "upgrade_unit", "index": unit["index"]}) is not None

    @staticmethod
    def full(unit):
        exp = unit.get("exp")
        if not exp:
            return True
        have, need = (int(part) for part in exp.split("/"))
        return have >= need

    def equip(self):
        position = self.view()
        items = [item["name"] for item in position.get("equipment", []) or []
                 if item["name"] not in SHUNNED_EQUIPMENT]
        self.draw.shuffle(items)
        for item in items:
            for unit in position.get("units", []):
                decision = {"type": "use_equipment", "name": item, "index": unit["index"]}
                if self.legal(decision):
                    return self.take(decision) is not None
        return False

    def blueprint(self):
        names = [name for name in self.blueprints
                 if self.legal({"type": "active_blueprint", "name": name})]
        if not names:
            return False
        return self.take({"type": "active_blueprint",
                          "name": self.draw.choice(names)}) is not None

    # Placement.

    def places(self, name, position):
        """Places to try for a formation of `name`, the preferred one first."""
        unit = self.units[name]
        old = [u for u in position.get("units", []) if u["index"] in self.old]
        if old:
            near = self.draw.choice(old)
            x = near["position"]["x"] + self.draw.uniform(-2, 2) * unit["width"]
            y = near["position"]["y"] + self.draw.uniform(-2, 2) * unit["depth"]
            if name in BACK_LINE:
                front = max(u["position"]["y"] for u in position.get("units", []))
                if y > front:
                    y = front - unit["depth"] - self.draw.randrange(3 * unit["depth"])
        else:
            x = CENTRE[0] + self.draw.uniform(-0.5, 0.5) * WIDTH
            y = CENTRE[1] + self.draw.uniform(-0.5, 0.5) * HEIGHT
        x0, y0 = self.snap(x, unit["width"], X_MIN, X_MAX), self.snap(y, unit["depth"], Y_MIN, Y_MAX)
        seen = set()
        for ring in range(0, 12):
            for dx in range(-ring, ring + 1):
                for dy in range(-ring, ring + 1):
                    if max(abs(dx), abs(dy)) != ring:
                        continue
                    place = (self.snap(x0 + 10 * dx, unit["width"], X_MIN, X_MAX),
                             self.snap(y0 + 10 * dy, unit["depth"], Y_MIN, Y_MAX))
                    if place not in seen:
                        seen.add(place)
                        yield {"x": place[0], "y": place[1]}

    @staticmethod
    def snap(value, size, low, high):
        """The grid point nearest `value` a formation of `size` stands on."""
        offset = 5 if (size // 10) % 2 else 0
        value = min(max(value, low + size / 2), high - size / 2)
        return int(round((value - offset) / 10) * 10 + offset)

    def settle_arrivals(self):
        position = self.view()
        for unit in position.get("units", []):
            if unit["index"] in self.arrived and unit["index"] not in self.bought:
                for place in self.places(unit["name"], position):
                    decision = {"type": "move_unit", "index": unit["index"], "position": place}
                    if self.legal(decision):
                        self.take(decision)
                        break

    def defend(self):
        position = self.view()
        other = opening_state(self.seen)["red" if self.side == "blue" else "blue"]
        # The other side's left, its own negative x, faces this side's positive x.
        left = sum(1 for unit in other.get("units", []) if unit["position"]["x"] > 0)
        right = sum(1 for unit in other.get("units", []) if unit["position"]["x"] < 0)
        toward_left = self.draw.random() < (0.5 if left == right else left / (left + right))
        x = CENTRE[0] + (-WIDTH // 4 if toward_left else WIDTH // 4)
        x += self.draw.uniform(-0.5, 0.5) * WIDTH / 8
        y = CENTRE[1] + self.draw.uniform(-0.5, 0.5) * HEIGHT / 16
        movable = [unit for unit in position.get("units", []) if unit["index"] not in self.bought
                   and self.legal({"type": "move_unit", "index": unit["index"],
                                   "position": unit["position"]})]
        self.draw.shuffle(movable)
        for unit in movable[:self.draw.randint(1, 2)]:
            size = self.units.get(unit["name"], {"width": 20, "depth": 20})
            for dx in range(0, 60, 10):
                place = {"x": self.snap(x + dx, size["width"], X_MIN, X_MAX),
                         "y": self.snap(y, size["depth"], Y_MIN, Y_MAX)}
                decision = {"type": "move_unit", "index": unit["index"], "position": place}
                if self.legal(decision):
                    self.take(decision)
                    break

    # The match.

    def play(self):
        while True:
            view = self.ask({"op": "match.show", "wait": True})
            if view.get("schema") == "mechcore.error":
                print(f"cannot read the match: {view['reason']}", file=sys.stderr)
                return 1
            self.side = view["side"]
            phase = view["phase"]
            if phase == "over":
                return 0
            if view["sides"][self.side]["committed"] or phase not in ("opening", "deploy"):
                continue
            if phase == "opening":
                self.opening(view)
            else:
                self.deploy(view)
            answer = self.ask({"op": "match.commit"})
            if answer.get("schema") == "mechcore.error":
                print(f"commit refused: {answer['reason']}", file=sys.stderr)
                return 1


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--name", action="store_true", help="print the bot name and exit")
    parser.add_argument("--seed", type=int, help="the bot's random seed; independent of the player seed, random when omitted")
    args = parser.parse_args()
    if args.name:
        print("game-ai")
        return 0
    return Player(args.seed).play()


if __name__ == "__main__":
    sys.exit(main())
