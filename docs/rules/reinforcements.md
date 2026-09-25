# Reinforcement dealing

These rules cover standard versus 1v1 with no game rules, after the
[opening initialization](opening.md). The seed determines a stream and initial
pools. Later deals also depend on both players' fielded units, unlocked units,
active technologies and officers, and previous card choices. Predicting a deal
does not predict those decisions or combat outcomes.

The replays behind these rules are build 1.11.1.3.2259's. Build 2.0.0.1.2324's
dump keeps every method named below; where 2.0 adds a rule, the section says
so and says what is read rather than recorded.

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

`ReinforcePool.OnNewRound` asks `CheckReinforeCondition` of every officer card
in the pools. A card that fails is removed. Among its group's remaining
variants that pass, one replacement is chosen uniformly, including a random
call when only one remains. No eligible variant means no replacement. Levels
and original IDs are visited in ascending order; replacements are appended
only after that traversal, then pools are sorted.

The condition is the officer's `appearCondition` (`EAppearCondition`):

- **0, None:** always passes.
- **1, NonexistUnit:** every type in the officer's `unitID` list must be absent
  from the union of both players' fielded unit types.
- **2, SupplyPercent**, new in build 2.0: see the next section.

`GetCurrentReinforceItemProbabilityData` uses the last
`reinforceItemProbabilityDatas` row whose round is no greater than the current
round, whose five columns are relative integer weights of levels 1 to 5.
`GenLvProbs` sets a level's weight to zero when it has no allowed candidate.
Candidates satisfy `earliestRound` and, when positive, `latestRound`.

`RoundRand` deals `Config.reinforceItemCount` cards. For each card it draws
uniformly over the sum of remaining level weights, then `RandOnce` draws within
that level's candidate window. The window starts at the number already taken
from that level. Its indices refer to the full pool; a selected index is
swapped with that pool's current take position. If only one candidate remains
in the window, selection consumes no random call and performs no swap.
Exhausting a level zeros its weight. Pools are sorted again after the deal.

If a deal contains a level-4 `CommanderSkillBase`, all level-4 commander skills
are excluded on the next numeric round. This depends on what was offered,
regardless of what was chosen. A unit round does not defer this exclusion to a
later ordinary round.

## Investment share (build 2.0), read and not recorded

Build 2.0 gives the unit-modification officers `appearCondition` 2,
`SupplyPercent`, with an `appearConditionParameter` pair `[p0, p1]`.
`ReinforcementRandomObject_Common.Generate` builds one `PlayerUnitEconomy` per
real player of the match (`Match.playerManager.realPlayerControllers`, both
sides) with `CollectPlayerUnitEconomies`, and hands the list to
`ReinforcePool.OnNewRound`, which passes it to `CheckReinforeCondition`.

A player's economy values each unit type it holds:

- each of its formations of the type adds
  `UnitUtility.CalculateUpgradeLevelCost`, the card's `GetMoney` plus
  `GetLevelUpSupplyCost` of every level up to the formation's own;
- the type adds its `CardData.unlockPrice` once;
- the type adds `UnitTechnologyManager.CalculateUpgradeCost`, its active
  technologies;

and `TotalValue` is the sum over its types.

For every economy with `TotalValue > 0`, `CheckReinforeCondition` sums the
values of the officer's `unitID` types and requires, when `p0 ≤ p1`,
`TotalValue × p0 ≤ 100 × sum ≤ TotalValue × p1`. One economy outside fails the
card. So an officer that modifies a unit is dealt only while neither player
has more than `p1` percent of its investment in that unit.

Not established:

- which of a player's formations the economy walks (standing ones only, or
  also sold or lost ones), and the `CardLevel` a level-1 formation passes;
- the branch for `p0 > p1`, which reads as passing outside the interval, and
  a loop flag set when `p0 > 0`; no standard officer uses either;
- any recording: no 2.0 replay or capture has been checked against it.

The reinforcement prediction refuses a pool that holds a card with this
condition, rather than dealing it as if unconditioned.

## Unit reinforcement

`GenerateUnitReinforceItems` requests `commonParms.unit_reinforcement_quantity`
cards whose `activeRound` equals the current round. Eligible rows have scope 0
and the same scene filter. Active officers with `preventUnitReinforcements`
exclude their listed unit types. Neither pass can offer a unit type already
selected by that deal.

`UnitFirstRand` additionally excludes currently fielded unit types, then draws
uniformly from candidates sorted by card ID. Its counter starts at the lesser
of the requested count and candidate count. Each draw decrements the counter
once for selection **and once for each removed variant of the chosen unit**.
The native loop contains both decrements. Consequently this pass can return
fewer cards than requested even when more eligible unit types remain.

`UnitSecondRand` fills the remaining slots using an investment score for each
unit type across both players. It is its own, not the economy the previous
section describes:

- Each fielded unit adds `CardData.baseMoney`, without level scaling.
- Each unlocked shop unit adds `CardData.unlockPrice`.
- For types present in that combined map, each player's active technologies
  add `UnitTechnologyManager.CalculateUpgradeCost`.

The technology calculation follows `UnitUtility.CalculateUpgradeTechnologyCost`:
base upgrade price plus the number of earlier active technologies times the
unit's positive `techUpgradeIncreaseSupplyPerCount`, or the global
`Config.upgradeTechnologyCostIncreaseDelta` otherwise. A positive
`techUpgradeMaxSupplyLimit` caps each term. This is a base investment measure;
paid prices and officer discounts are not used. Standard unit rows set neither
override nor cap, so technology ordering does not affect the sum.

Candidates are ordered by score, then card ID; missing unit scores are zero.
Each remaining slot draws uniformly among the minimum-score candidates and
removes every variant of that chosen unit. A singleton still consumes a draw.
Both passes use the same reinforcement stream.

After both passes, a round at or beyond the selected schedule's `supplyRound`
replaces the last offer with `supplyReinforceID`, indexed by
`round - supplyRound` and clamped to the final entry. Random draws for the
replaced card have already happened. No replay covers this substitution; it
is the native method and the resource row.

## Declining

Every round offers a decline beside its deal, and the decline pays supply. An
ordinary round pays the map's `MatchSetting.giveUpSupply` on build 2.0, which
was `Config.noReinforcementSupply` on 2259; the getter is inlined wherever it
is read, so the match is by name and value, not by call chain. A unit round
pays the selected schedule's `giveUpSupply` at the round's place in
`roundGroup`: `ReinforcementSystem.m_RoundGiveUpSupply` is built from
`UnitReinforceRoundPool.GetGiveUpSupply(round)`, which reads exactly that.

No replay shows a unit-round decline together with the round after it, so the
unit-round figure is the configuration's, not a measured one.

## Inputs and evidence

[config/reinforcements.yaml](../../config/reinforcements.yaml) is written by
`scripts/extract_reinforcements.py` from the build's typed export: the officer,
unit card, round pool, probability and card rows of `ConfigDataContainer`, the
commander skill and equipment cards of `CommanderSkillGroupData` and
`EquipmentGroupData`, and `Config.reinforceItemCount`. Every value is an
integer or a flag of the build; there are no fitted weights, stream offsets or
score coefficients. A card's `supply_percent` is its `appearConditionParameter`
when its condition is `SupplyPercent`.

The native call chain is `OnEnterDeployment` → `GenerateReinforceItems` →
`GenerateRoundReinforceItems` / `GenerateUnitReinforceItems`. Ordinary dealing
closes on `ReinforcePool.OnNewRound`, `CheckReinforeCondition`, `CheckAppear`,
`GenLvProbs`, `RoundRand`, `RandOnce` and `SelectReinforce`. Unit dealing closes
on `UnitFirstRand`, `UnitSecondRand`, their sort callbacks and the technology
cost methods. The skill cooldown cast is bound to `CommanderSkillBase` through
the binary metadata slot, rather than inferred from matching offers.

The 2259 rules were checked against the offer arrays and successive native
random states of the replay corpus in mechcore-replay, which
`scripts/verify-battles.py` replays through the document crate. Modified
pools, other modes, negative seeds and nonstandard capped unit types remain
outside the supported scope. A last recorded round has no following snapshot
to independently check its outgoing state.
