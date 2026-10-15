# Master Concept Glossary

> **Scope:** This glossary defines the canonical terms used across the Mechcore
> Wiki for **standard 1v1 matches** on the repository's pinned game version,
> linking each concept to its governing guide.

Use this page as a quick dictionary when reading mechanics guides or looking up
how two systems interact.

## Match Structure & Settlement

- **Match vs. Round vs. Fight:** A **match** is the complete contest between
  `blue` and `red` until one side's reactor core reaches `0` or below (or a side
  concedes). A **round** combines one secret pre-battle deployment phase and one
  automated **fight** (see **[How a Match Works](how-a-match-works.md)**).
- **Reactor Core:** Each side's off-board match life total (`4500` base on
  standard 1v1 maps before Round `0` opening modifiers). It is not a physical
  structure on the battlefield and never regenerates after Round `0` (see
  **[Reactor Core Settlement](reactor-damage.md)**).
- **Settlement Score:** The reactor core damage a single standing mech deals to
  the opponent when a fight ends. Across all standard units, a mech's score is
  constant across levels `1` through `9` and is multiplied by `0.25` for each
  applicable penalty category (**summoned**, **reborn**, or **turned**), rounded
  down per mech (see **[Reactor Core Settlement](reactor-damage.md)**).

## Deployment, Regions & Mobility

- **Main Region vs. FlankRegions:** Your **main region** is your primary
  deployment rectangle (`300m × 140m`, inset to `280m × 120m` in Round `1` when
  flanks are closed). The two **flank regions** (`40m × 200m` strips along the
  left and right sides of the opponent's half) open from Round `2` onward (see
  **[Deployment, Flanks, and Mobility](deployment-and-mobility.md#deployment-regions-and-when-they-open)**).
- **Arrival-Round vs. Legacy Formation (`legacy`):** A formation deployed in the
  current round can be dragged and rotated freely before combat starts. Once a
  formation survives into a subsequent round, it becomes a **legacy** formation
  locked to its coordinates unless unlocked by **Deployment Module**, **Jump
  Drive** (on **Wasp**, **Overlord**, or **Phoenix**), or **Redeployment** (see
  **[Deployment, Flanks, and Mobility](deployment-and-mobility.md#which-formations-can-move-move_unit)**).
- **Super Deployment (`travelling`):** Any formation that starts a fight inside
  an enemy flank region enters combat in an `8`-second arrival state (`4`
  seconds with **Quick Teleport**). It lands at `40%` of its maximum life,
  heals in `8` per-second ticks back to `100%`, cannot move, target, fire, or
  advance production/self-repair/shield timers while arriving, and activates its
  shields and combat loops only when arrival finishes (see
  **[Deployment, Flanks, and Mobility](deployment-and-mobility.md#flank-deployment-and-travelling-super-deployment)**).
- **Footprint & Rotation:** The rectangular meter area (`width × depth`) a
  formation or contraption occupies on the deployment grid. Rotating a
  non-square formation by `90°` swaps its width and depth and triggers an
  automatic `10`-meter grid search if the rotated footprint overlaps an obstacle
  (see **[Deployment, Flanks, and Mobility](deployment-and-mobility.md#footprints-rotation-and-collisions)**).

## Units, Formations & Battlefield Objects

- **Formation vs. Individual Mech:** A **formation** is the squad card you buy,
  move, upgrade, and equip on the deployment board (`1` to `24` members). An
  **individual mech** is a single combat actor inside that formation (`24` in a
  **Crawler** squad, `18` in **Fang**, `5` in **Sledgehammer**, `1` in
  single-entity squads). Experience and upgrades belong to the formation;
  targeting, life, shields, and reactor core settlement count individual mechs
  (see **[Units & Battlefield Objects](units.md#formation-vs-individual-mech)**).
- **Domain (Ground vs. Aerial):** Whether a unit moves on the ground (`ground`)
  or flies (`air`). Weapons specify whether they can target ground units, aerial
  units, or both (see **[Units & Battlefield Objects](units.md#3-domain-ground-vs-aerial)**).
- **Unit Size & Titan Exception:** Every unit is classified as `small`,
  `medium`, or `large`. The three `800`-supply colossi (**War Factory**,
  **Abyss**, and **Mountain**) form a distinct subgroup that cannot wear
  equipment (`wears_equipment: false`) and costs `350` supply to unlock (see
  **[Units & Battlefield Objects](units.md#4-size-classes-and-the-titan-subgroup)**).
- **Summoned Unit (`is_support`):** Any combat unit created mid-fight by a
  production technology, death spawn, commander skill airdrop, or tower/officer
  effect rather than deployed from a persistent formation. Summoned units
  vanish when the round ends, never earn or share combat experience, and deal
  `25%` (`× 0.25`) of their normal reactor core score if standing at fight end
  (see **[Units & Battlefield Objects](units.md#non-standard-battlefield-objects)**).
- **Contraptions:** Purchasable single-box deployment objects (**Sentry
  Missile**, **Shield Generator**, and **Incendiary Bomb** from the **Flame
  Weapon** blueprint) placed up to `8` times per round. Notably, **Sentry
  Missile** and **Shield Generator** are exempt from unit placement collision
  checks (see **[A Round's Decisions](a-rounds-decisions.md#buying-and-placing-contraptions-release_contraption)**).
- **Towers & Constructions:** Each side owns two fixed battlefield towers—the
  **Energy Tower** (left) and **Research Center** (right)—plus any map-dealt or
  skill-placed **constructions** (**Defensive Wall**, **Anti-Armor Cannon**,
  **Rapid-Fire Cannon**). Destroying an enemy tower applies a combat debuff to
  that enemy team for the rest of the fight and grants `100` base experience,
  but deals `0` reactor core damage (see **[Units & Battlefield Objects](units.md#non-standard-battlefield-objects)**).

## Experience, Levels & Modifiers

- **Experience Bar (`current / maximum`) & Full-Bar State (`IsExpMax`):** Each
  formation tracks experience toward its next level (`1` to `9`). Once a
  formation's bar reaches its threshold (or within fixed-point tolerance), it
  enters the full-bar state, stops taking any further combat experience shares,
  and becomes eligible to upgrade even after surviving prior rounds (see
  **[Experience & Unit Levels](experience-and-levels.md#experience-bars-and-the-full-bar-state)**).
- **Kill Bounty vs. Shared Experience Pool:** Killing an enemy unit awards its
  `loot_exp` **twice**: once in full (`100%`) to the killer's formation, and a
  second equal pool (`100%`) split evenly across every eligible formation that
  dealt at least `1` life damage to that victim plus every eligible allied
  formation with a unit within `65` meters (see
  **[Experience & Unit Levels](experience-and-levels.md#how-combat-kills-distribute-experience)**).
- **Level Stat Scaling (`× n`):** A unit at level `n` (`1` to `9`) has `n` times
  its level-1 maximum life, `n` times its level-1 attack damage, and hands out
  `(2n - 1)` times its level-1 kill experience when destroyed, while its
  movement speed, attack range, and reactor core settlement score stay unchanged
  (see **[Experience & Unit Levels](experience-and-levels.md#what-unit-levels-scale-and-what-stays-fixed)**).

## Economy, Reinforcements & Tech

- **Supply & Per-Round Base Income Cap:** The currency spent during deployment.
  Standard 1v1 base supply starts at `200` in Round `1` and grows by `+200` per
  round up to `4000` per round (`min(200 + (round - 1) × 200, 4000)`), with
  unspent supply carrying over intact (see
  **[How a Match Works](how-a-match-works.md#supply-and-action-constraints)**).
- **Allowances (`2` Buys / `1` Unlock / `8` Contraptions):** Per-round action
  counters that reset each round before officer and blueprint modifiers are
  applied (see **[A Round's Decisions](a-rounds-decisions.md#per-round-action-allowances)**).
- **Unit Technology Step Cost (`+200`):** Each unit type has `4` technology
  slots. Every technology you research on a unit type increases the cost of all
  remaining unresearched technologies on that same unit type by `+200` supply
  (and does not require the unit to be unlocked in your shop first; see
  **[A Round's Decisions](a-rounds-decisions.md#researching-unit-technologies-upgrade_technology)**).
