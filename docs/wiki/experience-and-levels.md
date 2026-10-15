# Experience & Unit Levels

> **Scope:** This guide covers how formations earn experience in combat, how
> full experience bars and level upgrades work, what unit levels `1` through `9`
> scale, and the exact experience thresholds and kill bounties in **standard 1v1
> matches** on the repository's pinned game version. For deployment upgrade
> rules and supply costs, see
> **[A Round's Decisions — Upgrading a Formation's Level](a-rounds-decisions.md#upgrading-a-formations-level-upgrade_unit)**.

In Mechabellum, **experience belongs to a formation as a whole**, not to
individual mechs within the squad. Even if `23` out of `24` **Crawlers** die
during a fight, all experience earned by that squad during the battle is kept by
the formation when the full squad returns next round.

## What Unit Levels Scale (and What Stays Fixed)

Every formation has a level from **`1` to `9`**. Upgrading a formation from
level `1` to level `n` changes three things and leaves everything else untouched:

| Property | Level `n` Scaling (`n = 1 … 9`) | Practical Consequence |
| --- | --- | --- |
| **Maximum Life (HP)** | **`n ×` Level 1 Base HP** | A Level `2` mech has `200%` of its Level `1` HP; a Level `3` mech has `300%` |
| **Base Attack Damage** | **`n ×` Level 1 Base Attack** | Direct weapon damage scales linearly with level (`2×` at Level `2`, `3×` at Level `3`, up to `9×` at Level `9`), making high-level single-shot units cross one-shot kill thresholds |
| **Kill Bounty Handed Out (`loot_exp`)** | **`(2n - 1) ×` Level 1 Bounty** | Killing an enemy unit at Level `n` awards `1×`, `3×`, `5×`, `7×`, …, `17×` its Level `1` bounty—allowing under-leveled armies to catch up rapidly when they bring down a high-level veteran |
| **Reactor Core Settlement Score** | **Unchanged (`1×` at all levels)** | Across all `101` unit rows in the pinned game configuration, a surviving Level `9` mech deals the **exact same** reactor core damage as a surviving Level `1` mech (see **[Reactor Core Settlement](reactor-damage.md)**) |
| **Speed, Range, Attack Interval & Footprint** | **Unchanged** | Leveling never makes a unit walk faster, shoot farther, fire faster, or take up more board space |

## Experience Bars and the Full-Bar State

Each formation displays its experience as `current / maximum` (for example,
`124 / 450` on a Level `1` **Crawler** formation):

- **The 8 Progression Thresholds:** Each unit type has an 8-entry `upgrade_exp`
  table (`upgradeLv2` through `upgradeLv9`). The `n`-th entry is the experience
  required to fill the bar of a formation currently at **Level `n`** (for levels
  `1` through `8`). Because the game clamps level lookups to the last entry, a
  **Level `9`** formation also displays a bar whose maximum equals Level `8`'s
  threshold, even though Level `9` is the maximum level and cannot be upgraded
  further.
- **How Thresholds Scale by Level:** For every unit except **Vulcan**, the
  experience required to fill Level `n`'s bar follows a single curve from that
  unit's Level `1` threshold (`base`):
  `upgrade_exp(unit, n) = round_half_up(base × F(n))`, where `F(1..8)` is `1`,
  `2.25321`, `3.5618`, `4.49028`, `5.21046`, `5.79888`, `6.29639`, and
  `6.72735`. (**Vulcan** is the sole exception: its Level `1` bar is `2200`,
  while its Levels `2` through `8` follow the curve from a `2420` base, i.e.
  `1.1 × 2200`.)
- **What Happens When a Bar Fills (`IsExpMax`):** Once a formation's experience
  reaches its threshold (including gains that land within a tiny fixed-point
  tolerance of the cap), the formation is marked **full** (`IsExpMax`) and its
  experience is clamped to `maximum`. A full formation:
  1. **Unlocks the standard level-up button** during deployment (paying the unit's
     `upgrade_supply` increments its level by `+1` and resets its experience bar
     to `0`, with **zero spillover** of excess combat experience into the new
     level).
  2. **Stops taking any further combat experience shares** for the rest of the
     fight so that subsequent kills are not wasted on a capped squad.
  3. **Cannot be targeted by Intensive Training** (which also refuses Level `9`
     units).
- **Zero-Experience Same-Round Upgrades:** As detailed in
  **[A Round's Decisions — Upgrading a Formation's Level](a-rounds-decisions.md#upgrading-a-formations-level-upgrade_unit)**,
  a formation that arrived in the current round—or that already upgraded once in
  the current round—holds `0` combat experience and can be upgraded immediately
  by paying supply without filling its experience bar first.

## How Combat Kills Distribute Experience

During combat, experience is awarded **one kill at a time** whenever an enemy
unit, fixed tower (`100` base EXP), or construction (`20` EXP for **Defensive
Wall**, `150` EXP for **Anti-Armor Cannon** / **Rapid-Fire Cannon**) is
destroyed.

Every valid kill hands out the victim's `loot_exp` **twice** (`100%` killer
bounty + `100%` shared assist pool):

1. **The Killer's Direct Bounty (`100%`):**
   - If the killing blow was dealt by a mech belonging to an **eligible
     formation** (a persistent formation that is currently on its original team,
     not a summoned unit, not mind-controlled by the opposing side, and not
     already at a full experience bar), that killer's formation immediately
     receives **`100%` of the victim's `loot_exp`**.
   - If the kill was made by a non-unit source—such as a defensive turret, a
     **Sentry Missile**, or a **Missile Strike**—no single formation claims the
     killer's direct bounty; instead, that `100%` is added into the shared pool
     below.
2. **The Shared Assist Pool (`100%`):**
   - A second pool equal to `100%` of the victim's `loot_exp` is split **evenly**
     among every eligible allied formation that meets **either** of two
     conditions:
     - **Dealt Life Damage:** At least one mech in the formation dealt **`>= 1`
       actual life damage** to the victim earlier in the fight.
     - **Proximity (`65m`):** The formation has a mech within **`65` meters**
       (edge-to-center, queried via the side's spatial quadtree) of the victim
       when it dies.
   - **One Share per Qualifying Formation:** Each qualifying formation receives
     **one equal share** regardless of how many of its individual mechs hit the
     victim or stood within `65m`.
   - **Killer Double-Dips:** The killer's own formation also qualifies for one
     share of the assist pool (receiving `100%` direct bounty **plus** its `1/N`
     share of the assist pool).
   - **Fallback When No Formation Qualifies Locally:** If a non-unit kill (such
     as a long-range missile) occurs with no eligible formation having damaged
     the victim or standing within `65m`, the shared pool is split evenly across
     **every eligible formation on that side**.
   - **Full Formations & Summons:** Summoned units never take a share and are
     not counted in the split denominator. A formation whose bar is already full
     takes no experience, though its presence near the kill can still occupy a
     slot in the proximity check.

### Critical Edge Cases in Experience Attribution

- **Zero-Damage Hits Count for Nothing:** A hit registers for assist experience
  only if it removes at least `1` point of life. For example, an **Incendiary
  Bomb** shell impact deals `0` direct damage and only spawns fire on the
  ground—so a **Hound** formation that launched the shell does not count as
  having damaged victims that later burn to death in the fire. Likewise, the
  very first tick of a **Steel Ball** beam whose ramp multiplier rounds to `0`
  damage does not register its formation for an assist if the **Steel Ball**
  switches targets before dealing `1` damage.
- **Unowned Airdrop Impacts Feed the Victim's Enemies:** The landing impact of
  an airdrop skill has no unit owner and always credits the **enemies of
  whichever unit died**. If your own **Vulcan's Descent** or **Rhino Drop**
  lands on top of your own **Crawlers** and crushes them, the **opposing player**
  receives the experience for your dead **Crawlers**! Even if a summoned airdrop
  dies to its own landing damage, that experience goes to the enemy team.
- **Friendly-Fire Explosions Feed the Enemy:** When a unit dies to a hit coming
  from its **own side** (for example, when an allied **Fire Badger**'s death
  explosion kills a friendly mech next to it), the proximity assist pool is
  awarded to **opposing formations** within `65m` (plus any formations that had
  damaged the victim).
- **Self-Destruct (`SuicideEffect`) Awards Zero Experience:** When a unit takes
  its own life through a self-destruct effect (such as a **Spider Mine**
  detonating or a **Fire Badger** triggering **Scorching Charge** on itself),
  that self-inflicted death hands out **`0` experience** to anyone.

## Experience Rate Modifiers & End-of-Fight Rounding

Two types of modifiers multiply the experience a formation receives from both
direct kills and assist shares:

- **Officer Experience Modifiers (Card-Level):** Specialist and tech officers
  such as **Smart Marksman** (`+75%` EXP to **Marksman**), **Smart Arclight**
  (`+75%` EXP to **Arclight**), or **Training Specialist** modify the card's
  experience rate (`1 + card_enhancement`).
- **Technology Experience Modifiers (Unit-Level):** Unit technologies such as
  **Machine Learning** on **Vortex** (`+100%` EXP) or ** Combat Experience** /
  experience-boosting techs modify the unit's experience rate (`unit_enhancement`).
  If a unit's technologies are disabled by EMP, its technology experience bonus
  is suppressed while disabled.
- **Combined Formula:** When both apply, additive enhancements on the card and
  unit sum together before multiplying:
  `gain = base_amount × (1 + card_add + unit_add) × card_reduce × unit_reduce`.
  Modifiers multiply how fast the bar fills without changing the table's bar
  threshold itself.
- **End-of-Fight Whole-Number Truncation:** During combat, shared splits and
  percentage modifiers can produce fractional experience values. When the fight
  ends, every formation's experience is **truncated down to an integer** (for
  example, `124.8` becomes `124`), and that whole number is what persists into
  the next round.

## Complete Unit Experience & Kill Bounty Table

In the table below:

- **`Lv n → n+1` columns (`upgrade_exp`):** How much experience fills the bar of
  a formation currently at Level `n` (so `Lv 1→2` is the bar size at Level `1`).
  Level `9` displays the same bar size as `Lv 8→9`.
- **Kill Bounty (`loot_exp`):** How much base experience **one individual mech**
  of that type hands out when killed at Level `1`, Level `2` (`3×`), and Level
  `9` (`17×`). For any level `n` (`1–9`), `loot_exp(n) = loot_exp(1) × (2n - 1)`.

| Unit | Lv 1→2 | Lv 2→3 | Lv 3→4 | Lv 4→5 | Lv 5→6 | Lv 6→7 | Lv 7→8 | Lv 8→9 | Kill Bounty (Lv 1) | Kill Bounty (Lv 2) | Kill Bounty (Lv 9) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **Crawler** | `450` | `1014` | `1603` | `2021` | `2345` | `2609` | `2833` | `3027` | `4` | `12` | `68` |
| **Fang** | `600` | `1352` | `2137` | `2694` | `3126` | `3479` | `3778` | `4036` | `6` | `18` | `102` |
| **Hound** | `750` | `1690` | `2671` | `3368` | `3908` | `4349` | `4722` | `5046` | `20` | `60` | `340` |
| **Void Eye** | `650` | `1465` | `2315` | `2919` | `3387` | `3769` | `4093` | `4373` | `33` | `99` | `561` |
| **Arclight** | `750` | `1690` | `2671` | `3368` | `3908` | `4349` | `4722` | `5046` | `100` | `300` | `1700` |
| **Marksman** | `650` | `1465` | `2315` | `2919` | `3387` | `3769` | `4093` | `4373` | `100` | `300` | `1700` |
| **Vortex** | `750` | `1690` | `2671` | `3368` | `3908` | `4349` | `4722` | `5046` | `100` | `300` | `1700` |
| **Mustang** | `1200` | `2704` | `4274` | `5388` | `6253` | `6959` | `7556` | `8073` | `17` | `51` | `289` |
| **Sledgehammer** | `1300` | `2929` | `4630` | `5837` | `6774` | `7539` | `8185` | `8746` | `40` | `120` | `680` |
| **Steel Ball** | `1000` | `2253` | `3562` | `4490` | `5210` | `5799` | `6296` | `6727` | `50` | `150` | `850` |
| **Fire Badger** | `1400` | `3154` | `4987` | `6286` | `7295` | `8118` | `8815` | `9418` | `67` | `201` | `1139` |
| **Sabertooth** | `1300` | `2929` | `4630` | `5837` | `6774` | `7539` | `8185` | `8746` | `200` | `600` | `3400` |
| **Tarantula** | `1500` | `3380` | `5343` | `6735` | `7816` | `8698` | `9445` | `10091` | `200` | `600` | `3400` |
| **Wasp** | `900` | `2028` | `3206` | `4041` | `4689` | `5219` | `5667` | `6055` | `17` | `51` | `289` |
| **Stormcaller** | `1100` | `2479` | `3918` | `4939` | `5732` | `6379` | `6926` | `7400` | `50` | `150` | `850` |
| **Phantom Ray** | `1000` | `2253` | `3562` | `4490` | `5210` | `5799` | `6296` | `6727` | `67` | `201` | `1139` |
| **Phoenix** | `1300` | `2929` | `4630` | `5837` | `6774` | `7539` | `8185` | `8746` | `100` | `300` | `1700` |
| **Rhino** | `1150` | `2591` | `4096` | `5164` | `5992` | `6669` | `7241` | `7736` | `200` | `600` | `3400` |
| **Hacker** | `560` | `1262` | `1995` | `2515` | `2918` | `3247` | `3526` | `3767` | `200` | `600` | `3400` |
| **Typhoon** | `1950` | `4394` | `6946` | `8756` | `10160` | `11308` | `12278` | `13118` | `150` | `450` | `2550` |
| **Centurion** | `2250` | `5070` | `8014` | `10103` | `11724` | `13047` | `14167` | `15137` | `300` | `900` | `5100` |
| **Farseer** | `1800` | `4056` | `6411` | `8083` | `9379` | `10438` | `11334` | `12109` | `300` | `900` | `5100` |
| **Scorpion** | `1500` | `3380` | `5343` | `6735` | `7816` | `8698` | `9445` | `10091` | `300` | `900` | `5100` |
| **Wraith** | `2250` | `5070` | `8014` | `10103` | `11724` | `13047` | `14167` | `15137` | `300` | `900` | `5100` |
| **Fortress** | `1600` | `3605` | `5699` | `7184` | `8337` | `9278` | `10074` | `10764` | `400` | `1200` | `6800` |
| **Melting Point** | `1400` | `3154` | `4987` | `6286` | `7295` | `8118` | `8815` | `9418` | `400` | `1200` | `6800` |
| **Raiden** | `2400` | `5408` | `8548` | `10777` | `12505` | `13917` | `15111` | `16146` | `400` | `1200` | `6800` |
| **Sandworm** | `2000` | `4506` | `7124` | `8981` | `10421` | `11598` | `12593` | `13455` | `400` | `1200` | `6800` |
| **Vulcan** | `2200` | `5453` | `8620` | `10866` | `12609` | `14033` | `15237` | `16280` | `400` | `1200` | `6800` |
| **Overlord** | `1750` | `3943` | `6233` | `7858` | `9118` | `10148` | `11019` | `11773` | `500` | `1500` | `8500` |
| **Abyss** | `4800` | `10815` | `17097` | `21553` | `25010` | `27835` | `30223` | `32291` | `800` | `2400` | `13600` |
| **Mountain** | `3200` | `7210` | `11398` | `14369` | `16673` | `18556` | `20148` | `21528` | `800` | `2400` | `13600` |
| **War Factory** | `3200` | `7210` | `11398` | `14369` | `16673` | `18556` | `20148` | `21528` | `800` | `2400` | `13600` |
| **Larva** *(Summon)* | `1000` | `2253` | `3562` | `4490` | `5210` | `5799` | `6296` | `6727` | `50` | `150` | `850` |
| **Spider Mine** *(Summon)* | `170` | `383` | `606` | `763` | `886` | `986` | `1070` | `1144` | `6` | `18` | `102` |
