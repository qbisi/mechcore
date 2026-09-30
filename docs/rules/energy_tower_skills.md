# Energy Tower skills

Two of the Energy Tower's skills change a fight. Enhanced Range adds 15 m to
the attack range of every ranged unit of the side that activated it, and High
Mobility adds 3 to the movement speed of every unit of that side. Both correct
a unit through the same writers as an officer, in the same channels, and add
to an officer's values there. Neither reaches a construction or a tower.

## Which skills reach a fight

`EnergyTowerManager.ActiveSkill` activates a skill during deployment, and
`EnergyTowerManager.RecordSkillEffect` makes it a fight effect only when the
skill's `IsCommonEffect` holds. That check reads two fields and no others:
`EnergyTowerSkillData.speedChangeValue` and
`EnergyTowerSkillData.attackRangeChangeValue`. Of a standard match's five
skills only Enhanced Range (`5`) and High Mobility (`6`) set one, which is why
a [layout](../spec/document/layout.md#energy_tower_skills) carries those two
alone. The other three buy supply or change a round's shopping, and
[`economy.yaml`](../../config/economy.yaml) holds what they do to a ledger.

[`config/energy_tower_skill_effects.yaml`](../../config/energy_tower_skill_effects.yaml)
holds the two, as
[`extract-energy-tower-skill-effects.py`](../../scripts/extract/extract-energy-tower-skill-effects.py)
reads them from the build's config container:

| Skill | Target | Field | Value |
| --- | --- | --- | --- |
| Enhanced Range (`5`) | `Ranged` | `attack_range_value` | +15 m |
| High Mobility (`6`) | `All` | `speed_value` | +3 |

## Where a skill writes

For every unit card of the match's pool that the skill's target type admits,
`RecordSkillEffect` calls `BattleSystem.RecordFightEffect` with the side and
the card's mech. That call registers the skill with
`FightEffectSystem.AddProviderDataSource`, which is where a researched
technology is registered too. When the fight builds a unit, the skill writes
through `EnergyTowerSkill.AddData`: `MechDataModifer.TryAddCommonData` for the
unit's numbers and `SkillDataModifier.AddData` for its skills'. These are the
writers of [officer corrections](officer_effects.md), and each field lands
where the officer field of the same name does:

- the speed is a plain integer in the unit's `DataSet`;
- the range is a value in the skill's `DataSet`.

A value joins the description before any rate multiplies it. The skill
selects every skill of a unit, not only its main one, so each of a Wraith's
four slots reads the extra 15 m.

The target type is answered by `UnitUtility.IsEffectTarget`, as an officer's
row is ([which units a correction reaches](officer_effects.md#which-units-a-correction-reaches)).
A Rhino, whose main skill is a melee attack, gets the speed and not the range.

A summon is written on as a unit of its type is: `FightEffectSystem.AddEffect`
gives it what the side registered for its mech
([a summon](battle_skill.md#a-summon)). A travelling unit reads the skills
from the first tick.

A construction's effects are looked up in `TeamFightEffectManager`'s
construction managers, and these skills never register there. So a turret's
range is its own under Enhanced Range, and a tower has no range to change.

The skills last one round. `EnergyTowerSystem.OnEnterDeploymentAfter` calls
`EnergyTowerManager.Refresh`, which takes them off as the next deployment
opens. A skill activated again is written again.

## What is refused

A skill whose targeting is not a single type that
[`targets.rs`](../../crates/simulation/src/modifier/targets.rs) resolves is
refused by name, as an officer's is.

## Evidence

### Recorded

- Enhanced Range writes +15 m into a ranged unit's skill channel and nothing
  into a melee unit's. High Mobility writes +3 into every unit's own channel:
  `tests/energy_tower/fights/both-skills.yaml`.
- Enhanced Range reaches every skill slot of a unit:
  `tests/energy_tower/fights/wraith-slots.yaml`.
- Enhanced Range leaves a turret's range alone:
  `tests/energy_tower/fights/turret-untouched.yaml`.
- High Mobility reaches a summon:
  `tests/battle_skill/fights/summon-energy-tower.yaml`.
- Both skills reach a travelling unit from the first tick:
  `tests/super_deployment/fights/energy-tower-skills.yaml`.

### Read

- A skill reaches a fight through the provider registration a technology
  uses: `EnergyTowerManager.RecordSkillEffect`,
  `BattleSystem.RecordFightEffect`, `FightEffectSystem.AddProviderDataSource`.
- It writes through an officer's writers: `EnergyTowerSkill.AddData`,
  `MechDataModifer.TryAddCommonData`, `SkillDataModifier.AddData`.
- Only the speed and range fields make a skill a fight effect:
  `EnergyTowerSkillData.speedChangeValue`,
  `EnergyTowerSkillData.attackRangeChangeValue`.
- The target type is answered as an officer's is: `UnitUtility.IsEffectTarget`.
- A skill lasts one round: `EnergyTowerSystem.OnEnterDeploymentAfter`,
  `EnergyTowerManager.Refresh`.

### Not established

- **The two skills beside an officer that corrects the same number.** They
  add as two values by the writers, which no recording has checked.
