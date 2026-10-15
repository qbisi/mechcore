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
   contraptions, strengthen towers, and aim commander skills (see
   **[A Round's Decisions](a-rounds-decisions.md)**).
5. **Why can't I buy or upgrade everything at once?** You are bounded by
   **supply**, per-round purchase and unlock allowances, and experience
   requirements for upgrading veterans.
6. **Why do this round's choices shape the rest of the match?** Formations—even
   those wiped out during a fight—return next round with their levels,
   experience, technologies, equipment, and board positions intact, while most
   existing formations stay locked at their positions unless freed (see
   **[Deployment, Flanks, and Mobility](deployment-and-mobility.md)**).

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

### How Surviving Mechs Are Scored

A formation on the board consists of `1` to `24` individual **mechs** (for
instance, `24` in a **Crawler** squad, `5` in a **Sledgehammer** squad, or `1`
in a **Marksman** or **Fortress**), and settlement scores each standing mech
individually:

- **Per-mech score depends on unit type, not level:** Each unit type has a fixed
  per-mech score (from `4` for a **Crawler** up to `500` for a **War Factory**,
  **Abyss**, or **Mountain**). In the pinned game configuration, a unit's score
  is identical from Level `1` through Level `9`—upgrading a formation scales its
  life and attack damage by `× n` (see
  **[Experience & Unit Levels](experience-and-levels.md)**), which increases
  reactor core damage indirectly by keeping more of your army alive.
- **Summoned, reborn, and turned mechs take a `× 0.25` penalty:** Any surviving
  mech that was spawned mid-fight (`is_support`), revived by a rebirth
  technology, or captured to the opposing team by a **Hacker** has its score
  multiplied by `0.25` per applicable condition and rounded down per mech, while
  towers, constructions, contraptions, and unfinished rebirth wreckages score
  `0`.

For the complete `33`-unit settlement table and compounding penalty examples,
see **[Reactor Core Settlement](reactor-damage.md)**. For unit sizes, domains,
and base stats, see **[Units & Battlefield Objects](units.md)**.

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

Every decision you make happens before combat begins—either in Round `0` (when
each side privately picks an **advance team** paired with a **specialist**
officer) or during each round's secret, simultaneous deployment phase.

Across a match, all `14` player action types fall into six practical goals:

1. **Choose your opening (Round `0`)** — Pick one of four private `(Advance Team + Specialist)` combinations.
2. **Answer the reinforcement offer (Round `2+`)** — Pick one of the four shared reinforcement cards or decline the offer for supply.
3. **Expand and upgrade your army** — Unlock unit types in the shop, buy new formations, upgrade formation levels, or research unit technologies.
4. **Position and equip formations** — Move eligible formations within open deployment regions and fit owned equipment items onto formations.
5. **Use towers, blueprints, skills, and contraptions** — Activate Research Center blueprints, trigger one-round Energy Tower skills, strengthen towers, release commander skills, or place contraptions.
6. **Concede the match** — Surrender the match immediately without fighting the current round.

For the exact phase order, action allowances, upgrade and research conditions,
equipment slot rules, and skill cooldowns, see
**[A Round's Decisions](a-rounds-decisions.md)**. For board regions, landing,
collisions, and flank deployment, see
**[Deployment, Flanks, and Mobility](deployment-and-mobility.md)**.

## Supply and Action Constraints

Three constraints prevent either side from fielding or upgrading everything in a
single round:

- **Supply income and carryover:** On standard 1v1 versus maps, Round `1` grants
  `200` base supply, and each subsequent round increases the base grant by `200`
  up to a **per-round base income cap of `4000`**
  (`min(200 + (round - 1) × 200, 4000)`—this caps a single round's base income,
  not how much total supply you can hold). Unspent supply carries over intact to
  the next round without interest, while officers, fitted equipment, declining a
  reinforcement offer (`50` supply in an ordinary round, or the schedule's
  refund in a unit round), **Field Recovery**, and the Energy Tower loan
  **Rapid Resupply** (`+200` now, `-300` next round) modify your available
  supply. Standard 1v1 combat itself never adds supply to your ledger.
- **Per-round action allowances:** By default, each round allows at most **`2`
  unit purchases**, **`1` unit unlock**, and **`8` contraption releases** (see
  **[A Round's Decisions — Per-Round Action Allowances](a-rounds-decisions.md#per-round-action-allowances)**).
- **Experience conditions for upgrading:** A formation that has already fought
  and holds experience can be upgraded only when its experience bar is **full**,
  whereas a formation that arrived or leveled up in the current round (holding
  zero experience) can be upgraded immediately by paying supply (see
  **[A Round's Decisions — Upgrading a Formation's Level](a-rounds-decisions.md#upgrading-a-formations-level-upgrade_unit)**).

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

Although formations persist on the board, **existing formations (`legacy`) are
locked in place in later rounds** unless freed by **Deployment Module**, **Jump
Drive**, or **Redeployment**. See
**[Deployment, Flanks, and Mobility](deployment-and-mobility.md)** for the full
movement and flank arrival rules.

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
