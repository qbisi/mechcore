# Reinforcement dealing

These rules cover standard versus 1v1 with no game rules, after the
[opening initialization](opening.md). The seed determines a stream and initial
pools. Later deals also depend on both players' fielded units, unlocked units,
active technologies and officers, and previous card choices. Predicting a deal
does not predict those decisions or combat outcomes.

## Stream and pool lifetime

`ReinforcementSystem.OnEnterDeployment` dispatches to
`GenerateReinforceItems`, which hands the round to the match's reinforcement
object, `ReinforcementRandomObject_Common.Generate`. Round 1 deals nothing. Subsequent rounds use the
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
- **2, SupplyPercent:** see the next section.

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

## Investment share

The unit-modification officers carry `appearCondition` 2, `SupplyPercent`,
with an `appearConditionParameter` pair `[p0, p1]`.
`ReinforcementRandomObject_Common.Generate` builds one `PlayerUnitEconomy` per
real player of the match (`Match.playerManager.realPlayerControllers`, both
sides) with `CollectPlayerUnitEconomies`, and hands the list to
`ReinforcePool.OnNewRound`, which passes it to `CheckReinforeCondition`.

A player's economy values each unit type it fields, reading the formations
the round opened with before its officers deliver, the same ones the unit
passes below read:

- each of its formations of the type adds
  `UnitUtility.CalculateUpgradeLevelCost`, the card's `GetMoney` at the first
  level plus `GetLevelUpSupplyCost` of every level the formation has risen
  through. A level-`n` formation is `CardLevel` `n − 1`, and every standard
  unit's `mechExpDatas` row charges one price for all eight levels, so the
  formation is worth its purchase price plus `n − 1` level prices;
- the type adds its `CardData.unlockPrice` once;
- the type adds `UnitTechnologyManager.CalculateUpgradeCost`, its active
  technologies;

and `TotalValue` is the sum over its types.

`CheckReinforeCondition` sums the values of the officer's `unitID` types in
each economy. An economy with `TotalValue > 0` requires, when `p0 ≤ p1`,
`TotalValue × p0 ≤ 100 × sum ≤ TotalValue × p1`; one with nothing invested
passes only when `p0 ≤ 0`. One economy outside fails the card. So an officer
that modifies a unit is dealt only while neither player has more than `p1`
percent of its investment in that unit: every standard card states `[0, 15]`
over a single unit.

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

Which player a pass reads as its own is its team's index, with a branch for an
asynchronous match that standard play does not take.

The technology calculation follows `UnitUtility.CalculateUpgradeTechnologyCost`:
base upgrade price plus the number of earlier active technologies times the
unit's positive `techUpgradeIncreaseSupplyPerCount`, or the global
`Config.upgradeTechnologyCostIncreaseDelta` otherwise. A positive
`techUpgradeMaxSupplyLimit` caps each term. This is a base investment measure;
paid prices and officer discounts are not used. Standard unit rows set neither
override nor cap, so technology ordering does not affect the sum. Each term
also adds the technology's own `PlayerDataChangeInt.Supply`, which only
`TDOfficerEffect_DefaultMechTechPrice` writes; no standard officer carries it,
so the term is zero.

Candidates are ordered by score, then card ID; missing unit scores are zero.
Each remaining slot draws uniformly among the minimum-score candidates and
removes every variant of that chosen unit. A singleton still consumes a draw.
Both passes use the same reinforcement stream.

After both passes, a round at or beyond the selected schedule's `supplyRound`
replaces the last offer with `supplyReinforceID`, indexed by
`round - supplyRound` and clamped to the final entry. Random draws for the
replaced card have already happened.

## Declining

Every round offers a decline beside its deal, and the decline pays supply. An
ordinary round pays the map's `MatchSetting.giveUpSupply`; the getter is
inlined wherever it is read, so the match is by name and value, not by call
chain. A unit round
pays the selected schedule's `giveUpSupply` at the round's place in
`roundGroup`: `ReinforcementSystem.m_RoundGiveUpSupply` is built from
`UnitReinforceRoundPool.GetGiveUpSupply(round)`, which reads exactly that.

## Inputs

[config/reinforcements.yaml](../../config/reinforcements.yaml) is written by
`scripts/extract_reinforcements.py` from the build's typed export: the officer,
unit card, round pool, probability and card rows of `ConfigDataContainer`, the
commander skill and equipment cards of `CommanderSkillGroupData` and
`EquipmentGroupData`, and `Config.reinforceItemCount`. Every value is an
integer or a flag of the build; there are no fitted weights, stream offsets or
score coefficients. A card's `supply_share` is its `appearConditionParameter`
and its `unitID` when its condition is `SupplyPercent`, and a unit's `upgrade`
is its one level price, which the extraction refuses to write when a row's
levels differ.

## Evidence

### Replayed

- Every ordinary and unit round of this version's corpus is dealt as recorded,
  offer by offer and in order, from the stream the opening leaves:
  `scripts/verify-battles.py`.
- Every officer with the investment share is dealt, or replaced from its group,
  as the corpus recorded: `scripts/verify-battles.py`.
- A decline in an ordinary round and in each of the first three unit rounds
  pays what the next round's supply records: `scripts/verify-battles.py`.

### Read

- A round's deal is generated as the round's deployment opens, by the match's
  reinforcement object: `ReinforcementSystem.OnEnterDeployment`,
  `ReinforcementSystem.GenerateReinforceItems`,
  `ReinforcementRandomObject_Common.Generate`.
- A round the unit round pool lists is a unit round and skips ordinary dealing:
  `UnitReinforceRoundPool.RoundGroup`,
  `ReinforcementRandomObject_Common.GenerateUnitReinforceItems`.
- An officer that fails its condition is replaced from its group, chosen
  uniformly, after the traversal: `ReinforcePool.OnNewRound`,
  `ReinforcePool.CheckReinforeCondition`.
- Choosing removes a nonrepeatable card and clears its group:
  `ReinforcePool.SelectReinforce`.
- The level weights are the last row reached, zeroed for an empty level:
  `ReinforcementRandomObject_Common.GetCurrentReinforceItemProbabilityData`,
  `ReinforcePool.GenLvProbs`.
- A deal draws a level, then a card within that level's window, swapping it to
  the take position: `ReinforcePool.RoundRand`, `ReinforcePool.RandOnce`,
  `Config.reinforceItemCount`.
- A level-4 commander skill offered excludes level-4 skills on the next round:
  `ReinforcePool.RoundRand`, `ReinforcePool.CheckAppear`.
- The investment share is each real player's economy, tested against the
  officer's parameter pair: `ReinforcementRandomObject_Common.CollectPlayerUnitEconomies`,
  `PlayerUnitEconomy.TotalValue`, `UnitUtility.CalculateUpgradeLevelCost`,
  `OfficerData.appearConditionParameter`.
- The first unit pass decrements its counter once for a draw and once for each
  entry of the drawn unit it removes, the drawn one included:
  `ReinforcementRandomObject_Common.UnitFirstRand`.
- The second unit pass draws among the least-invested types:
  `ReinforcementRandomObject_Common.UnitSecondRand`,
  `UnitTechnologyManager.CalculateUpgradeCost`,
  `UnitUtility.CalculateUpgradeTechnologyCost`.
- A round past the schedule's `supplyRound` replaces the last offer:
  `UnitReinforceRoundPool.supplyRound`,
  `UnitReinforceRoundPool.supplyReinforceID`.
- A decline pays the map's figure, or in a unit round the schedule's:
  `MatchSetting.giveUpSupply`, `UnitReinforceRoundPool.GetGiveUpSupply`,
  `ReinforcementSystem.m_RoundGiveUpSupply`.

### Not established

- **A last recorded round's outgoing stream.** It has no following snapshot to
  check it against.
- **Which formations the investment walks.** The enumeration goes through an
  interface the dump does not name; the formations the round opened with are
  what the corpus agrees with, and no recorded round separates them from sold
  or lost ones.
- **A share bounded from above its floor.** The build reads `p0 > p1` as a band
  the share must stay outside; no standard card has one, and the prediction
  refuses it.
- **The supply substitution.** No replay reaches its round.
- **The last unit round's decline.** No replay reaches it, and it is where the
  schedules' figures differ.
- **Modified pools, other modes, negative seeds and nonstandard capped unit
  types.** Outside the supported scope.
