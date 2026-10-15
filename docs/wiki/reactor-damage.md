# Reactor Core Settlement

> **Scope:** This guide covers how post-fight reactor core damage is calculated,
> why unit levels do not alter per-mech settlement scores in the pinned build,
> how summoned, reborn, and mind-controlled mechs are penalized, and the
> complete score table for **standard 1v1 matches** on the repository's pinned
> game version. For the overall match loop, see
> **[How a Match Works](how-a-match-works.md)**. For unit stats and squad sizes,
> see **[Units & Battlefield Objects](units.md)**.

At the end of every round's fight, the battlefield state is converted into
damage against one or both players' **reactor cores**—the off-board match life
totals (`4500` base on standard 1v1 maps) that determine who wins the match.

## Which Objects Count in Settlement

When a fight finishes (either because one side has been eliminated or because
the `120`-second combat timer has expired), settlement scans every combat group
on the board and filters down to **standing, active mechs**:

1. **Must Be Alive and Active (`IsActive`):** Any mech destroyed during the
   fight deals `0` reactor core damage, regardless of how much damage it dealt
   before dying.
2. **Wreckage / Pending Rebirth Scores `0`:** A mech with a rebirth technology
   (such as **Fortress**, **Phantom Ray**, or **Wraith** rebirth/reassembly) that
   has been knocked into a wreckage state and has **not finished reassembling**
   before the fight ends is not active and scores **`0`**.
3. **Flank Units Still Arriving (`travelling`) DO Count:** If a fight ends in
   under `8` seconds (or `4` seconds with **Quick Teleport**) while a flank
   formation is still in its **[Super Deployment (`travelling`)](deployment-and-mobility.md#flank-deployment-and-travelling-super-deployment)**
   arrival state, its surviving mechs are active and **do** deal their full
   reactor core score.
4. **Pending Production Batches Score `0`:** Mechs queued inside a **War
   Factory** or other production line that have not yet spawned onto the board
   when the fight ends do not exist as active actors and score **`0`**.
5. **Towers, Constructions, Contraptions & Supply Crates Score `0`:** Surviving
   or destroyed **Energy Towers**, **Research Centers**, **Defensive Walls**,
   **Anti-Armor Cannons**, **Rapid-Fire Cannons**, **Sentry Missiles**, **Shield
   Generators**, and battlefield supply crates never add to or subtract from
   reactor core damage.

## Why Unit Levels Do Not Change Per-Mech Score

A frequent misconception is that upgrading a unit to Level `2` or Level `3`
increases the reactor core damage that unit deals when it survives.

In the repository's pinned game configuration (`config/reactor_damage.yaml`,
extracted from `mechExpDatas.scoreLv1` through `scoreLv9`), **every single unit
row has the exact same score from Level `1` through Level `9`**:

- A surviving Level `1` **Marksman** deals `100` reactor core damage, and a
  surviving Level `9` **Marksman** deals `100` reactor core damage.
- A surviving Level `1` **Fortress** deals `350` reactor core damage, and a
  surviving Level `9` **Fortress** deals `350` reactor core damage.
- Why upgrading still drives reactor damage indirectly: as shown in
  **[Experience & Unit Levels](experience-and-levels.md)**, a Level `n` mech has
  `n×` life and `n×` attack damage, which lets it survive engagements and wipe
  out enemy squads so more of your mechs remain standing at settlement.

## The Three `× 0.25` Penalty Multipliers

Not every standing mech deals its full table score. Before adding a mech's score
to its side's total, settlement checks three independent conditions, each of
which multiplies that mech's score by **`0.25`** (`1073741824 / 4294967296` in
fixed-point arithmetic):

| Penalty Category | Condition on the Surviving Mech | Multiplier |
| --- | --- | --- |
| **1. Summoned (`is_support`)** | The mech was spawned mid-fight by a production technology (**War Factory**, **Overlord**, **Vulcan**, **Fortress**, **Farseer**, **Hound**, **Void Eye**, **Sandworm Larva**, **Spider Mine**), a commander skill airdrop (**Crawler**, **Rhino**, **Wasp**, **Vulcan's Descent**), or a temporary combat summon rather than deployed from a persistent formation | **`× 0.25`** |
| **2. Reborn (`has_reborn`)** | The mech died at least once during the current fight and successfully completed a rebirth / reassembly technology before the fight ended | **`× 0.25`** |
| **3. Turned (`team != original_team`)** | The mech is currently fighting for the opposing side when the fight ends (for example, captured by an enemy **Hacker** control beam and not reclaimed before fight end) | **`× 0.25`** (and scores **for the side it serves at fight end**) |

### Per-Mech Floor Rounding & Compounding

When a mech qualifies for one or more penalties, the multipliers compound on
that individual mech and the result is **rounded down to an integer (`floor`)
per mech** before summing across the army:

```text
mech_damage = floor(base_score × (0.25 ^ active_penalties))
side_core_damage_dealt = sum(mech_damage for each active mech currently on that side)
```

Because rounding happens **per individual mech** rather than on the squad total,
small-score units can drop to `1` or `0` under penalties:

- **Single penalty (`× 0.25`):**
  - A summoned or turned **Crawler** (`4` base score) deals `floor(4 × 0.25) = 1`.
  - A summoned or turned **Fang** (`6` base score) or **Spider Mine** (`6` base
    score) deals `floor(6 × 0.25) = 1`.
  - A summoned **Wasp** (`17` base score) deals `floor(17 × 0.25) = 4`.
  - A summoned **Steel Ball** or **Larva** (`50` base score) deals
    `floor(50 × 0.25) = 12`.
  - A summoned **Rhino** (`200` base score) deals `floor(200 × 0.25) = 50`.
  - A summoned **Vulcan** (`350` base score) deals `floor(350 × 0.25) = 87`.
- **Two stacked penalties (`× 0.25 × 0.25 = × 0.0625`):**
  - If a summoned **Rhino** (`200` base score) is captured by an enemy
    **Hacker** and survives on the enemy team, it deals
    `floor(200 × 0.25 × 0.25) = 12` to its original owner's reactor core.
  - If a summoned **Crawler** (`4` base score) or **Fang** (`6` base score) is
    captured by an enemy **Hacker**, `floor(4 × 0.0625) = 0` and
    `floor(6 × 0.0625) = 0`—it deals **`0`** reactor core damage!
- **Captured Production Spawns:** If a **Hacker** captures an enemy production
  unit (such as a **War Factory** or **Overlord**) and that unit subsequently
  produces a mech while controlled, the newly spawned mech is created natively
  on the controller's team (`team == original_team`), so it carries only the
  **summoned** penalty (`× 0.25`), not the turned penalty.

## Simultaneous Damage on Timeout

When the `120`-second fight timer expires with active mechs still standing on
both sides:

- `blue`'s surviving mechs deal their summed score to `red`'s reactor core.
- `red`'s surviving mechs deal their summed score to `blue`'s reactor core.
- Both deductions apply at the end of that fight.

## Complete Reactor Core Score Table (All 33 Units + Summons)

Because scores do not vary by level in the pinned game build, every entry below
applies at **all unit levels (`Lv 1–9`)**:

| Unit | Mechs per Formation | Base Score per Mech | Full Unpenalized Squad Score | Score with 1 Penalty (`× 0.25`) | Score with 2 Penalties (`× 0.0625`) |
| --- | ---: | ---: | ---: | ---: | ---: |
| **Crawler** | `24` | `4` | `96` | `1` | `0` |
| **Fang** | `18` | `6` | `108` | `1` | `0` |
| **Hound** | `5` | `20` | `100` | `5` | `1` |
| **Void Eye** | `3` | `33` | `99` | `8` | `2` |
| **Arclight** | `1` | `100` | `100` | `25` | `6` |
| **Marksman** | `1` | `100` | `100` | `25` | `6` |
| **Vortex** | `1` | `100` | `100` | `25` | `6` |
| **Mustang** | `12` | `17` | `204` | `4` | `1` |
| **Sledgehammer** | `5` | `40` | `200` | `10` | `2` |
| **Steel Ball** | `4` | `50` | `200` | `12` | `3` |
| **Fire Badger** | `3` | `67` | `201` | `16` | `4` |
| **Sabertooth** | `1` | `200` | `200` | `50` | `12` |
| **Tarantula** | `1` | `200` | `200` | `50` | `12` |
| **Wasp** | `12` | `17` | `204` | `4` | `1` |
| **Stormcaller** | `4` | `50` | `200` | `12` | `3` |
| **Phantom Ray** | `3` | `67` | `201` | `16` | `4` |
| **Phoenix** | `2` | `100` | `200` | `25` | `6` |
| **Rhino** | `1` | `200` | `200` | `50` | `12` |
| **Hacker** | `1` | `200` | `200` | `50` | `12` |
| **Typhoon** | `2` | `125` | `250` | `31` | `7` |
| **Centurion** | `1` | `250` | `250` | `62` | `15` |
| **Farseer** | `1` | `250` | `250` | `62` | `15` |
| **Scorpion** | `1` | `250` | `250` | `62` | `15` |
| **Wraith** | `1` | `250` | `250` | `62` | `15` |
| **Fortress** | `1` | `350` | `350` | `87` | `21` |
| **Melting Point** | `1` | `350` | `350` | `87` | `21` |
| **Raiden** | `1` | `350` | `350` | `87` | `21` |
| **Sandworm** | `1` | `350` | `350` | `87` | `21` |
| **Vulcan** | `1` | `350` | `350` | `87` | `21` |
| **Overlord** | `1` | `400` | `400` | `100` | `25` |
| **Abyss** | `1` | `500` | `500` | `125` | `31` |
| **Mountain** | `1` | `500` | `500` | `125` | `31` |
| **War Factory** | `1` | `500` | `500` | `125` | `31` |
| **Larva** *(Sandworm Summon)* | — | `50` | — | `12` | `3` |
| **Spider Mine** *(Summon)* | — | `6` | — | `1` | `0` |
| **Tentacle** *(Abyss Summon, `unit_id: 101`)* | — | `0` | — | `0` | `0` |
