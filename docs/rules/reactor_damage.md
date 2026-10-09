# Reactor damage

What a fight takes off each side's reactor core: every unit still standing
when the fight ends scores for the side it serves, and each side that scores
takes its score off the other side's core.

The machine-readable table is
[`config/reactor_damage.yaml`](../../config/reactor_damage.yaml).
`scripts/extract/extract_prices.py` copies it verbatim out of the build: each unit's
`score` per level from `mechExpDatas`, and the three rates below from the
`Config` object of `level0`.

The rule holds for a standard 1v1 fight. A match in the asynchronous mode, or
set to a score mode other than reducing the core, settles its score another
way, and nothing here covers it.

## A unit's score

A unit scores when it is alive at the fight's end and among the units its
side's team holds active. A unit that has fallen scores nothing, however much
it did before it fell.

Its score is its row's entry at its level, times a rate for each way it did
not come from its own side's formations, rounded down:

```text
score(unit) = floor(score_at_level(unit) × Π rates that apply)
```

- **Summoned**, the `support_unit_score_rate`: every unit the side did not
  deploy from its own formations. A unit a skill summons, a unit another unit
  produces or spawns during the fight, and a unit an officer or a technology
  hands the side as the fight is built are all summoned, even when a formation
  of the same type stands on the board.
- **Reborn**, the `rebirth_unit_score_rate`: a unit that died in this fight
  and was brought back. Rebirth is a unit technology's, and it revives the
  unit that died rather than making another, so a unit that falls and stands
  at the end is a reborn one. A summoned unit that is reborn takes both rates.
- **Changed sides**, the `team_changed_unit_score_rate`: a unit fighting for a
  side other than the one it started on. It scores for the side it serves at
  the end, not the side that deployed it.

The rates multiply: a summoned unit that changed sides takes both. The rounding
is applied once, to the unit's product, and never to a side's sum.

A level past the table's last reads the last. Where a row scores every level
alike, which is how a reader can score a summoned unit whose level no document
states, the level does not matter.

A worked example, reading `config/`: a level 1 Fortress deployed from its
formation scores its row's first entry; a Fortress that a Tarantula's control
has turned scores that entry times `team_changed_unit_score_rate`, rounded
down, for the side that now controls it.

## What a side's core takes

A side's score is the sum of its units' scores. Every side whose score is
above zero takes that score off every other side's core. In a 1v1:

- **One side stands**: the other side's core falls by the survivors' score,
  and the standing side's core does not move.
- **Both sides stand**, as when a fight runs out of time: each core falls by
  the other side's score.
- **Neither side stands**: no core moves.

Nothing else enters. There is no minimum and no ceiling on the damage, no term
for the round, and the towers a fight destroyed do not count. What a side
collected from supply crates during the fight is added into the fight's report
and taken out again before the score is compared, so it does not count either.

## Evidence

### Recorded

- A fight's last tick records each side's score as the game computes it, its
  `team_scored`, and the simulator answers the same score on every pinned
  fight: the Rhino that wins scores its row's 200 for red,
  `tests/control/fights/anti-interference-rhino.yaml`, and a Missile Strike
  that leaves neither side standing scores 0 for both,
  `tests/battle_skill/fights/heavy-missile-strike.yaml`.

### Replayed

- Each core falls, round by round, by exactly what the rule above answers from
  the round's recording, the scores its last tick records as
  `mechcore convert <recording> --to fight` reads them: `scripts/corpus/match-replays.py` records every round of
  the corpus with the game and compares its answer with the fall between the
  round's state and the next in the match document, and `--recordings` repeats
  the comparison over recordings already made. The fights of the corpus
  include both sides standing, summoned units the fight made during it and as
  it was built, and units that changed sides.

### Read

- A fight's end hands its result to the score, which the team system turns into
  core damage: `BattleSystem.OnFightOver`, `FightResultController.CalculateResult`,
  `FightOverStateController.Enter`, `TeamSystem.CalculateFightScore`,
  `TeamScoreCalculator.Perform`.
- A side's score is its standing units' scores, counted from the team's active
  units: `FightResultController.CalculateScore`, `FightTeam.activeMeches`,
  `FightReport.Score`.
- A side's supply crates are added into the report and taken out again:
  `FightRecord.Bonus`, `TeamScoreCalculator.CalculateTeamScore`.
- A unit's score is its row at its level times the rates, rounded down:
  `FightMech.GetScore`, `MechExpData.GetScore`, `MechExpData.scoreLv1`.
- A level past the last reads the last: `MechExpData.GetData`.
- The rate for a unit not deployed from its formations applies to any creation
  type but the default: `FightMech.mechCreateType`,
  `Config.supportUnitScoreRate`.
- Rebirth is a unit technology's, which says how many times a unit is
  brought back: `RebirthTech.GetRebirthCount`, `RebirthTech.GetUnitID`.
- A rebirth revives the unit that died, and it is the one place a unit's
  rebirth count grows, so a unit that died in the fight and stands at its end
  has been reborn: `DeadRebirthController.PerformDeadEffect`, `RebirthTask.RebirthMech`,
  `FightMech.AddRebirthCount`.
- The rate for a reborn unit applies once it has been reborn:
  `FightMech.rebirthCount`, `FightMech.GetScore`,
  `Config.rebirthUnitScoreRate`.
- The rate for a unit on another side: `Config.teamChangedUnitScoreRate`.
- Every side that scores takes its score off every other side's core, with no
  clamp: `TeamScoreCalculatorReduce.Perform`, `TeamScoreGaugeReduce.ChangeScore`,
  `Player.reactorCore`, `Player.reactorCoreReduceValue`.
- Destroyed towers are counted in the report and not in the score:
  `FightReport.DestroyedCrystalCount`.
- The standard mode reduces the core and is not asynchronous:
  `BattleSetting.ScoreMode`, `Match.IsAsyncMode`.

### Not established

- **A reborn unit in play.** No pinned fight leaves a reborn unit standing.
  A recording of two Typhoons with Field Reassembly that die, are reborn and
  stand at the end scored each at its row's 125 times
  `rebirth_unit_score_rate`, 31, in the game's own `team_scored`; it is pinned
  once the simulator fights the technology.
- **A summoned unit's level.** No document states it, and no row of this
  table scores levels differently, so nothing here depends on it. A row that
  did would leave a summoned unit's score unanswered.
- **How the simulator tells a summoned unit.** The build reads the unit's
  creation type, for which the simulator takes whether the fight made the
  unit rather than its side deploying it from a formation; no pinned fight
  tells the two apart.
