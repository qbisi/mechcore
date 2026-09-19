# Reinforcement dealing

These rules cover build **1.11.1.3.2259**, standard versus 1v1 with no game
rules, after the [opening initialization](opening.md). The seed determines a
stream and initial pools. Later deals also depend on both players' fielded
units, unlocked units, active technologies and officers, and previous card
choices. Predicting a deal does not predict those decisions or combat outcomes.

## Stream and pool lifetime

`ReinforcementSystem.OnEnterDeployment` dispatches to
`GenerateReinforceItems`. Round 1 deals nothing. Subsequent rounds use the
reinforcement stream left by the opening without reseeding it. A replay's
`MatchSnapshot.randomState` is the state before that round's generation;
`reinforceItems` is the resulting ordered deal. The unit round pool selected
during initialization decides whether to use ordinary or unit reinforcement.
A unit round bypasses ordinary pool refresh and ordinary dealing.

Ordinary cards have scope 1 and either no scene restriction or scene 1. Each
positive officer type group initially contributes its selected representative;
zero-type entries contribute individually. Pools are sorted by level, then ID.
Choosing a nonrepeatable card removes it from the pool and clears its positive
type group's remaining replacement variants. Declining and choosing a
repeatable card leave the pools intact. These operations consume no randomness.

## Ordinary reinforcement

`ReinforcePool.OnNewRound` checks the union of both players' fielded unit types.
For officer `appearCondition = 1`, every type in its `unitID` list must be
absent. A failed representative is removed. Among its group's remaining
variants satisfying that condition, one replacement is chosen uniformly,
including a random call when only one remains. No eligible variant means no
replacement. Levels and original IDs are visited in ascending order;
replacements are appended only after that traversal, then pools are sorted.

`GetCurrentReinforceItemProbabilityData` uses the last probability row whose
round is no greater than the current round. `GenLvProbs` sets a level's weight
to zero when it has no allowed candidate. Candidates satisfy `earliestRound`
and, when positive, `latestRound`. The configured weights are:

| Round | Level 1 | Level 2 | Level 3 | Level 4 | Level 5 |
| --- | --- | --- | --- | --- | --- |
| 2 | 60 | 40 | 0 | 0 | 0 |
| 3 | 30 | 40 | 30 | 0 | 0 |
| 4 onward | 25 | 25 | 25 | 25 | 0 |

`RoundRand` deals four cards. For each card it draws uniformly over the sum of
remaining level weights, then `RandOnce` draws within that level's candidate
window. The window starts at the number already taken from that level. Its
indices refer to the full pool; a selected index is swapped with that pool's
current take position. If only one candidate remains in the window, selection
consumes no random call and performs no swap. Exhausting a level zeros its
weight. Pools are sorted again after the deal.

If a deal contains a level-4 `CommanderSkillBase`, all level-4 commander skills
are excluded on the next numeric round. This depends on what was offered,
regardless of what was chosen. A unit round does not defer this exclusion to a
later ordinary round.

## Unit reinforcement

`GenerateUnitReinforceItems` requests four cards whose `activeRound` equals the
current round. Eligible rows have scope 0 and the same scene filter. Active
officers with `preventUnitReinforcements` exclude their listed unit types.
Neither pass can offer a unit type already selected by that deal.

`UnitFirstRand` additionally excludes currently fielded unit types, then draws
uniformly from candidates sorted by card ID. Its counter starts at the lesser
of the requested count and candidate count. Each draw decrements the counter
once for selection **and once for each removed variant of the chosen unit**.
The native loop contains both decrements. Consequently this pass can return
fewer than four cards even when more eligible unit types remain.

`UnitSecondRand` fills the remaining slots using an investment score for each
unit type across both players:

- Each fielded unit adds `CardData.baseMoney`, without level scaling.
- Each unlocked shop unit adds `CardData.unlockPrice`.
- For types present in that combined map, each player's active technologies
  add `UnitTechnologyManager.CalculateUpgradeCost`.

The technology calculation follows `UnitUtility.CalculateUpgradeTechnologyCost`:
base upgrade price plus the number of earlier active technologies times the
unit's positive `techUpgradeIncreaseSupplyPerCount`, or the global
`Config.upgradeTechnologyCostIncreaseDelta` otherwise. A positive
`techUpgradeMaxSupplyLimit` caps each term. This is a base investment measure;
paid prices and officer discounts are not used. Standard unit rows have zero
step overrides and zero caps, so their step is 200 and technology ordering does
not affect the sum. Nonstandard capped unit types are outside this rule's
verified scope.

Candidates are ordered by score, then card ID; missing unit scores are zero.
Each remaining slot draws uniformly among the minimum-score candidates and
removes every variant of that chosen unit. A singleton still consumes a draw.
Both passes use the same reinforcement stream.

After both passes, a round at or beyond the selected schedule's `supplyRound`
replaces the last offer with `supplyReinforceID`, indexed by
`round - supplyRound` and clamped to the final entry. Random draws for the
replaced card have already happened. This branch is specified by the native
method and resource row; late supply substitution has no replay coverage in
the verification corpus.

## Declining

Every round offers a decline beside its deal, and the decline pays supply. An
ordinary round pays `Config.noReinforcementSupply`, 50. A unit round pays the
selected schedule's `giveUpSupply` at the round's place in `roundGroup`:
`ReinforcementSystem.m_RoundGiveUpSupply` is built from
`UnitReinforceRoundPool.GetGiveUpSupply(round)`, which reads exactly that.
Schedule 1, units in rounds 2, 5, 8 and 11, pays 50, 150, 400 and 700. The 55
standard schedules hold 55 distinct lists, and the figure grows with the round
it pays for: 50 to 150 for the first unit round, then 150 to 350, 400 to 600
and 700 to 900.

No local replay shows a unit-round decline together with the round after it:
the 51 locally recorded build-2259 replays decline 53 times, all in ordinary
rounds, and the one unit-round decline among the downloaded ones ends its
match. The unit-round figure is the configuration's, not a measured one.

## Inputs and evidence boundary

[config/reinforcements.yaml](../../config/reinforcements.yaml) is extracted by
`scripts/extract_reinforcements.py` from the configuration export and `level0`.
All values are integers or flags from those resources; there are no fitted
weights, stream offsets or score coefficients.

| Input | Resource source and units |
| --- | --- |
| Ordinary count 4 | Config, MonoBehaviour 136, serialized body byte 108 (`reinforceItemCount`, native field offset 148); cards |
| Unit count 4 | `commonParms.unit_reinforcement_quantity`; cards |
| Level weights | `reinforceItemProbabilityDatas`; relative integer weights |
| Eligibility, levels, groups, round limits, repetition | Officer rows in `ConfigDataContainer`; commander skills at MonoBehaviour 167 and equipment at 188 in `level0` |
| Unit candidates and schedules | `unitReinforceDatas`, `unitReinforceRoundPool`; IDs, integer rounds and each unit round's decline supply |
| Unit investment | `cardDatas.baseMoney`, `unlockPrice`, technology step and cap fields; supply |
| Technology prices and global step | [Technology pricing](unit_techs.md); supply |

The native call chain is `OnEnterDeployment` → `GenerateReinforceItems` →
`GenerateRoundReinforceItems` / `GenerateUnitReinforceItems`. Ordinary dealing
closes on `ReinforcePool.OnNewRound`, `CheckReinforeCondition`, `CheckAppear`,
`GenLvProbs`, `RoundRand`, `RandOnce` and `SelectReinforce`. Unit dealing closes
on `UnitFirstRand`, `UnitSecondRand`, their sort callbacks and the technology
cost methods. The skill cooldown cast is bound to `CommanderSkillBase` through
the binary metadata slot, rather than inferred from matching offers. Native
instructions resolve the first-pass double decrement and the cost method's
tail call where the intermediate decompiler representation is incomplete.

The binary and resources have the [opening artifact identities](opening.md#evidence-and-boundary).
The evidence is native control flow plus extracted resource fields, checked
against replay offer arrays and successive native random states. Modified
pools, other modes, negative seeds and nonstandard capped unit types remain
outside the supported scope. A last recorded round has no following snapshot
to independently check its outgoing state. Reopen when those artifact
identities change, scope expands, or a native deal or stream boundary disagrees
with this flow.
