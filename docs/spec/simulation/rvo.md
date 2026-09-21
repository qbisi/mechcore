# RVO movement avoidance

[简体中文](rvo.zh.md)

## Scope

This contract defines fixed-point sampled RVO as build `1.11.1.3.2259` performs
it: the agent inputs, the four-tick pipeline, how a neighbour becomes a velocity
obstacle, and how a velocity is solved. It reproduces the combat movement
behaviour of the native `GRPF.RVO.Sampled.Agent`, `RVOAgentFixed` and
`RVOControllerFixed`. It is not a general RVO library, and nothing here should
be read as the algorithm rather than as this build's version of it.

The implementation is `crates/simulation/src/fight/rvo.rs`, entered from the combat
loop at `crates/simulation/src/fight/motion.rs::step_rvo`. The neighbour index it
queries is [quadtree.md](quadtree.md).

This module is agent-agent avoidance and nothing else. Target selection uses a
different quadtree in `crates/simulation/src/fight/search.rs`, with different data
structures, capacities and traversal rules. The two must not be mixed.

The solver takes two kinds of agent: every live unit, and every live building
with collision enabled. A building participates as a locked, immovable ground
agent that never avoids anything but that neighbours must avoid. A
construction is one only to the other side: it sinks for its own, whose units
still count it among their neighbours and build no velocity obstacle from it.

RVO's private double buffers, neighbour lists and VO lists are inputs to
nothing. They appear in no layout and no recording, and a simulator recomputes
them from the publicly restorable unit, building and movement state.

Whether an unaligned `FightCrystal` joins the fight is the map's decision, not a
Training Ground artefact to be deleted. A replay and a Training Ground scene
select the same map through `layout.map_id` and keep its objects: on build 2259
map 1021 has 0 neutral crystal RVO controllers and map 1001 has 73. Loading map
objects is not implemented, so agreement with a native capture on one map is not
evidence about another.

## Coordinates and numerics

RVO works on the world's horizontal plane:

```text
RVO.x = world.x + 400 m
RVO.y = world.z + 400 m
```

The translation changes no distance. It keeps native RVO coordinates positive.
World height `world.y` takes no part in avoidance.

Position, radius, velocity, time and weight are all signed Q32.32 raw integers.
A metre value from a unit configuration quantises to 1 mm first, then enters
Q32.32. An intermediate value may not be converted to `f32` or `f64` and back,
because every one of these decides tick-by-tick agreement:

- multiplication follows the native raw truncation and wrapping path;
- division rounds to nearest on the absolute value, then restores the sign;
- vector division computes one shared reciprocal, then multiplies each
  component by it;
- `FPoint.op_LessThan` treats values within 43 raw as equal, so at least 44 raw
  of difference is required for true;
- `FPoint.Min/Max` returns its second argument when the two are equivalent, so
  argument order is part of the behaviour;
- square root, trigonometric and exponential functions use the `Fastest`
  fixed-point approximations that match the target build.

The collision branch's `centerMagnitude < radius` is a direct strict
comparison and does not use the tolerance comparison above.

## The four-tick double-buffered pipeline

An ordinary combat logic tick is about `0.05 s`, and RVO solves once every four
of them. One RVO boundary runs in this order:

1. publish the target point and velocity the previous solve produced;
2. update each unit's current velocity from what was published;
3. assemble agent inputs for buildings and live units;
4. build the quadtree from `tree_position` and aggregate each node's maximum
   published speed;
5. switch agents to their current `position`, then query the tree already built;
6. generate velocity obstacles for the neighbours and solve;
7. store the new target point and velocity in the solver buffer, to be published
   at the next four-tick boundary.

So a full RVO period separates solving from taking effect. Within a combat logic
tick, a unit's movement consumes the currently published target and velocity
first, and only afterwards may that tick reach an RVO boundary.

### The first tree

A native sampled agent writes its public `Position` at construction but does not
initialise the solver's internal position buffer, and the first `BuildQuadtree`
happens before the first `BufferSwitch`. Every new agent's first
`tree_position` is therefore `(0, 0)`, while the neighbour query that follows
already uses the real `position`.

```text
first boundary: tree_position = (0, 0),          position = real current
later:          tree_position = previous boundary, position = real current
```

The simulator reproduces this explicitly through `rvo_first_tree_pending`. It is
not jitter, and skipping the first solve is not an equivalent substitute.
Combined with leaf capacity it is observable: at 12 agents the root does not
split and the whole leaf is scanned, while at 29 agents the zero-coordinate tree
splits and a query from a real position may not reach the branch holding the
agents. [quadtree.md](quadtree.md#the-first-tree-at-zero-positions) has the
query semantics.

## Agent input

| Field | Meaning |
| --- | --- |
| `key` | stable identity within the Unit or Building namespace |
| `main_layer` | `Ground=1`, `Air=2`; a different main layer is excluded outright |
| `layer` | a single bit derived from collider priority |
| `collides_with` | the candidate layer mask the querying agent accepts |
| `group` | the avoidance group; the kernel fills it with the team ID |
| `locked` | true for an immovable agent, whose neighbours carry all the avoidance |
| `tree_position` | the older internal position this round's tree is built from |
| `position` | the current position used by the query and by every VO |
| `current_velocity` | the velocity implied by the last published target and speed |
| `desired_velocity` | the velocity implied by this tick's target direction and desired speed |
| `desired_target_delta` | the vector from the current position to the original movement target |
| `desired_speed` | the speed requested now |
| `max_speed` | the speed ceiling, used by the tree's reachable range and by the overspeed penalty |
| `published_calculated_speed` | the last published solved speed, aggregated by quadtree nodes |
| `radius_outer` / `radius_inner` | the RVO radii, which are not the MCFR collision radius field |
| `size` | the native ordered enumeration `Xs < S < M < L` |
| `priority` | the Q32.32 weight that splits avoidance responsibility within a group |

A unit's `outer_radius`, `inner_radius`, `size`, `collider_priority` and
`priority` come from the `rvo` block of `config/units/*.yaml`. The configuration
requires `inner_radius <= outer_radius`, a priority in `0..=1`, and a collider
priority in `1..=10`.

### Collision layers

For priority `p`, a movable agent uses the even bit `2p-2` and accepts its own
layer and every higher bit. An immovable building uses the odd bit `2p-1` and
sets its own `collides_with` to 0. A colliding building uses priority 10,
`size=M`, inner and outer radius both half the building's width, and
`locked=true`. A construction does the same on its row's
`pathfinding_collider_priority` instead of 10 — 5 for a Defensive Wall — and
is marked `passable_by_own_group`.

The filter is directional: a candidate becomes a neighbour only when
`query.collides_with & candidate.layer != 0`. A building therefore avoids
nothing itself, while a unit's query can still be matched by a high-priority
building layer. A candidate marked `passable_by_own_group` is still selected by
a query of its own group, and takes one of the twenty places; it is dropped
only when velocity obstacles are built, which is how a wall lets its own side
through. The native sidecar shows it: a Crawler crossing its own wall lists
three of the wall's blocks among twenty neighbours and builds seventeen
obstacles.

## Neighbour selection

Each agent keeps at most 20 neighbours from the quadtree. Candidates filter in
this order:

1. exclude the agent's own key;
2. exclude a different `main_layer`;
3. exclude a layer the querying agent's collision mask does not accept;
4. require the squared current-coordinate distance to be strictly below the
   squared query range;
5. insert in ascending distance and truncate to 20.

An equidistant candidate is never inserted ahead of an equidistant item already
held, so the quadtree traversal order decides every tie. Input assembly, leaf
list order and branch visit order are all deterministic state, and none of them
may be re-sorted by ID afterwards.

## Velocity obstacles from neighbours

Let `center = other.position - self.position`.

### A different group

A different group uses a short time window:

```text
offset          = (0, 0)
radius          = self.outer + other.inner
inverse_horizon = 100        # 0.01 s
```

The algorithm only compares whether the groups are equal. The kernel happens to
fill `group` with a team ID, and the RVO module does not interpret the value as
an allegiance.

### The same group

The same group first computes the querying agent's share of the avoidance, `s`:

```text
other.locked:              s = 1
otherwise:                 s = other.priority / (self.priority + other.priority)
weights summing to 0 or less: s = 0.5
```

Then it builds the cooperative velocity centre:

```text
other_optimal = Lerp(other.current_velocity,
                     other.desired_velocity,
                     clamp(2s - 1, 0, 1))
offset        = Lerp(self.current_velocity, other_optimal, s)
```

The radius choice is asymmetric: when the querying agent's size is smaller than
the neighbour's, both inner radii are used, and otherwise both outer radii. The
time window is fixed at 12 seconds, so `inverse_horizon = 1/12`.

### VO geometry

Each VO's base weight is:

```text
weight_factor = max(1, 1 + 4 * exp(-((|center|^2 / radius^2)^2)))
```

If the current centre distance is strictly below the radius, an
already-colliding separation line is constructed, with a response coefficient of
`0.3` and `inverse_delta_time = 1 / (4 * logic_delta)`. Exact contact does not
enter the collision branch.

Without a collision, the relative position and radius project into velocity
space as two tangents, a cut-off line and a far arc. Tangent angles use the
fixed-point `Atan2Fastest`, `AcosFastest`, `SinFastest` and `CosFastest`. For a
candidate velocity, a VO returns the gradient that leaves the forbidden region
and the depth of penetration; an effective gradient is then multiplied by
`2 * weight_factor`, and a positive weight gains one further raw `Q32_ONE`.

## Solving for a velocity

The solve first applies a clockwise symmetry bias to the desired velocity, using
only the largest positive penetration across all VOs:

```text
bias = min(0.1, max_penetration / |desired_velocity|)
desired += clockwise_tangent(desired) * bias
target  += clockwise_tangent(target)  * bias
```

A desired velocity with magnitude below `0.001` leaves the vectors unmodified,
while still keeping the question of whether it lies inside a VO.

- If the biased desired velocity lies in no VO, the original target point delta
  and `desired_speed` are kept as they are.
- If it lies inside one, two traces start, one from `current_velocity` and one
  from the biased `desired_velocity`, and the lower-scoring trace wins. On a
  score equal within tolerance the second is chosen.

Each trace runs exactly 50 iterations:

```text
step_size = max(outer_radius, 0x33333333 * desired_speed)
remaining = 1 - Q32(step_index) / Q32(50)
step      = remaining^2 * step_size
point    += normalize(gradient) * step
```

The first evaluation becomes the incumbent unconditionally. After that, only a
score lower by at least 44 raw replaces it. The gradient score is the sum of:

- the single highest-weighted gradient across all VOs, never a sum over several;
- an attraction term toward the biased desired velocity, weight `0.1`;
- a penalty for exceeding maximum speed, weight `3`;
- a penalty for exceeding the desired speed, weighted by two separately
  truncated `0.1` terms added together.

The best point becomes the new target point delta directly, and the solved speed
is `min(|point|, max_speed)`. No further multiplication by a time step happens
here: the target point delta and the speed are two independent outputs of the
native `CalculateVelocity`.

## Determinism invariants

Changing this module must preserve all of these:

- the coordinate translation, and that world height takes no part;
- every Q32.32 rule above, including the 43-raw tolerance, `Min/Max` returning
  its second argument, the shared reciprocal in vector division, and the
  `Fastest` approximations;
- the strict comparison in the collision branch, which is not the tolerance
  comparison;
- the seven-step boundary order, and the full RVO period between solving and
  taking effect;
- the zero `tree_position` on the first tree;
- neighbour ties resolved by traversal order, never re-sorted by ID;
- the asymmetric inner-or-outer radius choice by size;
- exactly 50 trace iterations, and the 44-raw threshold for replacing the
  incumbent;
- scoring from the single highest-weighted gradient rather than a sum.

Three layers of test hold this:

- `rvo.rs` unit tests pin build 2259's same-group pair solution and VO
  construction at raw values;
- kernel tests cover building collision, Q32.32 distance boundaries, the tree's
  coarse reachable range, and the behaviour at the edge of stopping;
- the native smoke samples in `tests/regression/mcfr-regressions.yaml` compare the stable
  physics projection's per-tick `physics_result_hash`, the Steel Ball battle
  sample included.

```text
cargo test -p mechcore-simulation rvo
cargo test -p mechcore-simulation --test battle native_regression_smoke_hashes_match
```

## Fidelity boundary

This is a reproduction of one build's behaviour, established by tick-for-tick
agreement between several mutually independent native trajectories and the
simulator, with no known counterexample. It is not a derivation from the RVO
algorithm, so agreement inside the recorded corpus is not an argument about
anything outside it.

Covered: the ordinary ground Formation Unit main path the corpus reaches,
including single and multiple neighbours and dense multi-unit migration.

Not covered:

- obstacles;
- airborne agents;
- group transition;
- neighbour truncation and ordering that no recorded scenario has triggered;
- units absent from the corpus, and any other internal condition branch nothing
  has taken.

A claim about any of those is unverified, however natural an extension of a
covered one it looks.

The local native RVO sidecar exists for research and for locating a divergence.
Its fields and scope are [adapter.md](../adapter/adapter.md)'s. It does not
substitute for a full MCFR battle hash and it is not a layout field.

## Unresolved

**Should `group` mean allegiance?** The module compares group identity and
nothing else, while the kernel fills it with a team ID. Either the field is an
opaque partition that a caller may key however it likes, in which case the
kernel's choice is incidental, or it is the team, in which case the module
should say so and a caller should stop being free to change it.

**Should a building be an agent or a boundary?** A colliding building is
currently a locked agent with a synthesised priority, size and radius, which
puts it in the neighbour budget of 20. A dense scene can therefore spend
neighbour slots on walls and drop units that matter more. Modelling static
geometry separately would remove that interaction at the cost of a second
avoidance path.

**What does map object loading mean for identity?** Native alignment is
established on maps whose neutral crystals the simulator does not load. Either
those objects are outside the fidelity claim permanently, which the scope should
state as a property rather than as a gap, or loading them is required before any
cross-map claim, which makes today's agreement map-specific in a way no recorded
hash announces.
