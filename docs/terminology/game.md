# Game terms

The game's concepts as a match document and the rules documents name them.
`game` marks Chinese the game's own text uses; `ours` marks a term the game
does not show in Chinese, chosen here. **Build** names where the decompiled
build keeps the concept, a class, a configuration table or a member, as the
rules documents cite them; it says where to look, not what the code does.

## Match

| English | 中文 | Source | Build | Meaning |
| --- | --- | --- | --- | --- |
| match | 对局 | ours | `MatchSetting` | one game between two sides, from the opening to the end, played over rounds |
| round | 回合 | game | `PlayerRoundRecord` | one turn of a match: both sides buy and place, then a fight is fought |
| this round | 本回合 | game | | an effect that lasts until the round it was bought in ends |
| side | 阵营 | ours | | one of the two players, `blue` or `red` |
| opening | 开局 | ours | `advanceTeamDatas` | what a side holds before round 1: its advance team or specialist |
| advance team | 先遣队 | ours | `AdvanceTeamData` | an opening that hands a side units |
| specialist | 专家 | game | `OfficerData` | an opening, or a reinforcement card, that hands a side an officer |
| map | 地图 | ours | `MatchSetting` | the board a match is fought on, named by its ID |
| territory | 领地 | ours | `PlayerTerritory`, `TerritoryManager` | a region of a map a side may place in, opening in a given round |
| crystal | 水晶 | ours | `FightCrystal` | a neutral object a map places on the board |
| reactor core | 反应堆核心 | ours | `MatchSetting.GetReactorCore` | a side's life; a round's loser loses some, and a side at none has lost |

## Economy

| English | 中文 | Source | Build | Meaning |
| --- | --- | --- | --- | --- |
| supply | 补给 | game | `supply` fields | the currency a side spends in the shop |
| shop | 商店 | game | `cardDatas` | where a side buys units and cards each round |
| unlock | 解锁 | game | `CardData.unlockPrice` | buying the right to buy a unit |
| upgrade | 升级 | game | `CardElement.GetUpgradeSupply` | raising a formation's level |
| level | 等级 | game | `CardLevel` | a formation's rank, which multiplies its life and damage |
| experience | 经验 | game | `ExpSystem`, `mechExpDatas` | what a formation earns in a fight towards its next level |
| reinforcement card | 增援卡 | ours | `ReinforcePool`, `unitReinforceDatas` | a card the shop offers between rounds: units, an officer, equipment or a commander skill |
| blueprint | 蓝图 | game | `BlueprintData` | a purchase that puts a commander skill or an officer within reach for the match |
| sell | 出售 | ours | `CardData.canBeSold` | giving a formation back for part of its price |

## Forces

| English | 中文 | Source | Build | Meaning |
| --- | --- | --- | --- | --- |
| unit | 单位 | game | `MechData`, `CardData` | a kind of mech, and the type a formation is made of |
| formation | 编队 | ours | `CardElement`, `MechTeam` | the mechs bought as one and placed as one, which level and research together |
| squad | 小队 | game | `UnitReinforceData.unitID` | one formation's worth of a unit, as a card or an officer hands it out |
| mech | 机甲 | ours | `FightMech` | one member of a formation on the board; `CardData.mechCount` says how many a formation has |
| technology | 科技 | game | `TechnologyData`, `Technology` | an upgrade a formation researches, which changes its unit for the match |
| officer | 军官 | ours | `OfficerData`, `Officer` | a side-wide bonus a card or a specialist grants |
| equipment | 装备 | game | `EquipmentData`, `Equipment` | an item a formation wears; [rules/equipment.md](../rules/equipment.md) says how many |
| commander skill | 战场技能 | game | `CommanderSkillGroupData`, `CommanderSkillBase` | a skill a side releases on the board; the game's English calls it a Battlefield Power |
| energy tower | 能量塔 | game | `FightTower`, `EnergyTower` | a building whose skill a side may use, and whose loss weakens its side |
| energy tower skill | 能量塔技能 | ours | `EnergyTowerSkillData` | the skill a side picks for its energy tower |
| contraption | 装置 | ours | `ContraptionData`, `ContraptionManager` | a device a side places: a shield, a missile or an interceptor |
| construction | 建筑 | ours | `ConstructionData`, `FightConstruction` | a building a side places, such as a wall or a turret |

## Fight

| English | 中文 | Source | Build | Meaning |
| --- | --- | --- | --- | --- |
| fight | 战斗 | ours | `FightingState`, `FightCoreSystem` | the part of a round where the board plays itself out |
| tick | 帧 | ours | | one step of a fight's simulation |
| deploy | 部署 | game | | placing a formation on the board before a fight |
| damage | 伤害 | game | `DamageProperty` | what a hit takes off a target's life |
| shield | 护盾 | game | `AdvancedEnergyShieldSystem` | a layer that takes damage before life does |
| range | 射程 | game | `FightSkill.GetAttackRange` | how far a unit attacks |
| buff | 增益 | ours | `BuffData`, `Buff` | an effect a source puts on a unit for a while, whether it helps or harms |

## Easily confused

- **Skill.** A commander skill is a side's, released on the board. A unit's
  skill (`FightSkill`) is its weapon. An energy tower skill is the tower's,
  picked once. A construction's skill is a turret's weapon. A document's
  `battle_skills` field holds commander skills.
- **Unit, formation, squad, mech.** A unit is a type, such as Marksman. A
  formation is one purchase of it on the board. A squad is the game's word for
  what a card hands out, one formation's worth. A mech is one member of a
  formation. The build's `Mech*` names mean a type in `MechData` and a member
  in `FightMech`.
- **Level and rank.** The same thing: the game's English descriptions say
  "Rank 3", the documents say level.
- **Round, fight and tick.** A round holds one fight, and a fight runs in
  ticks. A turn is not a game term; a match's `.turn` file holds the round in
  progress.
- **Card.** A reinforcement card is what the shop offers between rounds. The
  build's `cardDatas` is the unit table, one row per unit, not those cards.
- **Specialist and officer.** A specialist is how an officer arrives, at the
  opening or on a card; what it grants is an officer.

## Deprecated spellings

A name below is written in its official form, not the one beside it.

| Write | Not | Why |
| --- | --- | --- |
| Marksman | Longbow | the build's animation clips say `Longbow_*`, a name the game no longer shows |
| Redeployment | Redeploy | the game names commander skill `1000001` Redeployment |
| Anti-Armor Cannon, Rapid-Fire Cannon | Anti-Armor Turret, Rapid-Fire Turret | the game's names; a sprite keeps its snake-case id |
| commander skill | battle skill | one term in prose; `battle_skills` stays the document field's name |
