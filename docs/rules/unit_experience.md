# Unit experience

How much experience fills each level of a unit, the formula those amounts
follow, what a full bar means, and what Intensive Training does with it.

A unit's `exp` is its experience within its current level. Paying to
upgrade a unit starts the new level at zero, whatever the old level held;
[`action.md`](../spec/document/action.md#upgrade_unit) states that rule. A
document writes `exp` as `current/maximum`, such as `124/450`, whose `maximum`
is the full bar of the unit's level from 1 through 9.

The machine-readable table is
[`config/unit_experience.yaml`](../../config/unit_experience.yaml).
`scripts/extract/extract_prices.py` copies it verbatim out of the build's
`mechExpDatas`, whose `upgradeLv2` through `upgradeLv9` are its eight entries
per unit, and checks the formula below on every row it writes.

## Where a bar is full

A unit's bar is full at `GetUpgradeExp(level + 1)`, unless something has
replaced its bar (below), and level 9 fills at
the same amount as level 8. Both halves are read from the build's code:

- `MechExpData.PreProcess` builds the list `GetUpgradeExp` reads as
  `[0, upgradeLv2, …, upgradeLv9]`, indexed by `CardLevel`, whose `Level1` is
  0. The entry at a level is the experience it takes to arrive there.
- Every caller that sets or checks a unit's bar asks for the level above
  its own: `UnitSystem.ChangeLevel` passes the new level plus one to
  `UnitUtility.GetUpgradeExp`, and `CardElement` does the same through
  `CardData.GetUpgradeExp`.
- `MechExpData.GetData` clamps the index to the list's last entry, and nothing
  between it and those callers checks the level. So a level 9 unit, asking
  for a tenth level that does not exist, reads `upgradeLv9`.

The table's n-th entry is therefore the full bar of a unit at level n, and
level 9 repeats level 8's.

## What a full bar means

A full bar is a state the game keeps, not only a number it compares:
`MechTeam` carries `hasEnterMaxExp` beside its experience, and `IsExpMax`
answers from it. Three kinds of reader consult it:

- `ExpSystem.IsValidOwner`, which decides which units share the
  experience a fight hands out, so a full unit takes no further share;
- `CS_AddExp.CheckAvaliable`, where Intensive Training decides whether a
  unit is a target it can take, which is why it refuses a full one;
- the upgrade icon and the AI's upgrade action, consistent with a full bar
  being one of the conditions a level-up rests on. The other conditions, and
  when a full bar becomes a level, are the fight's and round's, and not
  established here.

## The formula

Every row but Vulcan's is one number, the first level's experience, times a
factor that depends only on the level, rounded half up:

```text
upgrade_exp(unit, n) = round_half_up(base(unit) × F(n)),  base(unit) = upgrade_exp(unit, 1)
```

| Level n | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| F(n) | 1 | 2.25321 | 3.5618 | 4.49028 | 5.21046 | 5.79888 | 6.29639 | 6.72735 |

The factors are fitted, not read: no shipped field carries them. Each is a
value that every standard row admits once rounded half up, and the admissible
interval is narrow enough that the five-digit figure above is not a choice
between materially different answers. Floor and ceiling rounding admit no
common factor at all, so the rounding is half up.

Vulcan is the one exception: its levels 2 through 8 follow the formula from a
base 1.1 times its own first level. The table is what a reader should use; the
formula is a statement about how the table was built, and it does not describe
Vulcan's first entry.

## Intensive Training fills the bar

Commander skill `1100001`, 强化训练 (Intensive Training), sets the unit it
targets to a full bar: `current` becomes the `maximum` of its level, whatever it
held before. It changes the level of nothing, costs nothing to
release, and does its work during deployment, so it is not a release the fight
sees; [`action.md`](../spec/document/action.md#release_commander_skill) states
the transition.

It cannot target a unit at level 9, or one whose bar is already full; the
second is the `IsExpMax` check its availability test makes. A level 9 unit can
still hold a full bar; it cannot be trained into one.

A full bar is also what a fight can leave behind. A unit that no training
touched can open a round holding exactly its level's bar, so a full bar is a
state a position holds rather than a trace of the skill. When a full bar turns
into a level, and what a fight does with experience past it, belong to the
fight and are not established here.

## What a kill hands out

A fight hands experience out as it goes, one kill at a time, and every
formation keeps its own. The table's `loot_exp` is what one unit hands out when
it is killed at its level; a tower hands out 100, and a construction its row's
`exp`.

A formation starts a round's fight at -1.0, which is not a debt: its first gain
starts from zero. A kill hands out its amount twice over:

- **The killer's formation takes the whole amount**, when the killer is a unit
  that may take experience: alive, not summoned, not controlled by the other
  side, and not already at a full bar. A kill no unit made, by a turret or a
  missile say, adds the amount to the pool below instead.
- **A pool of the same amount is split evenly.** It goes to every formation
  that hit the target during the fight and may take experience, and to every
  formation of the killer's side with a unit standing within 65 metres of the
  target, edge to centre. Each formation takes one share, however many of its
  units qualify, and the killer's formation takes a share as well as the whole.
  A kill no unit made with no one to share it goes to every formation of the
  killer's side that may take experience. A summon takes no share and is not
  counted among those that do.
- **A hit that deals nothing hit no one.** A skill's hit goes on to take life
  and to be counted only if it deals at least 1: an Incendiary Bomb's shell,
  which deals nothing and leaves only its fire, does not make its Hound's
  formation one that hit the target, and a unit the fire then kills with no
  formation near counts for every formation of the Hound's side.
  A beam's blow is no different: a Steel Ball's first blow, its ramp's
  multiplier truncated to nothing, deals nothing, and a Steel Ball that turns
  to another target after it is not one that hit the first.
- **A kill with no owner at all counts for the dead one's enemies.** An air
  drop's hit has no owner, and the side it counts for is the one the dead unit
  fought, whichever side dropped it: a Vulcan landing among its own side's
  Crawlers hands their experience to the other side.
- **A kill by the dead one's own side shares with its enemies.** Where the hit
  is of the target's own side, the pool's nearby formations are the side's
  opponents standing within 65 metres of the target, beside every formation
  that hit it: a Fire Badger its ally's explosion fells hands its experience to
  the enemies about it. The killer, an exploded unit, is dead and takes nothing.
- **A unit's own blow taking its life hands out nothing.** The hit of
  `SuicideEffect` is no side's, and no one takes the Fire Badger's experience
  as it takes its own life.

"Within 65 metres" is asked of the side's unit quadtree first, for a square 65
metres wide around the target. The tree answers with whole nodes, so a unit is
found or missed depending on how the side's tree has split: once a side has
twenty units, one standing well within 65 metres can be missed. A unit found is
then held to the circle, and a unit found need not be alive, only still in the
tree.

## An officer's rate

An officer's `exp_rate`, Smart Marksman's `+0.75` say, multiplies every gain
of the formations of the units it reaches: what a kill hands its killer and
what it shares out alike. A Marksman whose first kill hands it 8 takes 14 with
Smart Marksman held, and a share of 4 is 7. A formation it does not reach gains
what it would without it.

The rate is the formation's, not a unit's. The officer writes it onto the
unit's card, where enhancements sum and impairments compound as on a unit,
and the card hands both halves to its formation. Each gain is then the amount
times `(1 + enhancement) × remaining`, and the bar still caps it; the bar
itself is the table's whatever the rate.

## A technology's rate

A technology's `exp_rate`, Machine Learning's `+1` on the Vortex, lands on the
unit instead, as its own `MechDataChangeFloatRate.ExpChangeRate`: a technology
is not one of the sources that write a card. Each gain takes it beside the
card's, as `(1 + card enhancement + unit enhancement) × card remaining × unit
remaining`. The unit is the killer for what a kill hands it, and the
formation's first unit for a share, which is handed no unit; a formation's
units are one type with one side's technologies, so each answers the same
rate. A unit whose technologies are switched off answers none. Machine
Learning doubles the Vortex formation's gains and leaves every other
formation's as they are.

No gain carries a formation past its bar, and one that brings it within 43
raw of its bar, `FPoint.Min`'s tolerance, brings it to its bar exactly. A
full formation takes no share,
though a share is still set aside for it when it stands near. As the fight
ends, each formation's experience is cut down to a whole number, and that is
what the formation carries into the next round.

## Evidence

### Recorded

- What a kill hands out and to whom, as the section above states it: every
  formation's experience is in each recording's hash, and the
  simulator reproduces it tick by tick in every fight the topics pin, which
  every unit's `fights/`, `tests/marksman/fights/` among them, and the other
  topics' hold.
- A missile's kill, which no unit made and no one else shares, goes to every
  formation of the missile's side: `tests/missile/fights/crawlers.yaml`.
- An air drop's kills count for the dead ones' enemies, its own side's dead
  included, and a summon's own kills leave its side's formations their whole
  share: `tests/battle_skill/fights/rhino-drop.yaml`,
  `tests/battle_skill/fights/vulcans-descent.yaml`.
- A Fire Badger an ally's explosion fells hands its pool to every formation
  that hit it and to the enemies within 65 metres of it, and one that took its
  own life hands out nothing: `tests/extra_weapon/fights/scorching-charge.yaml`,
  `tests/extra_weapon/fights/scorching-charge-survivor.yaml`.
- An officer's experience rate multiplies its formation's every gain, the
  whole and the share, and no other formation's, and leaves the bar the table's:
  `tests/modifier/fights/officer-exp-rate-marksman.yaml`,
  `tests/modifier/fights/officer-exp-rate-arclight.yaml`, against
  `tests/modifier/fights/officer-exp-rate-none.yaml`.
- A technology's experience rate multiplies its unit's formation's gains and
  no other formation's: `tests/modifier/fights/technology-exp-rate.yaml`, where
  the Vortex ends with 112 and with 56 without Machine Learning.
- A kill that brings a formation within 4 raw of its bar brings it to the
  bar: `tests/corpus/fights/201370830-r5.yaml`, tick 403.
- A Steel Ball whose beam dealt the Crawler it then left nothing takes no
  share of it: `tests/corpus/fights/201477097-r4.yaml`, tick 715, where only
  the killer's formation takes the Crawler's pool.

### Replayed

- A unit an Incendiary Bomb's fire kills is shared by no formation the shell
  struck: `scripts/corpus/verify-matches.py` fights round 5 of replay
  201371791 as the match says only with the shell's hit left uncounted.

- A formation ends the fight on its experience cut down to a whole number, and
  opens the next round holding it: `scripts/corpus/match-replays.py` with `--recordings`
  converts every recorded round of the corpus to its fight document and
  compares each unit's `exp` with the next state in the match document. The
  corpus includes fights that ran out of time, whose recordings end before the
  cut, and the fractions they end on are cut down.

### Read

- The list a bar is read from is `[0, upgradeLv2, …, upgradeLv9]`, indexed by
  level, and the index is clamped to its last entry: `MechExpData.PreProcess`,
  `MechExpData.GetUpgradeExp`, `MechExpData.upgradeLv2`.
- A unit's bar is the entry for the level above its own:
  `UnitSystem.ChangeLevel`, `UnitUtility.GetUpgradeExp`,
  `CardData.GetUpgradeExp`.
- A full bar is kept as a state: `MechTeam.hasEnterMaxExp`, `MechTeam.IsExpMax`,
  `CardElement.IsExpMax`.
- A unit's bar is the table's entry unless its data set holds an
  `UpgradeExp`, which then replaces it: `CardElement.GetNextLevelExp`.
- A full unit takes no share of a fight's experience: `ExpSystem.IsValidOwner`.
- A kill's experience, the killer's whole and the shared pool:
  `ExpSystem.OnActorHitted`, `ExpSystem.DoCalculateExp`, `Config.assistKillExpRate`.
- A hit under 1 stops before the calculator and is never heard of:
  `DamagePerformer.PerformHitTargetEffect`,
  `FightCalculator.PerformHitTargetEffect`, `ExpSystem.OnActorHitted`.
- Who shares it, by attack and by distance: `ExpSystem.AddAttackData`,
  `ExpSystem.AddRangeUnit`, `Config.assistExpRange`, `RectRange.Overlaps`,
  `FightTeam.CreateQuadtree`; a hit of the target's own group shares with its
  opponents' units: `ExpSystem.OnActorHitted`, `GroupManager.GetOpponentGroups`.
- A unit's own blow takes its life from itself: `SuicideEffect.Perform`.
- The bar caps a gain through `FPoint.Min`, which answers the bar for a sum
  within 43 raw below it: `MechTeam.AddExp`, `FPoint.Min`.
- A gain starts from zero and stops at the bar, and is the amount times
  `(1 + expAddRate + add) × expReduceRate × reduce`, `add` and `reduce` being
  the unit's own `MechDataChangeFloatRate.ExpChangeRate`, of the killer or,
  for a share, of the formation's first unit: `MechTeam.AddExp`,
  `ExpSystem.DoCalculateExp`.
- A formation's `expAddRate` and `expReduceRate` are its card's
  `UnitDataChangeFloatRate.ExpChangeRate`, and start at zero and one:
  `CardElement.RefreshExpRate`, `MechTeam.ChangeExpRate`, `MechTeam..ctor`.
- A technology's rate reaches the unit and not the card: `TechnologyData`
  answers `ICommonMechDataChangeDataSource` and not
  `IUnitDataChangeDataSource`, and `TechnologyData.GetExpChangeRate` answers
  its entry at the unit's level.
- `MechTeam.AddExp` reads `FightMech.GetDataFloatAddRate` and
  `FightMech.GetDataFloatReduceRate` of `MechDataChangeFloatRate.ExpChangeRate`
  off the unit it is handed, or off `MechTeam.meches`' first when it is handed
  none, which `ExpSystem.DoCalculateExp` does for a share: `MechTeam.AddExp`.
- An officer's rate reaches the card and not the unit:
  `OfficerData.get_ExpChangeRate`, and `OfficerData.GetExpChangeRate`, which
  answers zero to `MechDataModifer.TryAddCommonData`.
- What a unit, a tower and a construction hand out: `MechExpData.lootExpLv1`,
  `FightMech.GetProvideExp`, `FightCrystal.GetProvideExp`,
  `towerDefaultDatas.exp`.
- A fight's end cuts experience to a whole number: `BattleSystem.OnFightOver`,
  `MechTeam.PruneExp`.
- Intensive Training refuses a full unit: `CS_AddExp.CheckAvaliable`.
- Every row but Vulcan's follows the formula; `scripts/extract/extract_prices.py`
  checks it on every row it writes, which it did on this version's table:
  `MechExpData.upgradeLv2`.

### Not established

- **A hit a unit's own energy shield takes whole.** `DamagePerformer` lets the
  shield take its part before it asks whether the hit deals at least 1, so
  such a hit may leave its owner out of the share as a shell that deals
  nothing does; the simulator counts it, and no recording shows a kill after
  one.
- **Intensive Training's refusals in play.** That it refuses a level 9 unit
  and a full one was observed in the Training Ground on another version; no
  test pins it.
- **Intensive Training reaching exactly the table's value.** Every release of
  it in another version's corpus did, at levels 1 through 4. This version's
  corpus holds releases of it, but its replay leaves a unit's experience to the
  fight and compares none: `scripts/corpus/verify-matches.py`. Levels 5 through 8
  are unobserved.
- **When a full bar becomes a level.** A fight stops a formation's gains at
  its bar; what turns a full bar into the next level after the fight is not
  read.
- **What writes a unit's `UpgradeExp`.** Not an officer's `expChangeRate`,
  which rates the gains and leaves the bar the table's in
  `tests/modifier/fights/officer-exp-rate-marksman.yaml`; no recording has a
  bar changed.
- **An impairment of the rate, and two rates on one card.** Every officer row
  that carries one is a single enhancement, and the simulator compounds an
  impairment and sums two enhancements as a card's `DataSet` does without a
  recording that pins either.
