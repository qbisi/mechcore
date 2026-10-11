# Action legality

A decision a side takes while it deploys is a player action, and the game
checks it before it does anything: the action's performer answers a
`PlayerActionCheckResult`, and any answer but `OK` refuses the action and
leaves the position as it was. This document lists, for each of the
[actions](../spec/document/action.md) a match writes, what that check asks of
the position the action is taken from. An action that passes every condition
listed under it is one the game performs; one that fails any of them is one no
recording holds.

The conditions are listed in the order the check asks them. The order decides
only which reason a refusal reports, never whether the action is refused.

These rules cover a standard 1v1 match. What a condition reads from a table
(a price, a slot count, an allowance) is that table's: the documents beside
this one say what the entries mean, and this one does not repeat them.

## Two layers check an action

Every action passes `PAP_<Action>.Check` before it is performed. An action that
the performer carries out by issuing a match action passes that match action's
`MAP_<Action>.Check` too: `use_equipment`, `move_unit`, `upgrade_unit`,
`release_contraption` and `active_energy_tower_skill` do. An action is legal
when both layers answer `OK`.

`buy_unit` lands the formation where the board has room and then moves it to
the position the action names, so a purchase is checked as a purchase and its
position as a move.

## Per action

### `choose_advance_team`

- The side has not already chosen an opening (`RepeatAction`).
- The offer at `index` is one the side was dealt, and its team is among the
  side's candidates (`InvalidItem`); a side whose candidate has already been
  taken is refused (`RepeatAction`).
- An offer that carries a reinforcement item is refused once the side's
  reinforcement choice is finished (`RepeatAction`).

### `choose_reinforce_item`

- The item is one the side is offered (`UnknownItem`).
- Unless it is the decline, the side holds its price (`NotEnoughSupply`) and the
  item is in the offer of the side's current reinforcement step (`InvalidItem`).
- The side has a reinforcement step left to choose in: one choice per step, and
  a step chosen in is not chosen in again (`RepeatAction`).

### `buy_unit`

- The main region has room for a new formation of the unit's footprint
  (`InvalidPosition`).
- The shop has unlocked the unit (`InvalidUnit`).
- When the shop sets a same-unit limit, the side holds fewer formations of the
  unit than it (`UnitCountLimit`). Only a game rule's officer sets one, so a
  standard match has none.
- The side holds the unit's price (`NotEnoughSupply`).
- When the shop limits purchases, a purchase is left this round and the unit
  has a purchase left of its own (`NotEnoughCount`). Nothing in a standard
  match turns that limit on, so a purchase is held only to the round's
  allowance, which the side's own count says.
- The position it moves to passes `move_unit`'s conditions.

### `unlock_unit`

- The shop allows unlocking (`InvalidAction`), which only the survival mode
  turns off.
- The unit is one the shop still holds locked; a unit already unlocked is
  refused (`InvalidUnit`).
- The side holds the unlock price (`NotEnoughSupply`), and an unlock is left
  this round (`NotEnoughCount`).

### `upgrade_unit`

- The side holds the formation (`InvalidUnit`).
- The formation is below its last level (`LevelMax`), in both layers.
- The side holds the upgrade price (`NotEnoughSupply`).
- A formation that holds experience has a full bar (`NotEnoughExp`): the
  check compares the formation's experience with its next level's whenever
  the experience is not negative. A formation that joined this round, or
  levelled this round, buys its next level without one.

### `upgrade_technology`

- `tech` is a technology of `unit` in the side's loadout (`InvalidItem`,
  `UnknownItem`): the side's technologies are prepared from its loadout as the
  match starts, one set per unit, and a technology outside them is not one the
  side can research.
- It is not already researched (`InvalidAction`).
- The side holds its price (`NotEnoughSupply`).
- A technology with a predecessor needs that predecessor researched
  (`UnknownTarget`, `InvalidTarget`). No technology a standard match's loadouts
  hold has one.

The check does not ask whether the shop has unlocked the unit.

### `active_blueprint`

- The blueprint is one the side can activate (`InvalidItem`) and not already
  active (`RepeatAction`). The side can activate the first level of each
  blueprint the match's map offers, and the Research Center's research
  blueprints only where a game rule enables research. Activating a chain's
  first level puts its second in its place, so the second is refused before
  the first is active and the first after the second has replaced it.
- The match has reached its unlock round (`RoundLimit`).
- A blueprint that takes rounds to research needs a free research slot
  (`Researching`).
- The side holds its price, which rises by the match's per-blueprint increase
  for every blueprint already active (`NotEnoughSupply`). A standard match's
  increase is zero.

### `active_energy_tower_skill`

- The skill is one of the side's (`InvalidItem`) and not already active this
  round (`RepeatAction`).
- The side holds the skill's full price, before anything it grants
  (`NotEnoughSupply`).
- The round's purchase allowance plus the skill's change to it is not negative
  (`NotEnoughCount`).

### `strengthen_tower`

- The tower is one of the side's and its strengthen option is open
  (`InvalidTarget`). Only the tutorial and a restored snapshot change the
  option, so in a standard match it is open for both towers.
- A next level exists (`LevelMax`).
- The side holds that level's price (`NotEnoughSupply`).

### `use_equipment`

- The item is in the side's stock (`InvalidItem`) and not worn
  (`EquipmentLimit`).
- The side holds the formation (`InvalidUnit`, `UnknownTarget`).
- The formation's card wears equipment and has a free slot (`TargetLimit`);
  [equipment.md](equipment.md) says which cards wear none and how many slots a
  formation has.
- The item may target the formation (`InvalidTarget`).

### `move_unit`

- The side holds the formation (`InvalidUnit`).
- The position is in one of the side's regions (`InvalidPosition`), and that
  region is open this round (`RoundLimit`): a flank opens in the second.
- The formation may move this round (`IsMoved`); [mobility.md](mobility.md) says
  which may.
- The footprint fits the region and overlaps nothing standing there
  (`RegionLimit`), and two formations moved together do not
  overlap each other (`InvalidAction`).

### `release_commander_skill`

- A release aimed at a unit or a construction is refused while the skill is
  active (`RepeatAction`) or cooling (`Cooling`), and for a target the side does
  not hold (`UnknownTarget`) or the skill does not take (`InvalidTarget`).
  [commander_skills.md](commander_skills.md) says what cools a skill; a release
  aimed at an area is checked for where it falls, not for the slot, and
  [battle_skill.md](battle_skill.md) says where each may fall.

### `release_contraption`

- The contraption is one the side holds (`InvalidItem`): every contraption the
  build ships, which the side is given as the match starts.
- The side holds its price (`NotEnoughSupply`) and a release is left this round
  (`NotEnoughCount`).
- The position is inside the contraption's available region
  (`InvalidPosition`): the main region, an Interceptor's and a mine's as it
  is and every other contraption's drawn in on each side by a margin the
  contraption gives, and an interceptor's footprint fits there as a move's
  would.

### `concede`

Nothing is checked.

## Finishing a round

A side ends its deployment with a finish, which the game checks for nothing:
it is taken whether or not the side answered its reinforcement offer, and the
round is fought without the answer.

## Evidence

### Replayed

- A formation that has fought is upgraded only with a full bar, Intensive
  Training's among them, and one that joined or levelled in the round is
  upgraded without one: every match of this version's corpus,
  `scripts/corpus/verify-matches.py`.

### Read

- An action is checked before it is performed, and the reasons a check answers
  are one enumeration: `PlayerActionCheckResult`, `PAP_BuyUnit.Check`.
- The second layer is the match action's own check: `MAP_UseEquipment.Check`,
  `MAP_MoveUnit.Check`, `MAP_UpgradeUnit.Check`, `MAP_ReleaseContraption.Check`,
  `MAP_ActiveEnergyTowerSkill.Check`.
- The opening: `PAP_ChooseAdvanceTeam.Check`,
  `BattleOpeningController.IsSelected`, `BattleOpeningController.GetOpeningData`,
  `PlayerController.CanSelectAdvanceTeam`,
  `AdvanceTeamSystem.GetAdvanceteamTeam`, `ReinforcementManager.IsChooseFinished`.
- A reinforcement choice: `PAP_ChooseReinforceItem.Check`,
  `ReinforcementManager.CanChooseReinforceItem`, `ReinforcementManager.GetItem`,
  `ReinforcementManager.roundReinforceItems`,
  `ReinforcementManager.reinforceProgress`,
  `ReinforcementManager.choosedReinforceItems`, `AddSupplyReinforceItem`.
- A purchase: `ShopManager.CanBuyUnit`, `TerritoryManager.CanAddUnit`,
  `TerritoryManager.GetAvailiblePositionForNewActor`,
  `ShopDataChangeInt.SameUnitCount`, `UnitManager.GetUnitCount`,
  `ShopManager.hasBuyCountLimit`, `Shop.buyCount`, `Shop.unitCounts`.
- An unlock: `ShopManager.CanUnlockUnit`, `ShopManager.canUnlockUnit`,
  `Shop.GetLockedUnit`, `Shop.UnlockCount`.
- An upgrade: `PAP_UpgradeUnit.Check`, `UnitManager.CanUpgradeUnit`,
  `MechTeam.IsMaxLevel`, `CardElement.GetUpgradeSupply`, `MechTeam.expFloat`,
  `CardElement.GetNextLevelExp`, `CardElement.GetLevel`; `PAP_UpgradeUnit.Check`
  waives `NotEnoughExp` only under `ExternalConfig.il2cppTest`.
- A technology: `PAP_UpgradeTechnology.Check`, `TechnologyManager.GetTechnology`,
  `TechnologyManager.IsOwner`, `TechnologyManager.technologyManagers`,
  `TechnologyManager.PrepareTechnology`, `UnitTechnologyManager.GetUpgradeCost`,
  `TechnologyData.previousTechID`; the technologies that have a predecessor are
  `TechnologyGroupData` rows outside the ones `config/unit_techs.yaml` holds.
- A blueprint: `BlueprintManager.CanActive`, `BlueprintData.unlockRound`,
  `BlueprintData.researchTime`, `BlueprintManager.researchSlotCount`,
  `BlueprintManager.GetActivedCount`, `BattleInfo.BlueprintIncreaseSupply`,
  which a replay's header states and the standard replays state as zero.
- An energy tower skill: `EnergyTowerManager.CanActiveSkill`,
  `ActivableItem.isActive`, `EnergyTowerSkillData.supply`,
  `EnergyTowerSkillData.shopBuyCountChangeValue`, `ShopManager.GetMaxBuyCount`.
- A tower: `PAP_StrengthenTower.Check`, `BuildingManager.CanStrengthenTower`,
  `BuildingManager.hasStrengthenOption`, `BuildingManager.GetTowerStrengthenData`,
  `MatchUtility.GetMinLevelTowerStrengthenData`.
- What the shop limits: `SystemOfficerController.ChangeSameUnitCount` writes
  `ShopDataChangeInt.SameUnitCount` from `OfficerData.sameUnitCount`, which
  only a game rule's officer sets; nothing calls `ShopManager.SetBuyCountPerUnit`
  or `ShopManager.AddBuyCountLimit`; only `SurviveGameplayController.OnMatchStart`
  calls `ShopManager.DisableUnlockUnit`.
- What a side can activate or hold: `BlueprintManager.PrepareBlueprint` adds
  `MatchSetting.blueprints` that `BlueprintData.IsFirstLevel`, the research
  ones only under `GameRuleManager.IsEnableBlueprintResearch`;
  `BlueprintManager.Active` calls `BlueprintManager.ReplaceBlueprint`;
  `ContraptionManager.Init` creates every row of `Config.GetContraptionDatas`;
  only `GuiderGameController.SetGuiderConfig` and
  `PlayerSnapshotController.ApplyTowerSnapshot` call
  `BuildingManager.ChangeHasStrengthenOption`.
- A finish: `PAP_FinishDeploy.Check`.
- Equipment: `MAP_UseEquipment.Check`, `EquipmentManager.CanUseEquipment`,
  `Equipment.owner`, `CardElement.CanAddEquipment`, `CardData.canAddEquipment`,
  `UnitDataChangeInt.EquipmentSlotCount`, `UnitManager.HasUnit`,
  `UnitUtility.IsEffectTarget`.
- A move: `TerritoryManager.CanMoveUnits`, `PlayerTerritory.GetRegionForPosition`,
  `MapRegion.activeRound`, `CardElement.CanMovable`,
  `TerritoryManager.CanMoveUnitToPosition`, `MapRect.Overlaps`.
- A commander skill: `PAP_ReleaseCommanderSkill.Check`,
  `CommanderSkillManager.CanReleaseCommanderSkill`,
  `CommanderSkillBase.IsActive`, `CommanderSkillBase.coolingRound`,
  `IUnitEffectCommanderSkill.CheckAvaliable`.
- A contraption: `ContraptionManager.CanRelease`,
  `ContraptionManager.IsReadyToRelease`, `ContraptionManager.contraptions`,
  `ContraptionManager.RemainCount`, `ContraptionManager.GetAvailableRegion`,
  `LandMineContraption`,
  `InterceptContraption`.

### Not established

- **What leaves a formation's experience negative.** That a formation that
  joined or levelled this round upgrades without a full bar is what the
  replays show; which writes of `MechTeam.expFloat` make it so is not read.
- **What an unanswered offer leaves.** A round fought without a side's answer
  is fought; what the next round deals that side, and whether a deadline
  answers for it, is not read.
- **Whether a check runs outside deployment.** What refuses an action taken in
  another phase is not read here.
