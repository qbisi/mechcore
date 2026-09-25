# Deployment mobility

Which units a side may move during deployment, and what frees a unit that may
not.

A state carries the answer per unit as `movable`;
[`state.md`](../spec/document/state.md#a-unit-carries-two-fields-a-layout-does-not)
defines the field and [`action.md`](../spec/document/action.md#move_unit) the
decisions that write it.

## The rule

A unit may move in the round it arrives, and stays where it is in every
later round unless something frees it. It arrives when it is bought, when a
reinforcement card hands it out, or when it is delivered as a round opens, which
is how the opening's force and an officer's squads reach the board.

Three things free a unit that has stayed from an earlier round:

| What | ID | Frees |
| --- | --- | --- |
| Deployment Module, 部署模块 | equipment `13040001` | the unit that wears it, in every round |
| Jump Drive, 高速引擎 | technology `1606` Wasp, `1611` Overlord, `1616` Phoenix | every unit of that type, in every round |
| Redeploy, 再部署 | commander skill `1000001` | the unit it targets, for the rest of the round |

Redeploy is a deployment skill: it changes the position before the fight, so a
round marks its slot `used` rather than released.

## What the game checks

`TerritoryManager.CanMoveUnits`, which every move passes through, asks
`CardElement.CanMovable()` of each unit, and that is one comparison:
`mobilityState != Limit`. `MobilityState` has three values, `Once`, `Limit` and
`Free`.

Redeploy's `PAP_ReleaseCommanderSkill.ChangeMobilityState` writes `Once` to its
target and keeps the state it replaced, which `UndoChangeMobilityState` puts
back.

Which of the other writes sets `Limit` as a round opens, and `Free` for the
Module and the Jump Drive, is not read: no call site names
`CardElement.SetMobilityState`, so the writes are inlined where the code was
not followed.

A unit "delivered as the round opened" is one at or above the unit allocator
the previous round closed with. The snapshot's own `roundCount` does not
separate those from units bought the round before: an officer's squad delivered
as a round opens can already read 1.

## Evidence

### Replayed

- A unit that arrived this round, a unit wearing the Deployment Module and a
  unit whose type's Jump Drive is researched may move, and every other one from
  an earlier round may not: every recorded move of this version's corpus is of a
  unit the rule lets move, and every formation's `movable` agrees, in battles
  that fit the Module and battles that research a Jump Drive:
  `scripts/verify-battles.py`.

### Read

- A move is allowed for a unit whose mobility state is not `Limit`:
  `TerritoryManager.CanMoveUnits`, `CardElement.CanMovable`.
- Redeploy writes `Once` to its target and restores the replaced state on undo:
  `PAP_ReleaseCommanderSkill.ChangeMobilityState`,
  `PAP_ReleaseCommanderSkill.UndoChangeMobilityState`.

### Not established

- **Which writes set `Limit` and `Free`.** They are inlined and not read; the
  rule is what the corpus replays.
- **That the rule is not too lenient.** A corpus holds no move the game refused,
  so it shows the rule is not too strict and cannot show it lets through only
  what the game does. A unit that stays in place is consistent with either.
