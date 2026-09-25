# What a player knows about the other

What the game gives a player about the other side, and the two answers that
question has: what the client holds, which a replay shows, and what the
interface shows a human, which nothing here does. What a match on this
platform shows a player is not here; that is the `match` namespace of
[`cli.md`](../spec/mechcore/cli.md), which decides it rather than reproducing
it, and which names this document for the difference.

## The client holds both sides in full

A locally recorded replay is written by the client that played the match, and
it carries both players' round snapshots whole. One of those snapshots holds
what no interface shows an opponent: the other player's supply, the units it
has unlocked, its officers and its research.

A replay's `Seat` names the seat of the player who recorded it, and the other
seat's round snapshots carry that player's `shop.unlockedUnits`, `officers` and
`supply` in full.

So hiding is the interface's work, not the client's. A player's machine can
answer any question about the other side; what the game decides is what it
draws.

## What the interface draws is not established here

The deployment interface covers the other side and lifts that cover as the
round settles: `FogOfWarController` shows and hides a fog volume over a player
territory, and it takes `OnTeamFinishDeploy` as the moment to hide it. That is
the shape of a mechanism read from names and call edges; what the fog covers,
and for whom, is not closed.

These remain open, and none of them is answered by reading further:

- What a player sees of the other side's board while both are deploying, and
  what changes when one of them finishes.
- Whether inspecting an enemy unit shows its level, its equipment and the
  technologies it has been given.
- Whether a reinforcement card a player takes is announced to the other, and
  whether an officer's effect is visible anywhere but in what it does.
- Whether the Research Center's blueprints, the Energy Tower's skills and a
  commander skill's panel are visible before they are used.

Each is a question about what is drawn, so each closes the same way: play a
match and record what the interface shows at the moments the adapter captures
the state, then compare. That is a research question with an observation, not
a reading of the binary.

## Why this matters to a document

A battle document records an ordered list of decisions. A player never saw one:
the game shows boards, and a human infers what happened by comparing the board
before a round with the board after it. Two different decision lists can leave
one board, so an ordered list says more than any interface could:

- a unit upgraded and then given equipment, and the same unit given equipment
  and then upgraded, stand identically at the end of the round;
- a unit type unlocked, bought, researched and then sold leaves the unlock and
  the research in the position and nothing on the board;
- a purchase moved three times and a purchase placed once stand in one place.

Those are decisions a player would have no reason to take in that order, which
is why a board is usually enough to infer the list. They are not impossible,
and the difference is the platform's to decide rather than the game's.

## Evidence

### Replayed

- Every locally recorded replay carries both sides whole: every replay of this
  version's corpus converts with both sides' snapshots in every round, whether a
  spectator recorded it or either player: `scripts/verify-battles.py`.

### Read

- The deployment fog is a client object shown and hidden over a territory, and
  a team finishing its deployment is when it hides: `FogOfWarController.Hide`,
  `FogOfWarController.OnTeamFinishDeploy`.

### Not established

- **What the interface draws of the other side**, for each of the questions
  above.
