# Mechcore Wiki

A player- and agent-facing guide to **Mechabellum**, grounded in the native game
build and extracted configuration tables of this repository.

Every article explains observable game behavior, the consequences of player
decisions, and the exceptions that matter in play, within an explicit scope.

## Master Concept Portal

Like community strategy wikis, Mechcore Wiki is organized around **two
complementary entryways**: narrative guides that walk through a round step by
step, and a **6-Domain Master Concept Index** for jumping directly to any rule
or entity table:

| Domain | Core Concepts & Questions Answered | Canonical Guides |
| --- | --- | --- |
| **1. Match Flow & Victory** | How a 1v1 match is won, Round `0` opening vs. Round `1` vs. Round `2+` sequence, reactor core (`4500` base) vs. battlefield towers, and what persists vs. resets between rounds | **[How a Match Works](how-a-match-works.md)** · **[Master Concept Glossary](glossary.md)** |
| **2. Pre-Battle Decisions & Economy** | Supply formula (`200 + 200 × (r - 1)` up to `4000`/round), the `14` action types, allowances (`2` buys, `1` unlock, `8` contraptions), `+200` tech step cost, equipment slot limits, towers, blueprints, and commander skills | **[A Round's Decisions](a-rounds-decisions.md)** |
| **3. Board Space, Flanks & Mobility** | Main (`300m × 140m`) and flank (`40m × 200m`) regions, `10m`-grid automatic spiral landing, collision exclusions, `legacy` movement locks and their `3` unlocks, and the `8s` flank `travelling` state | **[Deployment, Flanks, and Mobility](deployment-and-mobility.md)** |
| **4. Units & Battlefield Objects** | Formation vs. individual mech, supply tiers and unlock costs, ground vs. aerial domain, size classes (`small` / `medium` / `large`), the `3` Titan equipment exceptions, summoned units, contraptions, towers, and constructions | **[Units & Battlefield Objects](units.md)** |
| **5. Experience & Unit Levels** | Level `1–9` stat scaling (`n×` HP and attack, `(2n - 1)×` kill bounty, unchanged speed/range/score), `upgrade_exp` thresholds, the `100%` killer + `100%` `65m`/damage assist split, and officer/tech EXP rates | **[Experience & Unit Levels](experience-and-levels.md)** |
| **6. Reactor Core Settlement** | Active survivor filtering, why Level `1–9` scores are constant in the pinned build, the three `× 0.25` penalties (**summoned**, **reborn**, **turned**) with per-mech floor rounding, and simultaneous timeout damage | **[Reactor Core Settlement](reactor-damage.md)** |

## Reading Path for New Players & Agents

1. **[How a Match Works](how-a-match-works.md)** — Trace a standard 1v1 match
   backward from reactor core victory and combat settlement to deployment
   choices, supply, and round-to-round persistence.
2. **[A Round's Decisions](a-rounds-decisions.md)** — Learn the exact phase
   order, per-round allowances, upgrade and technology rules, equipment slots,
   and commander skill cooldowns.
3. **[Deployment, Flanks, and Mobility](deployment-and-mobility.md)** — Master
   where units can be placed, why carried-over (`legacy`) formations lock in
   place, and how flank Super Deployment (`travelling`) works.
4. **[Units & Battlefield Objects](units.md)** — Compare all `33` purchasable
   units alongside summoned spawns, contraptions, towers, and constructions.
5. **[Experience & Unit Levels](experience-and-levels.md)** — Understand how
   kills distribute experience in combat and how leveling scales your army.
6. **[Reactor Core Settlement](reactor-damage.md)** — Check exact per-mech
   reactor scores and penalty multipliers at the end of a fight.
7. **[Master Concept Glossary](glossary.md)** — Look up any canonical term and
   jump straight to its governing rule.

## Scope and Authority

- **Native game authority:** Claims describe the native game on the repository's
  pinned game version, not simulator implementation status.
- **Standard 1v1 baseline:** Unless a page states otherwise, rules and numbers
  apply to standard 1v1 versus matches with no extra game rules enabled.
- **Verified values:** Numerical figures trace to the repository's extracted
  configuration tables rather than unverified community estimates.
