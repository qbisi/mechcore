# Units & Battlefield Objects

> **Scope:** This guide covers the `33` purchasable unit formations, summoned
> combat entities, contraptions, towers, constructions, and terrain obstacles in
> **standard 1v1 matches** on the repository's pinned game version. For how
> surviving mechs damage the opponent's reactor core, see
> **[Reactor Core Settlement](reactor-damage.md)**. For how formations earn
> experience and scale with level, see
> **[Experience & Unit Levels](experience-and-levels.md)**.

Every object that appears on the battlefield belongs to a distinct category with
its own rules for deployment, equipment, experience, and post-fight settlement.

## Formation vs. Individual Mech

In Mechabellum, you make decisions at the **formation** level during deployment,
while combat and settlement operate on **individual mechs**:

- **Formation (Squad):** A single card on your deployment grid—such as a `24`-mech
  **Crawler** pack, a `5`-mech **Sledgehammer** platoon, or a `1`-mech
  **Fortress**. Buying, moving, rotating, upgrading levels, fitting equipment,
  and tracking experience all happen on the formation as a whole.
- **Individual Mech:** Each member inside a formation moves, acquires targets,
  takes shield and life damage, and dies independently during a fight. When the
  fight ends, **[Reactor Core Settlement](reactor-damage.md)** counts each
  individual standing mech—not whole formations—and **[Experience & Unit Levels](experience-and-levels.md)**
  awards kill experience for each individual enemy mech destroyed.
- **Round-to-Round Restoration:** As long as you do not sell a formation via
  **Field Recovery**, every mech that died during the fight returns at the start
  of the next round at full life and at the formation's current level.

## Unit Taxonomy & Classification Axes

Six properties define how any unit interacts with deployment, upgrades,
equipment, and combat:

### 1. Supply Tier & Shop Unlock Cost

Standard units span five supply tiers (`100`, `200`, `300`, `400`/`500`, and
`800` supply):

- **Starter / Zero-Unlock Tier (`0` unlock cost):** Eleven unit types—**Crawler**,
  **Fang**, **Hound**, **Void Eye**, **Marksman**, **Arclight**, **Vortex**
  (`100` supply) and **Mustang**, **Sledgehammer**, **Steel Ball**, **Fire
  Badger**, **Sabertooth**, **Tarantula** (`200` supply)—have an unlock cost of
  `0`. Once unlocked (either from your Round `0` opening or via the `0`-supply
  unlock action), they cost `100` or `200` supply per formation.
- **Mid-Tier Specialists (`50` or `100` unlock cost):** Units such as **Wasp**,
  **Stormcaller**, **Phoenix**, **Rhino**, **Phantom Ray** (`200` supply, `50`
  unlock), **Hacker** (`200` supply, `100` unlock), and the `300`-supply tier
  (**Wraith**, **Scorpion**, **Typhoon**, **Farseer**, **Centurion**, `50`
  unlock) require a small one-time shop unlock before purchase.
- **Giants (`200` unlock cost):** The `400`-supply (**Fortress**, **Vulcan**,
  **Melting Point**, **Sandworm**, **Raiden**) and `500`-supply (**Overlord**)
  heavy units require `200` supply to unlock (reduced by `200` if you hold
  **Giant Specialist**, making them free to unlock).
- **Titans (`350` unlock cost):** The three `800`-supply colossi (**War
  Factory**, **Abyss**, and **Mountain**) require `350` supply to unlock.

### 2. Upgrade Supply Cost (`50%` of Base Supply)

Upgrading a formation by one level always costs **half of its base purchase
supply** (`50` for `100`-supply units, `100` for `200`-supply units, `150` for
`300`-supply units, `200` for `400`-supply units, `250` for **Overlord**, and
`400` for `800`-supply units), before applying **Cost Control** (`-25%` upgrade
cost) or the **Efficient Maintenance** blueprint (`-50` upgrade cost on
**Fortress**, **Melting Point**, **Sandworm**, and **Overlord**).

### 3. Domain: Ground vs. Aerial

Every unit occupies either the `ground` or `air` domain:

- **Ground units (`26` purchasable types):** Move across the surface, interact
  with ground terrain (**Sand**, **Acid**, **Flame**, **Sticky Oil Bomb**), and
  can be targeted by ground-targeting weapons and **Hacker** control beams.
- **Aerial units (`7` purchasable types):** **Wasp**, **Phoenix**, **Phantom
  Ray**, **Wraith**, **Overlord**, **Raiden**, and **Abyss** fly in the `air`
  domain. Ground-only weapons (such as **Crawler**, **Rhino**, **Sledgehammer**,
  **Steel Ball**, **Stormcaller**, **Vulcan**, **Scorpion**, **Sandworm**, or
  **Sabertooth** prior to anti-air tech) cannot target them unless a technology
  grants an anti-air secondary weapon or mode.
- **Jump Drive Eligibility:** Only three aerial units—**Wasp**, **Overlord**,
  and **Phoenix**—have the **Jump Drive** technology, which also frees their
  `legacy` formations to reposition during deployment (see
  **[Deployment, Flanks, and Mobility](deployment-and-mobility.md#which-formations-can-move-move_unit)**).

### 4. Size Classes and the Titan Subgroup

Each unit has a size class (`small`, `medium`, or `large`) and equipment rule:

- **Small (`6` types):** **Crawler** (`24`), **Fang** (`18`), **Wasp** (`12`),
  **Mustang** (`12`), **Hound** (`5`), and **Void Eye** (`3`).
- **Medium (`17` types):** Multi-mech squads (**Sledgehammer** `5`, **Steel
  Ball** `4`, **Stormcaller** `4`, **Fire Badger** `3`, **Phantom Ray** `3`,
  **Phoenix** `2`, **Typhoon** `2`) and single-entity medium mechs (**Marksman**,
  **Arclight**, **Vortex**, **Rhino**, **Hacker**, **Sabertooth**, **Tarantula**,
  **Wraith**, **Scorpion**, **Farseer**).
- **Large (`10` types):** Single-entity heavy mechs (**Centurion**, **Fortress**,
  **Vulcan**, **Melting Point**, **Sandworm**, **Raiden**, **Overlord**, **War
  Factory**, **Abyss**, **Mountain**).
- **The Titan Equipment Exception (`wears_equipment: false`):** Exactly `30` of
  the `33` purchasable units can wear `1` equipment item (`2` with **Equipment
  Expansion**). The three `800`-supply units—**War Factory**, **Abyss**, and
  **Mountain**—can **never** equip items under any condition (see
  **[A Round's Decisions — Fitting Equipment](a-rounds-decisions.md#fitting-equipment-use_equipment)**).

### 5. Attack Delivery & Special Combat Roles

- **Melee (`crosses_shields: true`):** **Crawler**, **Rhino**, and **Sandworm**
  strike in melee and bypass dome energy shields by walking inside them.
- **Control (`base_damage: 0`):** **Hacker** deals no direct life damage with
  its primary beam; instead, it builds control progress on a ground target until
  the target turns and fights for the Hacker's side.
- **Ramp Beams:** **Melting Point** and **Steel Ball** channel continuous beams
  whose damage ramps up the longer they stay locked on the same target.

## Complete 33-Unit Master Roster (Level 1 Baseline)

All life, attack damage, and range figures below are extracted from `config/` at
**Level `1`** before officer, blueprint, equipment, or technology modifiers.
In the **Score (Full Squad)** column, the first number is the reactor core
damage dealt by a **single** standing mech (constant across Levels `1–9`), and
the parenthesized number is the reactor core damage if the **entire formation**
survives untouched.

| Unit | Supply | Unlock | Upgrade | Mechs | Footprint | Domain | Size | Lv 1 HP (per Mech) | Speed (`m/s`) | Lv 1 Attack / Cycle | Range | Primary Targets | Score (Full Squad) |
| --- | ---: | ---: | ---: | ---: | --- | --- | --- | ---: | ---: | --- | --- | --- | --- |
| **Crawler** | `100` | `0` | `50` | `24` | `50m × 20m` | Ground | Small | `250` | `16` | `79` / `0.6s` | `6m` | Ground (Melee) | `4` (`96`) |
| **Fang** | `100` | `0` | `50` | `18` | `50m × 20m` | Ground | Small | `117` | `6` | `63` / `1.5s` | `75m` | Ground & Air | `6` (`108`) |
| **Hound** | `100` | `0` | `50` | `5` | `40m × 20m` | Ground | Small | `879` | `10` | `246` / `2.4s` | `70m` | Ground, `6m` splash | `20` (`100`) |
| **Void Eye** | `100` | `0` | `50` | `3` | `40m × 20m` | Ground | Small | `1370` | `8` | `1144` / `3.3s` | `100m` | Ground | `33` (`99`) |
| **Arclight** | `100` | `0` | `50` | `1` | `20m × 20m` | Ground | Medium | `4813` | `7` | `365` / `0.9s` | `95m` | Ground, `7m` splash | `100` (`100`) |
| **Marksman** | `100` | `0` | `50` | `1` | `20m × 20m` | Ground | Medium | `1622` | `8` | `2329` / `3.1s` | `140m` | Ground & Air | `100` (`100`) |
| **Vortex** | `100` | `0` | `50` | `1` | `20m × 20m` | Ground | Medium | `8538` | `8` | `1570` / `1.8s` | `85m` | Ground | `100` (`100`) |
| **Mustang** | `200` | `0` | `100` | `12` | `50m × 20m` | Ground | Small | `343` | `16` | `36` / `0.4s` | `95m` | Ground & Air | `17` (`204`) |
| **Sledgehammer** | `200` | `0` | `100` | `5` | `50m × 20m` | Ground | Medium | `3478` | `7` | `608` / `4.5s` | `95m` | Ground, `5m` splash | `40` (`200`) |
| **Steel Ball** | `200` | `0` | `100` | `4` | `50m × 20m` | Ground | Medium | `4571` | `16` | `55` / `0.2s` | `45m` | Ground | `50` (`200`) |
| **Fire Badger** | `200` | `0` | `100` | `3` | `50m × 20m` | Ground | Medium | `4222` | `9` | `27` / `0.1s` | `75m` | Ground, `7m` splash | `67` (`201`) |
| **Sabertooth** | `200` | `0` | `100` | `1` | `30m × 30m` | Ground | Medium | `14142` | `8` | `6537` / `3.2s` | `95m` | Ground, `5m` splash | `200` (`200`) |
| **Tarantula** | `200` | `0` | `100` | `1` | `30m × 30m` | Ground | Medium | `14034` | `8` | `496` / `0.6s` | `80m` | Ground, `5m` splash | `200` (`200`) |
| **Wasp** | `200` | `50` | `100` | `12` | `50m × 20m` | Aerial | Small | `311` | `16` | `202` / `1.4s` | `50m` | Ground & Air | `17` (`204`) |
| **Stormcaller** | `200` | `50` | `100` | `4` | `50m × 20m` | Ground | Medium | `1149` | `6` | `772` / `6.6s` | `70–180m` | Ground, `5.5m` splash | `50` (`200`) |
| **Phantom Ray** | `200` | `50` | `100` | `3` | `50m × 20m` | Aerial | Medium | `3412` | `16` | `1087` / `3s` | `65m` | Ground & Air, `3m` splash | `67` (`201`) |
| **Phoenix** | `200` | `50` | `100` | `2` | `40m × 20m` | Aerial | Medium | `1491` | `16` | `2814` / `3.4s` | `120m` | Ground & Air | `100` (`200`) |
| **Rhino** | `200` | `50` | `100` | `1` | `30m × 30m` | Ground | Medium | `19297` | `16` | `3560` / `0.9s` | `6m` | Ground (Melee), `6m` splash | `200` (`200`) |
| **Hacker** | `200` | `100` | `100` | `1` | `30m × 30m` | Ground | Medium | `3249` | `8` | `600` / `0.3s` | `110m` | Ground | `200` (`200`) |
| **Typhoon** | `300` | `50` | `150` | `2` | `40m × 20m` | Ground | Medium | `9529` | `9` | `88` / `0.2s` | `100m` | Ground & Air, `5m` splash | `125` (`250`) |
| **Centurion** | `300` | `50` | `150` | `1` | `30m × 30m` | Ground | Medium | `16500` | `10` | `1395` / `0.8s` | `110m` | Ground & Air | `250` (`250`) |
| **Farseer** | `300` | `50` | `150` | `1` | `30m × 30m` | Ground | Medium | `11991` | `16` | `1348` / `2s` | `125m` | Ground & Air, `8m` splash | `250` (`250`) |
| **Scorpion** | `300` | `50` | `150` | `1` | `30m × 30m` | Ground | Medium | `18632` | `7` | `10650` / `4.5s` | `100m` | Ground, `15m` splash | `250` (`250`) |
| **Wraith** | `300` | `50` | `150` | `1` | `30m × 30m` | Aerial | Medium | `14115` | `10` | `381` / `1.6s` | `60m` | Ground & Air, `8m` splash | `250` (`250`) |
| **Fortress** | `400` | `200` | `200` | `1` | `40m × 40m` | Ground | Huge | `43938` | `6` | `6524` / `1.8s` | `100m` | Ground, `5m` splash | `350` (`350`) |
| **Melting Point** | `400` | `200` | `200` | `1` | `40m × 40m` | Ground | Huge | `30907` | `6` | `168` / `0.2s` | `115m` | Ground & Air, `3m` splash | `350` (`350`) |
| **Raiden** | `400` | `200` | `200` | `1` | `40m × 40m` | Aerial | Huge | `16065` | `10` | `5027` / `4.6s` | `110m` | Ground & Air | `350` (`350`) |
| **Sandworm** | `400` | `200` | `200` | `1` | `40m × 40m` | Ground | Huge | `48645` | `16` | `9726` / `2.5s` | `60m` | Ground (Melee), `12m` splash | `350` (`350`) |
| **Vulcan** | `400` | `200` | `200` | `1` | `40m × 40m` | Ground | Huge | `30279` | `6` | `75` / `0.1s` | `95m` | Ground, `15m` splash | `350` (`350`) |
| **Overlord** | `500` | `200` | `250` | `1` | `50m × 50m` | Aerial | Huge | `21339` | `10` | `4742` / `4.6s` | `120m` | Ground & Air, `7m` splash | `400` (`400`) |
| **Abyss** | `800` | `350` | `400` | `1` | `70m × 70m` | Aerial | Huge | `66955` | `10` | `3859` / `4s` | `100m` | Ground & Air | `500` (`500`) |
| **Mountain** | `800` | `350` | `400` | `1` | `70m × 70m` | Ground | Huge | `136657` | `6` | `5899` / `2s` | `100m` | Ground, `5m` splash | `500` (`500`) |
| **War Factory** | `800` | `350` | `400` | `1` | `70m × 70m` | Ground | Huge | `113593` | `6` | `7520` / `1.8s` | `100m` | Ground, `4.5m` splash | `500` (`500`) |

## Non-Standard Battlefield Objects

Beyond the `33` purchasable formations, four additional classes of objects
appear on the battlefield during deployment or combat:

| Category | Object | How It Enters the Board | Persists to Next Round? | Grants Combat EXP When Killed? | Deals Reactor Core Damage? |
| --- | --- | --- | --- | --- | --- |
| **Summoned Units** | Produced mechs (**War Factory** / **Overlord** / **Vulcan** / **Fortress** / **Farseer** / **Hound** / **Void Eye** spawns), **Larva** (`unit_id: 1001`), **Spider Mine** (`unit_id: 1002`), and commander-skill airdrops (**Crawler** / **Rhino** / **Wasp** / **Vulcan** drops) | Spawned mid-fight by technologies, skills, or tower effects (`is_support`) | **No** — vanishes at fight end | **Yes** — awards its table `loot_exp` (except self-destructing **Spider Mine** blows; see **[Experience & Unit Levels](experience-and-levels.md)**) | **Yes, at `25%` (`× 0.25`)** — rounded down per surviving mech (`Spider Mine` base score is `6`, `Larva` is `50`, `Tentacle` `101` is `0`) |
| **Contraptions** | **Sentry Missile**, **Shield Generator**, **Incendiary Bomb** | Purchased and placed during deployment (up to `8` per round) | **Only if unexploded / undestroyed** | **No** | **No** (`0` reactor score) |
| **Fixed Towers** | **Energy Tower** (left), **Research Center** (right) | Present on every standard 1v1 map from Round `1` | **Yes** — restored each round at current strengthen level (`0–4`) | **Yes (`100` EXP)** — plus inflicts the tower-destruction debuff on the owning side for that fight | **No** (`0` reactor score) |
| **Constructions** | **Defensive Wall**, **Anti-Armor Cannon**, **Rapid-Fire Cannon** | Dealt by the map in Round `0` or placed by commander skills | **Only if undestroyed** — destroyed segments/turrets do not regenerate | **Yes** — awards its construction row `exp` (`20` for **Defensive Wall**, `150` for cannons) | **No** (`0` reactor score) |
| **Terrain & Field Domes** | **Sand**, **Acid**, **Flame**, **Sticky Oil Bomb**, **Ionization**, **Shield Airdrop** domes | Map layout, weapon/bomb effects, or commander skills | **Shield Airdrop** persists if unbroken; **Sticky Oil Bomb** ticks down its round duration; combat fire/acid clears | **No** | **No** (`0` reactor score) |
