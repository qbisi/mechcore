# Travelling units

A formation deployed onto a flank travels there: it opens the next fight in
travel and arrives during it. Its units stand where their layout put them from
the first tick. They start on part of their life, heal once a second, and all of
a side's travelling units arrive together once the travel time is up. Until then
they can be struck and killed, but do nothing themselves.

## On the field, not in it

A travelling unit is one of its side's units from the start:
`FightTeam.activeMeches` holds it and `FightCoreSystem.TeamUpdate` counts it
alive, so a side whose other units are dead has not lost. A search finds it and a
shot kills it: `FightActor.IsValidTarget` asks only whether it is visible and
alive. What travel withholds is its own update, because `TeamUpdate` skips
`FightMech.Update` for a unit with `FightMech.IsSuperDeployment` set. Its skill
does not search, fire or cool, its motion does not move, and its buffs do not
run.

It has no RVO agent while it travels either: `MotionController.EnterFight`
skips `RVOControllerFixed.Active` for it, so other units walk through where it
stands. The presearch that gives every unit its first lock gives it one too.
The skill's attack target is searched only by the unit's own update, so the
recording names none until the first update after it arrives.

Its formation stands where a settled one does, turned to face the middle
([`unit-rules.md`](../spec/simulation/unit-rules.md)).

## Life and arrival

`SuperDeploymentController.EnterTravel` sets each unit's life to its maximum
times `superDeploymentLifeRate`, an FPoint product truncated to an integer:
0.4, stored a hair under it, so 4813 starts at 1925.
[`config/super_deployment.yaml`](../../config/super_deployment.yaml) holds the
rate and `superDeploymentTravelTime`, 8 seconds, as
[`extract-super-deployment.py`](../../scripts/extract/extract-super-deployment.py)
reads them from `Config`.

Each side has one `SuperDeploymentController`. `SuperDeploymentSystem` updates
after `InterceptSystem`, and each controller that still holds a travelling
unit runs `SuperDeploymentController.Update`, in this order:

1. It adds the logic delta to its travel time and its recovery time, both
   FPoint seconds starting at zero.
2. When the recovery time reaches a second, it takes a second off and heals
   every travelling unit by its maximum life times one less the rate, divided
   by the travel time.
3. When the travel time reaches `TravelTime()`, `FinishTranvel` brings every
   travelling unit in: in the list's order, it activates each unit's movement
   where it stands and takes it out of travel. Activating makes the unit's RVO
   agent, and the first tree built after reads a new agent's position as zero
   ([rvo.md](../spec/simulation/rvo.md#the-first-tree)), so in the first solve
   after the arrival no arriving unit is another's neighbour.

Both comparisons are `FPoint.op_GreaterThanOrEqual`, which counts a value within
43 raw below as equal. Twenty logic deltas are a second less 16 raw, so the
first two heals land on ticks 20 and 40, and the third on 61, once the
remainder has run out. The eighth heal and the arrival share tick 161.

`TravelTime()` is the travel time times one plus the side's
`FightTeam.superDeploymentTimeChangeRate`. Quick Teleport's `-0.5` sets that
rate through `FightTeam.SetSuperDeploymentTimeChangeRate` and halves the
travel. Quick Teleport is the one officer that sets it, and the pool deals it
once, so a side has at most one rate to set; the setter does not add, and a
layout cannot hold Quick Teleport twice
([layout.md](../spec/document/layout.md#officers)).

A unit that dies while travelling leaves the list through
`SuperDeploymentController.OnMechDead`. A side whose travelling units all
died stops counting. Nothing waits for travel to finish: a fight can end with
units still travelling.

## Corrections

Officers, technologies, equipment and Energy Tower skills write onto a
travelling unit as they write onto any other, when the fight is built:
`EffectProvider.AddEffect` calls each data source's `AddData` without asking
about travel. What travel holds back is `EffectProvider.ActiveCheck`, and for
these sources activation only announces the effect. So a travelling unit
starts on its share of its corrected life and heals by its corrected maximum.

**Whether a travelling unit's extra weapons are on does not change the fight.**
A unit placed on a flank has its effects switched off
(`BattleSystem.OnEnterSuperDeployment`, `FightEffectSystem.DeactiveEffect`),
the skills an extra weapon technology adds with them
(`ExtraSkillProvider.DisableSkill`), and all are switched on as it arrives
(`SuperDeploymentController.ExitTravel`, `FightEffectSystem.ActiveEffect`). So
the order a match placed the unit and researched its technologies decides
whether its extra weapons are on through the travel: a technology researched
after the unit's last deployment action stays on. A layout states neither
order, and the game fights every travelling unit of a layout with its extra
weapons off. It makes no difference: a travelling unit does not update, and
from its arrival it is the same unit either way. Blue's two Fire Badgers of
replay `2324_20260925--134259672`, bought onto the two flanks in round 3 with
Scorching Charge researched after both, the one a commander skill, an item and
an upgrade then passed off and the other on, fight that round as its layout
does from tick 161 on, every tick before differing only in that charge. A
recording therefore reads no skill of a travelling unit
([mcfr.md](../spec/mcfr/mcfr.md#skills)).

## Evidence

### Recorded

- A travelling unit stands where its layout put it and starts on 0.4 of its
  life. It heals on ticks 20, 40, 61 and every twenty ticks after, arrives on
  161, and names no attack target until it has updated:
  `tests/super_deployment/arrives.yaml`.
- Travelling units can be struck and killed, both sides travel at once, and
  the survivors arrive together:
  `tests/super_deployment/struck-while-travelling.yaml`.
- Officers, equipment, technologies and Energy Tower skills correct a
  travelling unit from the first tick:
  `tests/super_deployment/officers-and-equipment.yaml`,
  `tests/super_deployment/technologies.yaml`,
  `tests/super_deployment/energy-tower-skills.yaml`.
- Travelling units with extra weapons, on either flank of either side, fight
  from their arrival as units that did not travel: `tests/super_deployment/extra-weapons.yaml`.
- Whether a travelling unit's extra weapon is on changes nothing but that
  weapon's own state before its arrival: recorded from their replays, these
  two rounds differed from their layouts' fights on ticks 1 to 160 in that
  state alone, `tests/corpus/134259672-r3.yaml`,
  `tests/corpus/201370830-r7.yaml`.
- Quick Teleport halves the travel, with four heals of a quarter of 0.6:
  `tests/super_deployment/quick-teleport.yaml`.
- A squad that arrives together is solved once with none of its members among
  another's RVO neighbours, and walks off where a squad whose members saw each
  other would stand blocked: `tests/corpus/134270595-r2.yaml`.

### Read

- A travelling unit is counted alive and not updated:
  `FightCoreSystem.TeamUpdate`, `FightMech.IsSuperDeployment`.
- It has no RVO agent until it arrives: `MotionController.EnterFight`,
  `RVOControllerFixed.Active`.
- Its life and the controller's update: `SuperDeploymentController.EnterTravel`,
  `SuperDeploymentController.Update`, `SuperDeploymentController.TravelTime`,
  `SuperDeploymentController.FinishTranvel`.
- A unit that dies leaves the travel: `SuperDeploymentController.OnMechDead`.
- The order of the systems: `FightController.AddModules`.
- Corrections are written without regard to travel: `EffectProvider.AddEffect`,
  `EffectProvider.ActiveCheck`.
- Placing a unit on a flank switches its effects off and arriving switches
  them on: `BattleSystem.OnEnterSuperDeployment`,
  `FightEffectSystem.DeactiveEffect`, `SuperDeploymentController.ExitTravel`,
  `FightEffectSystem.ActiveEffect`, `ExtraSkillProvider.DisableSkill`.

### Not established


- **The order of two sides' travelling units in the list**, beyond the
  identity order the recordings agree with.
- **A travelling unit whose buffs, summons or generic effect providers
  activate on arrival**, which travel's `ActiveCheck` does gate. Each is read
  from the build, a buff's from `BuffCycleController.Active`
  ([equipment_effects.md](equipment_effects.md#buff-items)); no recording
  holds one.
