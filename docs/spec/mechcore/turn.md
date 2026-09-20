# Match turn file

[TOC]

## Scope

This contract defines the file a match in progress keeps beside its document:
what it holds, how two processes share it, and what is lost when it is.
`<match>.turn` sits beside `<match>.yaml`, which is the
[battle](../document/battle.md) document holding the rounds that have been
played. The command that reads and writes both is
[cli.md](cli.md)'s `match` namespace.

The turn file is coordination, not record. It holds the round in progress, and
a match that is over has none: everything a reader needs afterwards is in the
battle document. Nothing about a fight, a position or a transition is here;
they are decided by the rules and written to the document.

A match is cooperative. The file says which side each player was given and
keeps each side's decisions apart until they are committed, so that two
players deploying at once do not read each other's board. It is not a
permission system: a process that reads the file reads both sides, and nothing
here stops one from naming a side it was not given.

## Document shape

One JSON object, written whole:

```json
{
  "schema": "mechcore.match-turn.v1",
  "round": 3,
  "opened": "2026-09-20T01:02:03.123456Z",
  "rebuilt": false,
  "sides": {
    "blue": {"given": true, "committed": true, "decisions": []},
    "red": {"given": true, "committed": false, "decisions": [
      {"type": "buy_unit", "name": "marksman"},
      {"type": "move_unit", "index": 7, "position": {"x": 40, "y": -150}}
    ]}
  }
}
```

### `round`

The round being deployed, which is the round the document holds a state for and
whose actions are not both written yet. Round zero is the opening, and its one
decision per side is a `choose_advance_team`.

### `opened`

When that round opened, as an RFC 3339 instant in UTC. It is the only clock a
match reads, and it decides one thing: the round is fought when the header's
deployment time has passed since it, whether or not both sides have committed.

A round opens when the fight before it is resolved, and `opened` is written in
the same locked write that opens it.

### `rebuilt`

Whether this round's file was rebuilt rather than carried, which means its
clock restarted and any uncommitted decisions were lost. It is written when a
file is rebuilt and cleared when the next round opens, so it describes the
round it stands in and not the match. A side waiting on a round learns from it
why the wait grew, rather than waiting an unexplained second time.

### `sides`

One entry each for `blue` and `red`.

| Field | Means |
| --- | --- |
| `given` | that side has been handed to a player by `match new`; a third caller is refused |
| `committed` | that side has written this round's decisions to the document |
| `decisions` | what that side has decided this round and not committed, in the order taken |

A side's `decisions` are the [action](../document/action.md) spec's actions,
exactly as a document writes them, and they are the side's own until it
commits. Committing collapses them into the normal form a battle is written in,
writes them to the document's round, empties `decisions` and sets `committed`.

`committed` is not derivable from the document: a side that committed without
deciding anything and a side that has not committed both leave an empty list
there, and only this file tells them apart while the round is open.

## Normal form

The object is written with the keys in the order above, two-space indentation
and a closing newline, and a side's `decisions` in the order they were taken.
A match writes the whole file rather than editing it, so two writes of one
state are one byte sequence.

## Excluded fields

- **Positions.** A side's position is the round's opening state with its
  decisions applied, which the rules produce; storing it would be a second
  answer to a question that already has one.
- **The fight, and where it came from.** A fight is run from the match and its
  seed, and what it did reaches the document as the next round's state.
- **Who is resolving the fight.** The lock is held for the whole of a fight, so
  a fight cannot be started twice, and a process that dies during one releases
  the lock to the next: there is no state to time out and no death to detect.
- **Process identity.** No pid, no host, no token. A side is named on each
  operation by the player that was given it.
- **Anything about a finished round.** The document holds those.

## Sharing it

The file carries one advisory lock. An operation that writes takes it, reads
the file, decides, writes the file, and releases it; an operation that only
reads a view may take it or not, and a reader that finds a half-written file
treats it as unreadable rather than guessing. A fight is resolved inside the
lock, so the round after it opens in the same write that ends the round before.

The file is written in place rather than replaced, because the lock belongs to
the file a caller opened, and replacing it would leave another process holding
a lock on a file nobody else can see.

## Losing it

A turn file may be deleted or found unreadable. That loses what it holds and
nothing else: the uncommitted decisions of both sides, and the round's clock.

It is rebuilt from the document, at the round the document is in, with both
sides given, no decisions, no commits, the clock restarted and `rebuilt` set. A
match whose turn file was rebuilt therefore keeps every round it has played,
and both players carry on by naming the side they already had; a player that
had not yet joined cannot join it afterwards.

A match that reaches its end deletes the file. What goes with it is the last
round's uncommitted decisions, which were never played, and a clock no round is
waiting on. What a reader wants afterwards is the battle document, which holds
every round that was.

## Unresolved

None.
