# RVO quadtree

[简体中文](quadtree.zh.md)

## Scope

This contract defines the agent neighbour index that sampled RVO queries, as
`crates/simulation/src/fight/rvo.rs::NativeQuadtree` implements it for build
`1.11.1.3.2259`. It serves [rvo.md](rvo.md).

It is not the target quadtree in `crates/simulation/src/fight/search.rs`, which
selects attack targets. The two have different data structures, capacities and
traversal rules, and mixing them up produces plausible wrong answers rather
than errors.

The tree is not merely a query accelerator, which is the reason it has a
contract at all. Leaf capacity, linked-list insertion order, the reordering a
split performs, branch visit order and the Q32.32 comparison rules all decide
which of several equidistant candidates survives into the final twenty
neighbours. Change any of them and the battle diverges.

## Data structure

```text
NativeQuadtree
  inputs : &[AgentInput]       # the agent array, fixed for this round
  nodes  : Vec<QuadtreeNode>   # a contiguous node pool
  next   : Vec<Option<usize>>  # one singly-linked list entry per agent
  bounds : QuadtreeRect

QuadtreeNode
  child00  # index of the first child; equal to its own index means a leaf
  head     # index of the first agent in the leaf list
  count    # agents in the leaf
  max_speed
```

Four children are always appended contiguously in one go, so a node stores only
`child00` and the rest are `child00 + 1..3`. A branch holds no agents; every
agent ends up in a leaf list.

| Constant | Value | Meaning |
| --- | ---: | --- |
| `QUADTREE_LEAF_SIZE` | 15 | the 16th agent inserted into a leaf triggers a split |
| `QUADTREE_MAX_DEPTH` | 11 | no split happens at or below this depth |
| `MAX_NEIGHBOURS` | 20 | nearest neighbours one query keeps |
| `DEFAULT_AGENT_TIME_HORIZON` | 12 s | the speed time window a node's reachable range uses |

## Two positions

Every input carries both:

- `tree_position`, the older internal position that `BuildQuadtree` uses;
- `position`, the current position after BufferSwitch, used as the query centre
  and for a candidate's real distance.

This is deliberate double-buffering, not redundancy. The tree's bounds, its
quadrant assignment, and the distance computed at query time may come from
different time slices. In RVO coordinates `y` is world `z`, not height.

## Bounds and centre

Construction initialises the root bounds from the first agent's
`tree_position`, then expands in input order using the native `FPoint.Min/Max`.
An empty input builds no tree, and a query returns an empty set.

A rectangle's centre is computed in this operation order:

```text
center.x = min.x + (max.x - min.x) * 0.5
center.y = min.y + (max.y - min.y) * 0.5
```

It may not be rewritten as `(min + max) / 2`. When the raw width is odd, the two
truncate differently in Q32.32. `FPoint.Min/Max` returns its second argument for
values equal within tolerance, so the bounds depend on input order too.

## Quadrant numbering

A quadrant comes from the native tolerance comparison `center < tree_position`.
A value equal to the centre, or differing by only 43 raw, falls to the low side.

| Quadrant | x | y | Formula |
| ---: | --- | --- | --- |
| 0 | low | low | `0 + 0` |
| 1 | low | high | `0 + 1` |
| 2 | high | low | `2 + 0` |
| 3 | high | high | `2 + 1` |

```text
                 y high
          +---------+---------+
          |    1    |    3    |
          +---------+---------+
          |    0    |    2    |
          +---------+---------+  x high
```

## Insertion and splitting

Agents insert strictly in input array order. The kernel adds the live buildings
from `self.buildings` that have collision enabled, then the live units from a
`BTreeMap` ordered by Unit ID.

At a leaf:

1. with `count < 15`, prepend the new agent to the leaf list;
2. with `count >= 15` and depth below 11, create four children contiguously;
3. walking the old leaf list from head to tail, prepend each existing agent into
   its own child leaf;
4. clear the parent leaf, then continue descending with the new agent;
5. at depth 11, keep everything in one leaf even past 15.

Prepending reverses order, and a split prepends each item again, reversing it a
second time. That list order becomes the leaf scan order and therefore fixes
which equidistant candidate wins. It cannot be replaced by a sorted `Vec` or a
hash container.

A split still happens when every point coincides and the root bounds have zero
width and height. All points sink through quadrant 0 to the maximum depth, and
the final leaf may hold more than 15.

## Node maximum speed

After every insertion, `max_speed` is computed bottom-up:

- a leaf takes the maximum `published_calculated_speed` over its members;
- a branch takes the maximum over its four subtrees, visited 0, 1, 2, 3.

What aggregates here is each agent's last published solved speed, not this
round's `desired_speed` or `max_speed`. A building aggregates 0.

## Query range

On entering a node, its coarse reachable distance is computed first:

```text
reachable = max((node.max_speed + query.max_speed) * 12,
                query.outer_radius)
            + query.outer_radius
distance  = min(reachable, current_20th_neighbour_distance)
```

The second term does not apply until twenty neighbours are held. This range
deliberately does not use a candidate's own relative speed or radius. It decides
only whether a node is worth visiting at all.

### Visiting a branch

A branch performs no general rectangle distance test. It asks whether the
axis-aligned range of radius `distance`, centred on the querying agent's current
`position`, crosses the node's centre. Children are visited in the fixed order
0, 1, 2, 3.

After each subtree, if twenty neighbours are held, the current twentieth
distance narrows `distance` for the remaining branches. A candidate found in an
earlier branch can therefore stop a later branch from being visited at all.

### Scanning a leaf

A leaf is scanned `head -> next`, filtering in order:

```text
candidate.key != query.key
candidate.main_layer == query.main_layer
query.collides_with & candidate.layer != 0
distance_squared(candidate.position, query.position) < leaf_range_squared
```

Distance uses both sides' current `position`, never their `tree_position`. A
surviving candidate is inserted in ascending distance and only the first twenty
are kept. The distance comparison is the strict native tolerance comparison, and
an equidistant item is appended after the equidistant items already there.

`leaf_range_squared` is computed once, on entering the leaf. Even if the scan
fills twenty neighbours partway through and updates the twentieth distance, the
remaining members of this leaf are still judged by the range that applied on
entry; the twenty-item truncation is what keeps the nearer ones. The narrowed
range affects only branches visited after leaving this leaf. This must not be
optimised into a per-candidate shrinking circle.

## The first tree, at zero positions

A newly created sampled agent already publishes its real coordinates while the
solver's internal position buffer is still zero. The native call order is build
the tree, then BufferSwitch, then query neighbours, so the first RVO boundary
satisfies:

```text
every agent's tree_position = (0, 0)
every agent's position      = its real current position
```

Combined with leaf capacity that produces two distinct cases:

- **At most 15 agents.** The root stays a leaf. A query passes through no
  spatial branch, scans every member, and filters by real position.
- **More than 15 agents.** The tree splits repeatedly at zero coordinates. A
  query centres its crossing test on the real position and may never reach the
  quadrant 0 branch where the agents actually are.

Two native scenarios separate the two cases: Steel Ball, with 8 units per side
plus 4 colliding buildings, is 12 agents and does not split; Rhino against
Crawlers, with 25 units plus 4 buildings, is 29 agents and does. The first
solve publishes at the next RVO boundary, which is MCFR tick 8 in the native
captures. Every later build uses the positions saved at the previous RVO
boundary rather than zeros.

## Determinism invariants

Changing the quadtree must preserve all of these:

- bounds use only `tree_position`; the query centre and leaf distances use only
  `position`;
- leaf capacity is 15, the 16th item splits, maximum depth is 11;
- four children are appended contiguously to the node pool;
- leaf members are prepended, and a split prepends again in old-list order;
- branches are visited 0, 1, 2, 3;
- among equidistant neighbours the one traversed first is kept;
- the twentieth distance narrows later branches only once twenty are held;
- the Q32.32 centre, `Min/Max` and comparisons keep the native operation order.

The unit test covering the double buffer directly is
`quadtree_builds_from_the_previous_buffer_but_queries_current_positions`.

## Fidelity boundary

This tree is faithful to the native neighbour index along the paths the
recorded RVO corpus traverses, and the evidence is the tick-by-tick agreement
that [rvo.md](rvo.md#fidelity-boundary) describes. It is a reproduction of one
build's behaviour, not a derivation from the algorithm, so a claim about it
outside those paths is unverified.

Not established:

- neighbour truncation and ordering that no recorded scenario has triggered,
  which is to say the behaviour when a query genuinely exceeds twenty
  candidates in contention;
- depth 11 saturation outside the coincident-point case;
- any agent population the corpus has not reached, buildings with collision
  layers other than those it contains included.

## Unresolved

**Should the constants be configuration or code?** Leaf capacity, maximum
depth, neighbour count and time horizon are native values that happen to be
correct for build 2259. They are compiled in, so a second build needs a rebuild
rather than a config, and a wrong value fails as a silent divergence rather
than as a load error.

**Should the zero-position first tree be modelled or reproduced?** It is
currently reproduced, because the native call order produces it. Whether a
future kernel is allowed to skip building a tree it knows is degenerate, and
still claim identity, depends on whether the first solve can ever observe the
difference. Nobody has shown that it cannot.
