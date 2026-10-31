# A Round's Decisions

> **Scope:** This guide covers **standard 1v1 matches** (with no extra game
> rules enabled) on the repository's pinned game version. For board regions,
> unit landing, collision rules, movement locks, and flank travel, see
> **[Deployment, Flanks, and Mobility](deployment-and-mobility.md)**. For how
> combat experience fills a formation's bar and what levels scale, see
> **[Experience & Unit Levels](experience-and-levels.md)**. For the `33` unit
> types and their costs, see **[Units & Battlefield Objects](units.md)**.

Every player decision in Mechabellum happens before combat begins—either in
Round `0` (the opening) or during each round's deployment phase. Both sides make
their deployment decisions secretly and simultaneously: neither player sees the
opponent's new purchases, repositioned formations, or aimed commander skills
until the fight starts.

## Phase Order Within a Round

Not every action can be taken at any moment. The game enforces a strict sequence
at the start of a match and at the start of each round:

1. **Round `0` (Opening):** Each side is privately dealt four opening offers and
   takes exactly one `choose_advance_team` decision. No other action exists in
   Round `0`.
2. **Round `1` Deployment:** The squads from your chosen opening land on the
   board, their unit types unlock in your shop, and your Round `1` supply
   arrives. Round `1` deals **no reinforcement card offer**, so you immediately
   proceed to shop, board, tower, and skill actions.
3. **Round `2+` Deployment:**
   - **Step 1 — Answer the reinforcement offer (`choose_reinforce_item`):** Both
     sides are shown the same four reinforcement cards. Unless you concede the
     match, your **first decision of the round** must be either taking one of
     the four cards or declining the offer for supply. Every other deployment
     action is refused until this choice is made.
   - **Step 2 — Open deployment actions:** Once you have answered the
     reinforcement offer, you may unlock and buy units, upgrade levels and
     technologies, fit equipment, move eligible formations, strengthen towers,
     activate blueprints or Energy Tower skills, place contraptions, and release
     commander skills in any order their conditions allow.
   - **Step 3 — Finish deployment:** Ending deployment closes your turn for the
     round; it carries no cost or condition and is not counted among the `14`
     tactical decision types below.

## The 14 Player Decisions at a Glance

| Goal | Action Type | What It Does | Immediate Cost |
| --- | --- | --- | --- |
| **1. Opening** | `choose_advance_team` | Pick 1 of 4 private `(Advance Team + Specialist)` combinations in Round `0` | `0` supply (adjusts starting reactor core) |
| **2. Reinforcement** | `choose_reinforce_item` | Pick 1 of 4 shared reinforcement cards in Round `2+`, or choose `refund_<amount>` | Card price (or **grants** `50`+ supply on decline) |
| **3. Army Expansion & Upgrades** | `unlock_unit` | Unlock a locked unit type in your shop so it can be bought | Unit unlock price + `1` unlock allowance |
| | `buy_unit` | Buy one formation of an unlocked unit type and place it on the board | Unit price (+ level surcharges) + `1` buy allowance |
| | `upgrade_unit` | Raise one of your formations by `1` level (up to level `9`) | Unit upgrade price (modified by officers/equipment) |
| | `upgrade_technology` | Research one technology from a unit type's 4-tech match loadout | Tech price + `200` per tech already on that unit |
| **4. Positioning & Equipment** | `move_unit` | Reposition and optionally rotate an eligible formation on the board | `0` supply (requires movement eligibility) |
| | `use_equipment` | Fit an owned equipment item from stock onto a formation with a free slot | `0` supply (paid when the card was taken) |
| **5. Towers, Blueprints, Skills & Contraptions** | `active_blueprint` | Activate a Research Center blueprint (grants a commander skill or an enhancement officer) | Blueprint supply price (`100`–`300`) |
| | `active_energy_tower_skill` | Activate one Energy Tower skill for the current round | Skill price (`0`–`100`, or `+200` / `-300` loan) |
| | `strengthen_tower` | Raise your Energy Tower or Research Center by `1` level (up to level `4`) | Level price (`100` / `150` / `250` / `300`) |
| | `release_commander_skill` | Trigger a deployment skill on a unit/construction, or aim a combat skill at an area/target | `0` supply (**Field Recovery** refunds supply) |
| | `release_contraption` | Buy and place a **Shield Generator**, **Sentry Missile**, or **Missile Interceptor** | Contraption price (`50`–`100`) + `1` contraption allowance |
| **6. Surrender** | `concede` | Surrender the match immediately without fighting the current round | Ends the match |

## Per-Round Action Allowances

Even when you hold plenty of supply, each round restricts how many shop
expansions and devices you can deploy. These allowances reset at the start of
every round:

| Allowance | Standard Per-Round Limit | What Can Change It in Standard 1v1 |
| --- | ---: | --- |
| **Unit Purchases (`buy_unit`)** | `2` | • **Mass Recruitment** (Energy Tower skill, costs `50` supply): adds `+1` purchase for the current round only.<br>• **Additional Deployment Slot** (reinforcement officer card): adds `+1` purchase to the current round and every later round. |
| **Unit Unlocks (`unlock_unit`)** | `1` | Nothing in standard 1v1 modifies this limit. (Unit types unlocked automatically by your Round `0` opening, an officer's delivery schedule, or taking a unit reinforcement card do **not** spend your unlock allowance.) |
| **Contraption Releases (`release_contraption`)** | `8` | Nothing in standard 1v1 modifies this limit. All three contraption types share the same `8`-per-round pool. |

> **Scope check on unexercised limits:** In standard 1v1 (with no extra game
> rules), there is **no** cap on how many formations of the same unit type you
> may own on the board, and **no** per-unit-type purchase limit within a round
> beyond your total purchase allowance.

## Expanding and Upgrading Your Army

### Unlocking and Buying Units (`unlock_unit`, `buy_unit`)

- **Unlock before buying:** You can only buy a unit type that is currently in
  your `unlocked_units` list. Attempting to unlock a unit type that is already
  unlocked, or buy one that is still locked, is refused.
- **Buying above Level `1`:** Formations bought from the shop arrive at level
  `1` by default. If an officer (such as **Elite Specialist**) or the Energy
  Tower skill **Elite Recruitment** raises the shop level of that unit to level
  `L`, buying the formation costs its base purchase price **plus `L - 1`
  upgrades** at the unit's upgrade price.
- **Board room required:** Buying a unit requires space on your main deployment
  region and a valid final placement (see
  **[Deployment, Flanks, and Mobility](deployment-and-mobility.md)**).

### Upgrading a Formation's Level (`upgrade_unit`)

Raising a formation's level multiplies its mechs' base life and base attack
damage (`n` times base life and base damage at level `n`, up to level `9`), and
resets its experience bar to `0`.

Paying the upgrade supply price is **not** sufficient on its own to upgrade
every formation:

- **Formations holding experience require a full bar:** Once a formation has
  fought and earned experience (`current/maximum`), the game refuses
  `upgrade_unit` unless `current` has reached `maximum`—either from combat kills
  and assists or from targeting the formation with the commander skill
  **Intensive Training**.
- **Formations with zero experience since arriving or leveling:** A formation
  that has earned no experience since it joined the board or last leveled up—which
  includes **every formation that arrived or leveled up in the current
  round**—can be upgraded immediately by paying its upgrade supply price, with
  no experience bar requirement.
- **Upgrade price discounts:** Officers and fitted equipment (such as
  **Enhancement Module**, which subtracts `100` supply from its formation's
  upgrade price) modify the upgrade cost. Multiple discounts sum together and
  floor at `0` (an upgrade is never negative).

### Researching Unit Technologies (`upgrade_technology`)

Each unit type has a match-specific `tech_loadout` of four technologies chosen
before the match starts.

- **Loadout and uniqueness:** You can only research a technology that belongs to
  your side's `tech_loadout` for that unit and is not already researched.
  Standard 1v1 loadouts have no prerequisite chains between technologies.
- **Unlocking the unit is not required:** The legality check for
  `upgrade_technology` **does not require the unit type to be unlocked in your
  shop**. You can research a technology for a unit you hold from an opening or
  card—or even one you have not unlocked yet.
- **Escalating price per unit (`+200` step):** Every technology has its own base
  supply price, and each technology **already active on that same unit type**
  adds **`200` supply** to the cost of the next technology for that unit:

```text
technology_cost = base_tech_supply + 200 × (technologies_already_active_on_this_unit) + officer_modifiers
```

  Researching technologies on one unit type does not increase the technology
  prices of other unit types.

## Fitting Equipment (`use_equipment`)

When you take an equipment card from a reinforcement offer (or receive one from
an officer such as **Secondary Equipment Expert**), the item enters your
unfitted `equipment` stock.

- **Fitting is free:** Assigning an item from stock to a formation (`use_equipment`)
  costs `0` supply because the card was already paid for when taken.
- **Units that cannot wear equipment:** Across the standard 1v1 roster, three
  unit types have `wears_equipment: false` and **cannot wear any equipment**:
  **War Factory**, **Abyss**, and **Mountain**. Every other standard unit type
  can wear equipment.
- **Equipment slots (`1` default, `2` with Equipment Expansion):** Every
  eligible formation has **`1` equipment slot** by default. Holding the officer
  **Equipment Expansion** adds `+1` slot to every eligible formation on your
  side, allowing each formation to wear **`2` items**.
- **Duplicate items are allowed and stack:** The game checks that the item is in
  your unfitted stock and that the target formation has a free slot; it **does
  not forbid fitting two copies of the same item** onto one formation. For
  example, a formation wearing two **Enhancement Module** items under
  **Equipment Expansion** receives `-200` on its upgrade supply cost (making a
  `200`-supply **Fortress** upgrade cost `0`).

## Towers, Blueprints, Energy Tower Skills, and Contraptions

### Strengthening Towers (`strengthen_tower`)

Each side has two fixed towers on the battlefield: the **Energy Tower** and the
**Research Center**. Each tower starts at level `0` and can be strengthened independently up to level `4`:

| Strengthen Level | Supply Cost | Effect |
| ---: | ---: | --- |
| Level `1` | `100` | Increases the tower's life and changes the duration of the debuff inflicted if that tower is destroyed |
| Level `2` | `150` | Further increases tower life and loss-debuff duration |
| Level `3` | `250` | Further increases tower life and loss-debuff duration |
| Level `4` | `300` | Maximum tower life and loss-debuff duration |

### Activating Research Center Blueprints (`active_blueprint`)

In a standard 1v1 match, the Research Center offers five initial blueprints and
two level-II chain follow-ups, all of which activate immediately upon paying
their supply price:

| Blueprint | Supply Cost | What Activating It Grants |
| --- | ---: | --- |
| **Sticky Oil Bomb** | `150` | Adds the **Sticky Oil Bomb** commander skill to your skill panel |
| **Field Recovery** | `100` | Adds the **Field Recovery** commander skill to your skill panel |
| **Mobile Beacon** | `100` | Adds the **Mobile Beacon** commander skill to your skill panel |
| **Attack Enhancement** | `100` | Grants the **Attack Enhancement** side-wide officer and unlocks **Attack Enhancement II** |
| **Attack Enhancement II** | `300` | Requires **Attack Enhancement**; replaces it with the **Attack Enhancement II** officer |
| **Defense Enhancement** | `100` | Grants the **Defense Enhancement** side-wide officer and unlocks **Defense Enhancement II** |
| **Defense Enhancement II** | `300` | Requires **Defense Enhancement**; replaces it with the **Defense Enhancement II** officer |

### Activating Energy Tower Skills (`active_energy_tower_skill`)

Every Energy Tower skill lasts for the **current round only** and can be
activated at most once per round:

| Energy Tower Skill | Supply Cost | Immediate & Next-Round Effect |
| --- | ---: | --- |
| **Rapid Resupply** | `0` | Grants **`+200` supply immediately** this round, and deducts **`300` supply** from your next round's income |
| **Mass Recruitment** | `50` | Adds **`+1` unit purchase allowance** for the current round |
| **Elite Recruitment** | `100` | Raises the level of units bought from the shop for the rest of this round by **`+1`** (each purchase also pays that extra level's upgrade price) |
| **Enhanced Range** | `100` | Adds **`+15m` attack range** to all ranged units on your side for this round's fight (does not affect melee units or defensive turrets) |
| **High Mobility** | `50` | Adds **`+3` movement speed** to all units on your side for this round's fight (including summoned units; does not affect constructions) |

### Buying and Placing Contraptions (`release_contraption`)

Contraptions are bought directly during deployment (up to `8` total per round)
and persist into later rounds unless destroyed or triggered in combat:

| Contraption | Supply Cost | Combat & Cross-Round Behavior |
| --- | ---: | --- |
| **Shield Generator** (`shield`) | `100` | Projects a stationary dome shield from the start of the fight; persists into the next round if its energy is not depleted (raised by `+40%` with **Advanced Shield Device**) |
| **Sentry Missile** (`missile`) | `50` | Launches a single high-damage splashing missile that slows surviving targets when a non-hidden enemy enters trigger range; vanishes after firing, or persists into the next round if never triggered (damage raised by `+200%` with **Advanced Missile Device**) |
| **Missile Interceptor** (`interceptor`) | `100` | A ground building of your side that shoots down eligible enemy projectiles (including Sentry Missiles) in flight and blocks unit movement for both sides; persists into the next round unless destroyed during the fight |

## Releasing Commander Skills (`release_commander_skill`)

Commander skills (called **Battlefield Powers** in the game UI) sit in numbered
slots on your skill panel, added by reinforcement cards, blueprints, or
officers.

### Cooldowns and Panel Slots

- **Initial cooldown (`initial_cooldown`):** Most skills join your panel at `0`
  cooldown and can be used immediately in the round they arrive, but some
  high-impact skills (such as **Nuke**, **Lightning Storm**, and **Ion Blast**)
  join with an `initial_cooldown` of `1` round.
- **Per-round cooldown (`cooldown`):** When a round opens, any slot you spent in
  the previous round resets to its `cooldown` value, while every unspent slot on
  cooldown decreases by `1` down to `0`. Skills with `cooldown: 0` (such as
  **Field Recovery** and **Mobile Beacon**) can be used every single round.

### Deployment-Time Skills vs. Combat Releases

Commander skills divide into two groups by when their effect happens:

1. **Deployment-time skills (take effect immediately on the board before combat):**
   - **Intensive Training:** Targets one of your own formations that is below
     level `9` and whose experience bar is not already full; immediately fills
     its experience bar (`maximum/maximum`) so it can be upgraded this round.
   - **Redeployment:** Targets one of your own formations and unlocks its
     movement for the rest of the current deployment phase (see
     **[Deployment, Flanks, and Mobility](deployment-and-mobility.md)**).
   - **Field Recovery:** Targets one of your own formations or constructions,
     removes it from the board, and refunds supply:
     - **Recovering a formation** refunds the purchase price you originally paid
       for it (including any officer discount at the time of purchase) plus `1`
       upgrade price (at current officer rates) for each level above level `1`.
       Any equipment the formation was wearing returns immediately to your
       unfitted `equipment` stock and can be fitted onto another formation in the
       same round.
     - **Recovering a construction** pays a fixed refund by type: **`50` supply**
       for a **Defensive Wall**, and **`100` supply** for an **Anti-Armor
       Cannon** or **Rapid-Fire Cannon**.
2. **Combat-triggered skills (aimed during deployment, executed during the fight):**
   - Area strikes (**Missile Strike**, **Heavy Missile Strike**, **Lightning
     Storm**, **Incendiary Bomb**, **Acid Blast**, **Smoke Bomb**, **Ion
     Blast**, **Nuke**, **EM Blast**, **Photon Emission**), persistent field
     objects (**Shield Airdrop**, **Sticky Oil Bomb**), unit drops (**Rhino
     Drop**, **Wasp Swarm**, **Vulcan's Descent**, **Farseer Airdrop**,
     **Sandworm Strike**, **Sabertooth Airdrop**, **Overlord Artillery**),
     turret placements, and **Mobile Beacon** paths are all aimed during
     deployment and execute automatically once the fight starts.
   - A skill aimed at a unit or construction is refused if its slot is on
     cooldown or already used this round.
