"""Python client shared by the reference players; install PyYAML or use uv run --with pyyaml."""

import json
import sys


def problems():
    import yaml

    for line in sys.stdin:
        problem = json.loads(line)
        documents = list(yaml.safe_load_all(problem["match_yaml"]))
        yield Round(problem["side"], problem["round"], documents[0], documents[-1])


class Round:
    def __init__(self, side, number, header, opening):
        self.side = side
        self.other = "blue" if side == "red" else "red"
        self.number = number
        self.header = header
        self.state = opening if number else {}
        self.actions = []
        self.request_id = 0
        # A trial needs one legal answer from each seat. Model the opponent
        # as standing still and declining its reinforcement offer.
        self.opponent = []
        if not number:
            self.opponent = [self.choose_team(self.other, 0)]
        elif offers := self.state.get("reinforce_offers"):
            self.opponent = [{"type": "choose_reinforce_item", "index": len(offers) - 1,
                              "name": offers[-1]["name"]}]

    def choose_team(self, seat, index):
        offer = self.header[seat]["offers"][index]
        return {"type": "choose_advance_team", "index": index,
                "name": offer["team"], "specialist": offer["specialist"]}

    def ask(self, op, actions):
        self.request_id += 1
        print(json.dumps({"op": op, "id": self.request_id, "actions": actions}), flush=True)
        line = sys.stdin.readline()
        if not line:
            raise SystemExit(0)
        reply = json.loads(line)
        if (reply["round"], reply["id"]) != (self.number, self.request_id):
            raise ValueError("unexpected arena reply")
        return reply

    def try_action(self, action, *, keep=False):
        own = self.actions + [action]
        reply = self.ask("act", {self.side: own, self.other: self.opponent})
        if "error" in reply:
            return None
        state = reply["result"]["state"]
        if keep:
            self.actions = own
            self.state = state
        return state

    def commit(self):
        reply = self.ask("commit", self.actions)
        if "error" in reply:
            raise ValueError(reply["error"]["reason"])
