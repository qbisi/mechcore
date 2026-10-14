# How a Match Works

> **Scope:** This guide covers **standard 1v1 matches** (with no extra game
> rules enabled) on the repository's pinned game version. Asynchronous play,
> survival, roguelike modes, and matches with special game rules use different
> settings and are outside this scope.

A Mechabellum match is a round-based tactical contest between two sides (`blue`
and `red`). To understand why every deployment choice matters, it helps to trace
a match backward from how it ends:

1. **How do I win a match?** By reducing the opponent's **reactor core** (their
   match life total) to `0` or below—or when the opponent concedes.
2. **What damages the reactor core?** Every active mech still standing on the
   battlefield when a round's fight ends deals its settlement score to the
   other side's reactor core.
3. **How does the fight unfold?** Once both sides finish deploying, combat is
   completely automatic—there is no real-time unit micromanagement during the
   fight.
4. **What can I do?** Between fights, you choose reinforcements, unlock and buy
   unit formations, upgrade levels and unit technologies, fit equipment, place
   contraptions, strengthen towers, and aim commander skills.
5. **Why can't I buy or upgrade everything at once?** You are bounded by
   **supply**, per-round purchase and unlock allowances, and experience
   requirements for upgrading veterans.
6. **Why do this round's choices shape the rest of the match?** Formations—even
   those wiped out during a fight—return next round with their levels,
   experience, technologies, equipment, and board positions intact.

The sections below explain each link in that chain and close with the
[chronological timeline of a match](#round-by-round-timeline).

## How You Win a Match

A **match** is played over a sequence of **rounds**, and each round consists of
a pre-battle deployment phase followed by one automated **fight**. Ending a
fight ends only that round, not the match.

Each side enters the match with a **reactor core**, which is its match life
total:

- On standard 1v1 versus maps, both sides start from a base reactor core of
  `4500` before the round `0` opening choice modifies it.
- After Round `0`, a side's reactor core never increases; it only holds steady
  or falls after each round's fight settles.
- A match ends when at least one side's reactor core falls to `0` or below at
  the end of a round, or immediately when a side **concedes** during
  deployment. (Special tie-breaking rules if both reactor cores drop to `0` or
  below in the same round, or any standalone round-limit rulings, are not
  covered here.)

### Reactor Core vs. Battlefield Towers

The **reactor core is not a building on the battlefield**. Units never walk up
to or shoot at a reactor core during combat.

The structures that stand on the battlefield are combat objects:

- Each side's two fixed towers—the **Energy Tower** and the **Research Center**
- Each side's defensive **constructions** dealt by the map or placed by skills,
  such as **Defensive Wall** blocks, **Anti-Armor Cannon**, and **Rapid-Fire
  Cannon**

Destroying an enemy **Energy Tower** or **Research Center** during a fight
inflicts a temporary combat debuff on the losing side's live units and turrets
for that fight (reducing movement speed and damage dealt while increasing
damage taken). However, destroyed towers do not directly deal reactor core
damage when the round settles.

## Why a Fight Damages the Reactor Core

When a fight ends, settlement looks at the **active mechs still standing** on
the battlefield. A mech that fell during the fight scores `0`, no matter how
many enemies it destroyed before dying.

Each side adds up the scores of its surviving mechs, and any side with a
positive total subtracts that score from the **other side's** reactor core:

- **One side has survivors:** The losing side's reactor core falls by the
  surviving side's total score, and the surviving side's reactor core does not
  move.
- **Both sides have survivors** (for example, when the 120-second fight timer
  expires with mechs left on both teams): **Both** reactor cores take damage,
  each falling by the other side's surviving score.
- **Neither side has survivors** (for example, when a missile strike or mutual
  destruction eliminates the last mechs on both sides): Neither reactor core
  takes damage (`0` to both).

```text
+-------------------------------------------------------------------------+
|                           PRE-BATTLE DEPLOYMENT                         |
|  Both sides secretly spend supply & allowances: choose reinforcement,   |
|  buy/unlock/upgrade formations, research techs, fit equipment,          |
|  strengthen towers, place contraptions, and aim commander skills.       |
+------------------------------------+------------------------------------+
                                     |
                                     v
+-------------------------------------------------------------------------+
|                         AUTOMATED BATTLEFIELD                           |
|  Formations, turrets, and contraptions move, target, and fight          |
|  automatically. Destroying an enemy Energy Tower or Research Center     |
|  weakens that side in the fight, but towers are not the reactor core.   |
|                                                                         |
|      [Blue Surviving Mechs]                 [Red Surviving Mechs]       |
+-----------------+-------------------------------------+-----------------+
                  |                                     |
    Sum of Blue   |                                     |   Sum of Red
  survivor scores |                                     | survivor scores
                  v                                     v
     +-------------------------+           +-------------------------+
     |    RED REACTOR CORE     |           |    BLUE REACTOR CORE    |
     | (Match life outside map)|           | (Match life outside map)|
     +-------------------------+           +-------------------------+
```

### Unit Type vs. Unit Level in Settlement

A formation on the board is made up of one or more individual **mechs** (for
instance, a **Crawler** formation has `24` mechs, a **Fang** formation has `18`,
a **Sledgehammer** formation has `5`, and a **Marksman**, **Rhino**, or
**Fortress** formation has `1`), and settlement counts each surviving mech.

| Unit | Mechs per Formation | Score per Standard Surviving Mech (Lv 1–9) |
| --- | ---: | ---: |
| **Crawler** | 24 | 4 |
| **Fang** | 18 | 6 |
| **Wasp** / **Mustang** | 12 | 17 |
| **Sledgehammer** | 5 | 40 |
| **Steel Ball** / **Stormcaller** | 4 | 50 |
| **Marksman** / **Arclight** / **Vortex** | 1 | 100 |
| **Rhino** / **Hacker** / **Sabertooth** / **Tarantula** | 1 | 200 |
| **Fortress** / **Vulcan** / **Melting Point** / **Sandworm** / **Raiden** | 1 | 350 |
| **Overlord** | 1 | 400 |
| **War Factory** / **Abyss** / **Mountain** | 1 | 500 |

Two rules govern how unit stats relate to reactor core damage:

- **Different unit types have different per-mech scores.** A single surviving
  **Fortress** (`350`) deals much more reactor core damage than a single
  surviving **Marksman** (`100`) or a single surviving **Crawler** (`4`).
- **For the currently pinned game configuration, leveling up a unit does not
  directly change its per-surviving-mech reactor score.** Across all `101` unit
  entries in the game's table, a unit's score is identical from level `1`
  through level `9`: a surviving standard **Marksman** deals `100` reactor core
  damage at level `1` and `100` at level `3`.
- **Why leveling still matters:** A unit at level `n` (from `1` to `9`) has `n`
  times its base life and `n` times its base attack damage. Leveling helps your
  mechs win engagements and keep more allies alive at the end of the fight,
  which increases reactor core damage indirectly through survivor count.

### Exceptions That Modify a Mech's Score

Not every surviving mech scores its full table value:

- **Summoned mechs:** Any mech that did not deploy from one of its side's own
  formations—such as units spawned or produced during combat, dropped by a
  commander skill, or added solely for that fight—has its score reduced.
- **Reborn mechs:** A mech that died during the fight and was brought back by a
  rebirth technology has its score reduced if it is standing when the fight
  ends (and scores `0` if it is still waiting to finish reassembling when the
  fight ends).
- **Turned mechs:** A mech fighting for the opposing side at the end of the
  fight (such as one captured by a **Hacker** control beam) has its score
  reduced and scores **for the side it serves when the fight ends**, not the
  side that originally deployed it.

If more than one of those conditions applies to the same mech, the reduction
rates multiply and the mech's score is rounded down individually before adding
to its side's total. Destroyed towers and any supply crates collected during a
fight never count toward reactor core damage.

## How a Fight Unfolds

Once both players finish their deployment phase, the round's fight runs
automatically:

- **No real-time unit control:** You cannot click units to steer their movement,
  reassign their targets, or manually trigger their weapons during combat.
- **Automated combat loop:** Formations, defensive turrets, and contraptions act
  according to their positions, stats, researched technologies, and fitted
  equipment. Mechs advance toward enemy forces, select targets within their
  attack range and domain (ground vs. aerial), fire projectiles or beams or
  strike in melee, strip shields before reducing life, and earn experience for
  their formations when enemies are killed.
- **Decision timing vs. effect timing:** Even battlefield powers (**commander
  skills**) and **contraptions** whose effects trigger during combat—such as
  **Missile Strike**, **Lightning Storm**, **Shield Airdrop**, or a **Sentry
  Missile**—are selected and aimed during the **deployment phase** before the
  fight starts, never clicked live mid-battle.

## What You Decide Before the Fight

Every decision you make happens before combat begins—either in Round `0` (the
opening) or during each round's deployment phase. Both sides make their
deployment decisions secretly and simultaneously: neither player sees the
other's new purchases, moves, or aimed skills until the fight begins.

Across a match, all `14` player action types fall into six practical groups
(finishing deployment simply closes the phase and is not a separate tactical
action):

| Goal | What You Can Do | Recorded Action Types |
| --- | --- | --- |
| **1. Choose your opening (Round 0)** | Pick one of four private opening offers, each pairing an **advance team** of starting units with a **specialist** officer | `choose_advance_team` |
| **2. Answer the reinforcement offer (Round 2+)** | Pick one of the four shared reinforcement cards (an officer, commander skill, equipment item, or unit squad grant) or **decline** the offer in exchange for supply | `choose_reinforce_item` |
| **3. Expand and upgrade your army** | Unlock a unit type in the shop, buy a new formation onto the board, upgrade a formation's level, or research a unit technology from your match loadout | `unlock_unit`, `buy_unit`, `upgrade_unit`, `upgrade_technology` |
| **4. Position and equip formations** | Move eligible formations within open deployment regions and fit owned equipment items onto formations that have a free slot | `move_unit`, `use_equipment` |
| **5. Use towers, blueprints, skills, and contraptions** | Activate a Research Center blueprint, trigger a one-round Energy Tower skill, strengthen the Energy Tower or Research Center, release a commander skill from your panel, or buy and place a contraption | `active_blueprint`, `active_energy_tower_skill`, `strengthen_tower`, `release_commander_skill`, `release_contraption` |
| **6. Concede the match** | Surrender the match immediately without fighting the current round | `concede` |

## Supply, Allowances, and Upgrade Conditions

You cannot buy or upgrade an unlimited army in a single round. Three distinct
constraints govern what you can do during deployment.

### 1. Supply Income and Carryover

**Supply** is the currency spent in the shop and on towers, blueprints,
technologies, and reinforcement cards:

- **Base round income:** On every standard 1v1 versus map, Round `1` grants
  `200` base supply, and each later round adds `200` more (`200` in Round `1`,
  `400` in Round `2`, `600` in Round `3`, and so on), following
  `min(200 + (round - 1) × 200, 4000)`.
- **Base income cap vs. held supply:** `4000` is the ceiling on a single
  round's **base income**, not a cap on how much total supply you can hold.
- **Unspent supply carries over:** Any supply you do not spend in a round stays
  in your reserve for the next round (without earning interest).
- **Other supply sources and deductions:**
  - **Officers and equipment** held as a round opens can add bonus round supply,
    first-round supply, or one-time supply, or discount unit, unlock, upgrade,
    and technology costs.
  - **Declining a reinforcement offer** (from Round `2` onward) grants supply
    instead of a card: `50` supply in an ordinary reinforcement round, or the
    round schedule's refund in a unit reinforcement round.
  - **Recovering** one of your own formations or constructions with a recovery
    commander skill (such as **Field Recovery**) removes it from the board and
    refunds supply.
  - **Energy Tower loans:** Activating the Energy Tower skill **Rapid Resupply**
    costs `0` to trigger and grants `200` supply immediately, but deducts `300`
    supply from your next round's income.
  - **No combat supply in standard 1v1:** Nothing that happens during the fight
    itself adds supply to your ledger between rounds.

### 2. Per-Round Action Allowances

Even if you have plenty of supply, each round limits how many shop expansions
and devices you can deploy:

- **Unit purchases:** At most **`2` formations bought per round** by default.
  Activating the Energy Tower skill **Mass Recruitment** adds `1` purchase for
  the current round, and taking the officer card **Additional Deployment Slot**
  adds `1` purchase to the current round and every round after.
- **Unit unlocks:** At most **`1` unit type unlocked per round** in the shop
  (in addition to any unit types automatically unlocked by your opening, an
  officer's schedule, or a unit reinforcement card).
- **Contraption releases:** At most **`8` contraptions** (**Shield Generator**,
  **Sentry Missile**, or **Missile Interceptor**) bought and placed per round.

### 3. Experience Conditions for Upgrading a Formation

Paying supply is **not** the only requirement to raise a formation's level
(`upgrade_unit`):

- **Veterans that have earned experience need a full experience bar:** Once a
  formation has fought and holds experience toward its next level, you can pay
  to upgrade it **only when its experience bar is full** (`current/maximum` has
  reached `maximum`, either from kills and assists in combat or from targeting
  the formation with the deployment skill **Intensive Training**).
- **Newly arrived or just-leveled formations:** A formation that holds no
  experience since it joined or last leveled—which includes every formation that
  **arrived or leveled up in the current round**—can be upgraded immediately by
  paying its upgrade supply cost, without needing a full experience bar.
- **Upgrading resets experience:** Raising a formation's level starts the new
  level with `0` experience, and no formation can be upgraded past level `9`.

## What Persists Into the Next Round

A common misconception for new players is assuming that mechs destroyed in
combat are gone forever, or conversely that everything on the board resets. In
Mechabellum, **your army and its progression persist across rounds, while normal
combat casualties are restored**.

| Category | What Carries Into the Next Round | What Resets or Changes |
| --- | --- | --- |
| **Formations** | Every formation you own (unless removed by a recovery skill during deployment) returns at the start of the next round with its full mech count restored, at its level and board position | Mechs destroyed during the fight are restored; any formation that deployed on a flank (`travelling`) finishes arriving and settles |
| **Experience** | Each formation keeps the experience it earned in the fight, cut down to a whole number at fight end | Upgrading a formation resets its experience bar to `0`; a formation already at a full bar takes no further combat experience share |
| **Shop & Research** | Unlocked unit types, researched unit technologies, held officers, activated blueprints (with level II chain blueprints replacing level I), and tower strengthen levels (`0` to `4`) | Per-round shop allowances (`2` buys, `1` unlock, `8` contraptions) reset to their starting values plus officer modifiers |
| **Supply & Energy Tower** | Unspent supply carries over intact; next round's income is added | One-round Energy Tower skills expire; any **Rapid Resupply** debt (`300`) is deducted from the new round's income |
| **Equipment** | Both unfitted items in stock and items fitted onto formations persist; recovering a formation returns its fitted equipment to stock | Nothing wears out in standard 1v1 |
| **Constructions, Contraptions & Field Objects** | Surviving initial constructions, surviving contraptions, surviving **Shield Airdrop** domes, and **Sticky Oil Bomb** areas with remaining round duration stay on the board | Constructions, contraptions, and shields destroyed during combat do not regenerate; **Sticky Oil Bomb** areas count down each round and vanish at `0`; spent commander skills enter or tick down their cooldowns |

### Why You Cannot Freely Move Old Formations Every Round

Although your formations stay on the board from round to round, **their
positions are mostly permanent commitments**:

- **Arrival round:** A formation may move freely during the deployment phase of
  the round in which it **arrives**—whether you bought it from the shop, took a
  reinforcement card that granted it, or received it as an opening or officer
  delivery at the start of the round.
- **Later rounds (`legacy` formations):** Once a formation has stayed into a
  later round, it is locked in place and **cannot move** unless one of three
  things frees it:
  1. **Deployment Module** (equipment): Frees the formation wearing it to move
     in every round.
  2. **Jump Drive** (unit technology on **Wasp**, **Overlord**, or **Phoenix**):
     Frees every formation of that unit type to move in every round.
  3. **Redeployment** (commander skill): Frees the single targeted formation to
     move for the rest of the current round.

In Round `1`, formations can only be placed inside your main deployment half.
Starting in Round `2`, the two **flank** regions along the sides of the
opponent's half open up as well; moving or buying a formation into a flank marks
it as `travelling` for that round's fight, after which it stays settled there in
later rounds.

## Round-by-Round Timeline

Putting the pieces back in forward chronological order gives the complete loop
of a standard 1v1 match:

```text
Round 0 — Opening
  Map deals starting constructions and base reactor core (4500).
  Each side privately picks 1 of 4 (Advance Team + Specialist) offers.
         │
         ▼
Round 1 — First Deployment & Fight (No Reinforcement Card Step)
  Chosen advance teams land on the board and unlock their unit types.
  Base round 1 supply (200) + any round 1 officer supply arrives.
  Both sides secretly deploy in their main regions (flanks closed in Round 1).
  Automated fight runs -> surviving mechs damage opposing reactor core.
         │
         ▼
Round 2+ — Full Round Loop
  1. Round Opens
     • Owned formations return at full mech count; surviving objects stay.
     • Skill cooldowns tick down; allowances reset.
     • Base supply min(200 + (round - 1) × 200, 4000) + officer/equipment
       income − any Rapid Resupply debt arrives; scheduled officer squads land.
  2. Shared Reinforcement Offer
     • Both sides see the same 4 reinforcement cards.
     • Each side first takes 1 card (paying its cost) or declines for supply.
  3. Secret Deployment
     • Spend supply & allowances to unlock, buy, upgrade (if eligible),
       research technologies, fit equipment, move eligible formations
       (flanks now open), strengthen towers, activate blueprints/tower skills,
       place contraptions, and aim commander skills.
  4. Automated Fight
     • Both sides finish deployment; combat runs without manual control.
  5. Settlement & Match Check
     • Standing mechs subtract their scores from the opponent's reactor core;
       formations keep their whole-number combat experience.
     • If a side's reactor core is 0 or below (or a side conceded), the match
       ends; otherwise, the next round opens.
```
