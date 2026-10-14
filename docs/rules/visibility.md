# What a player knows about the other

What the game gives a player about the other side, and how information is
divided across three tiers: what the client holds in data, what the interface
shows a human player, and what `match show` exposes on this platform.

## Three tiers of visibility

Information visibility is structured across three distinct layers:

1. **Client data**: fully transparent between both sides, except for the
   opponent's in-progress decisions in the round currently being deployed.
2. **Player interface**: what a human sees in the UI, which conceals hidden
   seeds, unfielded options, and uncommitted deployment actions under the fog of
   war.
3. **Platform (`match show`)**: designed for fair and convenient match
   automation and analysis; exposes the match file contents publicly, except
   for the opponent's actions in the round currently in progress, whether or
   not that opponent has committed.

## The client layer: full transparency except in-progress decisions

A locally recorded replay is written by the client that played the match, and
it carries both players' round snapshots whole. One of those snapshots holds
what no interface shows an opponent: the other player's supply, the units it
has unlocked (`shop.unlockedUnits`), its officers, and its research.

A replay's `Seat` names the seat of the player who recorded it, and the other
seat's round snapshots carry that player's data in full. Between game clients,
all committed states and historical positions are bidirectionally transparent.
The only information withheld from the client is the opponent's in-progress,
uncommitted actions during the current round.

So hiding is the interface's work, not the client's. A player's machine can
answer any question about the other side; what the game decides is what it
draws.

## The player layer: what the interface presents

The interface draws and conceals information according to the following rules:

### Board visibility and deployment fog

The deployment interface covers the other side under a fog volume
(`FogOfWarController`):

- **Before committing deployment**: A player sees only the opponent's board
  state from the previous round (standing units from earlier rounds), or in round
  0, the opening advance team and specialist-delivered units. The opponent's
  actions during the ongoing round remain hidden.
- **After committing deployment**: When a player commits deployment, the fog
  lifts (`FogOfWarController.OnTeamFinishDeploy`). The player can now observe
  the opponent's entire board in real time, with the opponent's stacked actions
  applied.

### Unit inspection

Inspecting an enemy unit on the board reveals all of its configuration: its
current level, its fitted equipment, and all unit technologies unlocked on it.

### Public game state and panels

The following components are mutually visible to both players:

- Reinforcement offers and taken cards.
- Officers and their statuses.
- The commander skill panel.
- The equipment panel and unfitted equipment.
- Research Center blueprints.
- Energy Tower skill statuses.

### Concealed from the player

The interface does not present:

- The three random seeds: the match seed and each side's individual seed.
- The opponent's shop unlocked unit list (`shop.unlockedUnits`).
- Technology configurations of enemy unit types that have not yet been fielded.
- Red vs. Blue seat identity is difficult to distinguish from the board alone,
  as the UI renders each player's view oriented with their own territory toward
  negative `y`.

Aside from these concealed items, the player's potential knowledge aligns with
the client data layer.

## The platform layer: match show visibility

A match document records an ordered list of decisions. A player never saw one:
the game shows boards, and a human infers what happened by comparing the board
before a round with the board after it. Two different decision lists can leave
one board, so an ordered list says more than any interface could:

- a unit upgraded and then given equipment, and the same unit given equipment
  and then upgraded, stand identically at the end of the round;
- a unit type unlocked, bought, researched and then sold leaves the unlock and
  the research in the position and nothing on the board;
- a purchase moved three times and a purchase placed once stand in one place.

The platform contract in [`cli.md`](../spec/mechcore/cli.md) makes the match
document public except for the opponent's actions in the round currently in
progress. Those actions remain hidden whether or not either player has
committed, including actions already written to the match document. This also
applies to the round-zero opening choice. Committing does not make a round
historical: the opponent's actions become visible when that round has ended.
Both sides' round-opening states remain public throughout the round.

## Evidence

### Replayed

- Every locally recorded replay carries both sides whole: every replay of this
  version's corpus converts with both sides' snapshots in every round, whether a
  spectator recorded it or either player: `scripts/corpus/verify-matches.py`.

### Read

- The deployment fog is a client object shown and hidden over a territory, and
  a team finishing its deployment is when it hides: `FogOfWarController.Hide`,
  `FogOfWarController.OnTeamFinishDeploy`.
