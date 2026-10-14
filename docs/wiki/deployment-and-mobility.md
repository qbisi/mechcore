# Deployment, Flanks, and Mobility

> **Scope:** This guide covers **standard 1v1 matches** (with no extra game
> rules enabled) on the repository's pinned game version. For the full list of
> deployment actions, shop allowances, and skill cooldowns, see
> **[A Round's Decisions](a-rounds-decisions.md)**.

Where your formations stand—and when they are allowed to move or arrive—shapes
every fight in Mechabellum. Because formations persist across rounds, placing a
formation is usually a multi-round commitment unless you have a way to unlock
its movement.

## Deployment Regions and When They Open

Each side (`blue` and `red`) has three deployment rectangles on the battlefield,
mirrored across the center line:

1. **Main Deployment Region (open from Round `1`):** Your side's primary half of
   the board, containing your **Energy Tower**, **Research Center**, and initial
   defensive constructions. In Round `1`, all purchases and moves must stay
   inside your main region.
2. **Left and Right Flank Regions (open from Round `2`):** Two narrow rectangles
   running along the left and right outer edges of the **opponent's** half of
   the board, facing inward toward the center line. Starting in Round `2`, you
   can buy formations directly into a flank or move eligible formations there.

```text
+-------------------------------------------------------------------+
|                           RED BACKLINE                            |
|  +---------+   +-----------------------------------+   +---------+|
|  |  Blue   |   |                                   |   |  Blue   ||
|  |  Flank  |   |     RED MAIN DEPLOYMENT REGION    |   |  Flank  ||
|  | (Rd 2+) |   |     (Towers & Constructions)      |   | (Rd 2+) ||
|  +---------+   +-----------------------------------+   +---------+|
| - - - - - - - - - - - - - CENTER LINE - - - - - - - - - - - - - - |
|  +---------+   +-----------------------------------+   +---------+|
|  |  Red    |   |                                   |   |  Red    ||
|  |  Flank  |   |     BLUE MAIN DEPLOYMENT REGION   |   |  Flank  ||
|  | (Rd 2+) |   |     (Towers & Constructions)      |   | (Rd 2+) ||
|  +---------+   +-----------------------------------+   +---------+|
|                           BLUE BACKLINE                           |
+-------------------------------------------------------------------+
```

### Where Contraptions Can Be Placed

Contraptions (`release_contraption`) can only be placed inside your **main
deployment region**, never in a flank:

- **Missile Interceptor** and **Sentry Missile** can be placed anywhere inside
  the full bounds of your main deployment region (with the interceptor's
  footprint required to fit without overlapping obstacles).
- **Shield Generator** is restricted to an inset area inside your main
  deployment region, drawn inward from each edge by the contraption's margin.

## Automatic Landing on the 10-Meter Grid

Several events put new formations onto your board without a manual position
chosen by the player:

- The starting force from your Round `0` **advance team** (delivered as Round
  `1` opens)
- Squads granted by taking a **unit reinforcement card** in Round `2+`
- Squads delivered at the start of a round by a **specialist** or **officer**
  (such as **Marksman Specialist** delivering a Rank `3` Marksman on Round `2`,
  or **Rhino Specialist** delivering a Rank `2` Rhino on Round `4`)

When the game lands an unplaced formation, it always places it in your **main
deployment region** on the `10`-meter grid:

1. **Preferred center slot:** The game aligns the formation's footprint around
   the center of your main deployment region (`x = 0, y = -160` in side-local
   coordinates on standard versus maps), snapped to the `10`-meter grid (an odd
   footprint width shifts the center alignment by `5` meters).
2. **Nearest free grid cell:** If that center position is occupied, the game
   searches every `10`-meter grid position in the main region and places the
   formation at the valid free position **strictly closest** to the preferred
   center slot. When multiple squads arrive at once, they land one after
   another, each avoiding the ones already placed.

Because a newly arrived formation is free to move in the round it arrives, you
can immediately drag it from its landing spot to any valid position in an open
region during that round's deployment phase.

## Footprints, Rotation, and Collisions

Every unit formation, construction, tower, and **Missile Interceptor** occupies
a rectangular footprint on the deployment grid:

- **Rotation swaps width and height:** Rotating a formation (`rotated: true`)
  turns its rectangular footprint by `90` degrees, exchanging its width and
  depth. Moving a formation into a flank also turns its default facing toward
  the center of the map, so rotating in a flank swaps width and depth relative
  to the flank's inward orientation.
- **What blocks placement:** A formation's footprint must lie completely inside
  the target deployment region (`main`, `left flank`, or `right flank`) and
  cannot overlap with positive area against any of your side's:
  - Other unit formations
  - Defensive constructions (**Defensive Wall**, **Anti-Armor Cannon**,
    **Rapid-Fire Cannon**)
  - Fixed towers (**Energy Tower** and **Research Center**, in the main region)
  - **Missile Interceptors**
- **Touching edges are legal:** Two footprints whose edges touch without
  overlapping interior area are allowed.
- **What does not block placement:** **Shield Generator** and **Sentry Missile**
  contraptions take no part in deployment footprint collisions—you can place a
  unit directly over a Sentry Missile or Shield Generator center, or place
  those contraptions under existing formations.
- **Swapping two formations:** Moving two or more eligible formations together
  in a batch checks their final positions against each other rather than against
  their starting footprints, allowing two formations to trade places even in a
  crowded region.

## Which Formations Can Move (`move_unit`)

You cannot freely rearrange your entire army every round. Whether a formation is
allowed to move during deployment depends on when it arrived and what it
carries:

| Formation State | How It Reached the Board | Can It Move This Round? |
| --- | --- | --- |
| **Newly arrived (`joined` or `delivered`)** | Bought from the shop this round, granted by a reinforcement card this round, or delivered as this round opened (Round `1` advance team or scheduled officer delivery) | **Yes** — free to move anywhere in open regions for the entire deployment phase |
| **Carried over (`legacy`)** | Survived from any previous round | **No** — locked at its previous round's position unless freed by one of the 3 unlocks below |

### The 3 Ways to Move a Carried-Over (`legacy`) Formation

Only three mechanisms in a standard 1v1 match unlock a `legacy` formation's
movement:

1. **Deployment Module (Equipment):** Fitting **Deployment Module** onto a
   formation frees **that formation** to move in the current round and in every
   subsequent round while worn.
2. **Jump Drive (Unit Technology):** Researching **Jump Drive**—available only on
   **Wasp**, **Overlord**, and **Phoenix**—frees **every formation of that unit
   type** on your side to move in the current round and in every subsequent
   round.
3. **Redeployment (Commander Skill):** Releasing **Redeployment** on a formation
   frees **that single formation** to move for the rest of the current round's
   deployment phase. At the start of the next round, it returns to being locked
   in place unless freed again.

## Flank Deployment and Travelling (Super Deployment)

Deploying units into the enemy flanks (`left flank` or `right flank`, open from
Round `2` onward) lets you attack from the sides or rear, but newly flanked
formations do not start the fight at full readiness. Instead, they enter combat
in the **travelling** (`Super Deployment`) state.

### When a Formation Travels vs. When It Stays Settled

Whether a formation on a flank triggers the `travelling` arrival sequence
depends on whether it **entered that flank region this round**:

- **Enters `travelling` this round:**
  - A new formation bought (`buy_unit`) and placed onto a flank this round.
  - Any formation moved (`move_unit`) into a flank from your **main deployment
    region** this round.
  - A formation that started the round on one flank and was moved across the
    board to the **other flank** this round (because the left and right flanks
    are two separate regions).
- **Does NOT travel (`settled`):**
  - A formation that was already on a flank when the round opened and **stays in
    that same flank** (even if it has **Deployment Module**, **Jump Drive**, or
    **Redeployment** and you reposition it to a different spot *inside the same
    flank*). Once a formation has completed its arrival fight on a flank, it
    opens every subsequent round settled there and fights immediately from tick
    `1` like any main-region formation.

### What Happens During the 8-Second Flank Arrival

When a fight begins, all of a side's `travelling` formations share one arrival
timer (`8` seconds by default):

1. **Present on the board and targetable from tick `1`:** Travelling mechs stand
   at their deployed flank positions from the very start of the fight. They
   count as active, living mechs for their side (so a side with only travelling
   mechs left has not lost the fight), and **enemies can lock onto, shoot, and
   kill them** while they are still arriving.
2. **Cannot act and have no movement collision while arriving:** Until the
   arrival timer finishes, travelling mechs do not move, do not acquire attack
   targets, do not fire weapons, and have no RVO collision body—other units can
   walk straight through where they stand.
3. **Starts at `40%` life and heals once per second:** Each travelling mech
   begins the fight at **`40%` of its maximum life** (truncated to a whole
   number, including any life bonuses from levels, officers, technologies, or
   equipment). Over the `8`-second travel window, it heals once per second by
   `1/8` of the remaining `60%` life (`8` heals total), reaching full life
   (minus any enemy damage taken) when all surviving travelling units on that
   side finish arriving together at `8` seconds.
4. **Halved by Quick Teleport (`4` seconds):** Holding the officer **Quick
   Teleport** reduces your side's flank travel time by `50%` to **`4` seconds**,
   healing `1/4` of the remaining `60%` life each second for `4` seconds before
   arriving.
5. **Production lines, shields, and self-repair wait for arrival:**
   - **Stat modifiers apply immediately:** Life, damage, speed, and range
     modifiers from officers, technologies, equipment, and Energy Tower skills
     are already factored into a travelling mech's stats from tick `1`.
   - **Unit-spawned summons wait:** A travelling unit wearing a production line
     (**Tank Production Line**, **Mustang Production Line**, **Steel Ball
     Production Line**) or carrying a **Spider Mine** technology produces
     nothing during the travel window, and spawns its first batch immediately on
     the first update after arriving.
   - **Carried domes and personal shields open on arrival:** A **Barrier** dome
     (from the **Barrier** equipment or the **Fortress** / **Hacker**
     **Barrier** technology) and a unit's personal energy shield (**Portable
     Shield** or unit shield technology) stand inactive and empty during travel,
     so shots hit the unit's life directly while arriving; the moment the unit
     finishes arriving, its dome or personal shield activates at full capacity.
   - **Self-repair starts from arrival:** **Nano Repair Kit** and self-repair
     effects do not tick during travel; their timer starts fresh when the unit
     arrives.
