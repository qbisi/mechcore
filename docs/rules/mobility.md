# Deployment mobility

[简体中文](mobility.zh.md)

This index is pinned to game build 2259. It states which units a side may
move during deployment, what frees a unit that may not, and the evidence
for both.

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

`TerritoryManager.CanMoveUnits`, which every recorded move passes through,
asks `CardElement.CanMovable()` of each unit, and that is one comparison:
`mobilityState != Limit`. `MobilityState` has three values, `Once` (0),
`Limit` (1) and `Free` (2).

Redeploy's `PAP_ReleaseCommanderSkill.ChangeMobilityState` writes `Once` to its
target and keeps the state it replaced, which `UndoChangeMobilityState` puts
back. That much is read from the build's code.

Which of the other writes sets `Limit` as a round opens, and `Free` for the
Module and the Jump Drive, is not read: no call site names
`CardElement.SetMobilityState`, so the writes are inlined where the code was
not followed. The rule above rests for those parts on the corpus below.

## Evidence

The local observation set records 3,735 moves across 41 matches. Every one is of
a unit the rule lets move:

| Why it could move | Moves |
| --- | ---: |
| bought or handed out this round | 2,812 |
| delivered as the round opened | 790 |
| wearing the Deployment Module | 43 |
| its unit's Jump Drive researched | 40 |
| targeted by a standing Redeploy this round | 50 |

A unit "delivered as the round opened" is one at or above the unit
allocator the previous round closed with. The snapshot's own `roundCount` does
not separate those from units bought the round before: an officer's squad
delivered as a round opens can already read 1.

The corpus holds no move the game refused, so it shows the rule is not too
strict and cannot show it is not too lenient. A unit that stays in place
and is not moved is consistent with either.
