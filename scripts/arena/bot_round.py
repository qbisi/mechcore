"""Pure standard-library JSON-RPC 2.0 client shared by the reference players."""

import json
import sys


def serve(name, solve_round):
    """Run the JSON-RPC 2.0 stdio loop for `mechcore solve` / `mechcore arena`."""
    for raw in sys.stdin:
        line = raw.strip()
        if not line:
            continue
        msg = json.loads(line)
        method = msg.get("method")
        msg_id = msg.get("id")
        if method == "initialize":
            reply = {"jsonrpc": "2.0", "id": msg_id, "result": {"name": name}}
        elif method == "solve":
            params = msg["params"]
            round_session = Round(
                params["side"], params["problem"], params.get("limits") or {}
            )
            solve_round(round_session)
            reply = {
                "jsonrpc": "2.0",
                "id": msg_id,
                "result": {"actions": round_session.actions},
            }
        else:
            reply = {
                "jsonrpc": "2.0",
                "id": msg_id,
                "error": {"code": -32601, "message": f"unknown method {method!r}"},
            }
        print(json.dumps(reply), flush=True)


class Round:
    """Single-round problem context with reverse stateless `act` and `simulate`."""

    def __init__(self, side, problem, limits):
        self.side = side
        self.other = "blue" if side == "red" else "red"
        self.problem = problem
        self.opening_state = problem["state"]
        self.state = problem["state"]
        self.history = problem.get("history") or []
        self.ref = problem.get("ref")
        self.limits = limits
        self.number = int(self.state["round"])
        self.actions = []
        self.request_id = 0

    def choose_team(self, seat, index):
        offer = self.state[seat]["offers"][index]
        return {
            "type": "choose_advance_team",
            "index": index,
            "name": offer["team"],
            "specialist": offer["specialist"],
        }

    def ask(self, method, params):
        self.request_id += 1
        req_id = self.request_id
        print(
            json.dumps(
                {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params}
            ),
            flush=True,
        )
        while True:
            line = sys.stdin.readline()
            if not line:
                raise SystemExit(0)
            if not line.strip():
                continue
            reply = json.loads(line)
            if reply.get("id") != req_id:
                raise ValueError(
                    f"unexpected RPC reply id {reply.get('id')!r}, expected {req_id!r}"
                )
            return reply

    def _context(self):
        return {"ref": self.ref} if self.ref is not None else {"state": self.opening_state}

    def try_action(self, action, *, keep=False):
        own = self.actions + [action]
        reply = self.ask("act", {"context": self._context(), "actions": {self.side: own}})
        if "error" in reply:
            return None
        state = reply["result"]["state"]
        if keep:
            self.actions = own
            self.state = state
        return state
