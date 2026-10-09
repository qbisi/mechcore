# RVO quadtree

## Scope

This contract defines the agent neighbour index that sampled RVO queries, as
`crates/simulation/src/fight/rvo.rs::NativeQuadtree` implements it. It serves [rvo.md](rvo.md).

It is not the target quadtree in `crates/simulation/src/fight/search.rs`, which
selects attack targets. The two have different data structures, capacities and
traversal rules, and mixing them up produces plausible wrong answers rather
than errors.

The tree is not merely a query accelerator, which is the reason it has a
contract at all. Leaf capacity, linked-list insertion order, the reordering a
split performs, branch visit order and the Q32.32 comparison rules all decide
which of several equidistant candidates survives into the final twenty
neighbours. Change any of them and the fight diverges.

## Data structure

```text
NativeQuadtree
  inputs : &[AgentInput]       # the agent array, fixed for this round
  nodes  : Vec<QuadtreeNode>   # the node array, as long as the native one
  filled : usize               # nodes in use, the first ones
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
agent in the tree is in a leaf list.

The node array's length is state of the fight, not of one tree. It is 16 when
the fight's simulator is made, and when four more nodes would leave fewer than
one slot free (`filled + 4 >= length`) it doubles before they are appended. A
tree starts from the length the previous tree of the fight left, which the
kernel carries as `QuadtreeCapacity`.

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

Agents insert strictly in input array order, which is `Simulator.agents`'
order. The kernel adds each side's live towers and then its live
constructions, then the map's crystals in the order `config/maps.yaml` lists
them, then the live units the fight started with by Unit ID, and last the units
whose agents joined since, in the order they joined (`RvoState.added_units`):
`DoAddAgents` appends a summon's agent as the summon is made and a travelling
unit's as it lands, and `List.Remove` keeps the order of the rest. A unit
landing from travel joins behind every unit of the other side, though its ID
is lower; replay 201475290's round 3 splits its tree in another order
otherwise.

At a leaf:

1. with `count < 15`, or at depth 11, prepend the new agent to the leaf list
   and count it;
2. otherwise append four children, and make the first of them the leaf's
   `child00`, unless appending them doubled the node array;
3. walk the leaf's list from its head: prepend each agent into the node
   `child00 + quadrant` and move the head to the agent it pointed to;
4. set the leaf's `count` to 0, and continue with the new agent one level
   deeper: into its quadrant's child, or, when the leaf stayed a leaf, into
   the same node again.

The depth counts every pass of the loop, so a leaf that stayed a leaf is
entered again one level deeper. A distributed agent is not counted: `count`
counts only the agents inserted into the leaf since it was made or last
distributed.

When the split doubled the array, `child00` is still the leaf's own index, so
step 3 sends an agent of quadrant 0 back to the leaf itself. The agent is
prepended to the list whose head it is, pointing to itself, and the head moves
past it: it leaves the tree. An agent of another quadrant goes to the node that
many places after the leaf, whatever that node is.

Prepending reverses order, and a split prepends each item again, reversing it a
second time. That list order becomes the leaf scan order and therefore fixes
which equidistant candidate wins. It cannot be replaced by a sorted `Vec` or a
hash container.

A split still happens when every point coincides and the root bounds have zero
width and height. Every split then sends the whole list to quadrant 0, which
holds it with a count of 0, so each level takes fifteen more agents before it
splits. The splits that double the array lose everything inserted before them.

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

The first tree is also the one that starts from an array of 16, and its
coincident points split the most. Leaf capacity and the array's growth give
three cases:

- **At most 15 agents.** The root stays a leaf. A query passes through no
  spatial branch, scans every member, and filters by real position.
- **16 to 60 agents.** The tree splits along quadrant 0 at most three times,
  within the 16 nodes, and keeps every agent in its deepest leaf. A query
  centres its crossing test on the real position and reaches that leaf only
  from within its range of the origin on both axes.
- **More than 60 agents.** The fourth split doubles the array to 32, and the
  first 60 agents leave the tree; the eighth doubles it to 64, and the first
  120 have left. Only the agents inserted after the last doubling are in the
  tree, the last of the units by ID.

Two native scenarios separate the first two cases: Steel Ball, with 8 units per
side plus 4 colliding buildings, is 12 agents and does not split; Rhino against
Crawlers, with 25 units plus 4 buildings, is 29 agents and does. The first
solve publishes at the next RVO boundary, which is MCFR tick 8 in the native
captures. Every later build uses the positions saved at the previous RVO
boundary rather than zeros, in the array the first tree grew.

## Determinism invariants

Changing the quadtree must preserve all of these:

- bounds use only `tree_position`; the query centre and leaf distances use only
  `position`;
- leaf capacity is 15, the 16th item splits, maximum depth is 11;
- four children are appended contiguously to the node array, which starts at
  16 for the fight and doubles when `filled + 4` reaches its length;
- a split that doubles the array leaves its leaf a leaf, and distributes it
  over the nodes after it;
- leaf members are prepended, and a split prepends again in old-list order;
- branches are visited 0, 1, 2, 3;
- among equidistant neighbours the one traversed first is kept;
- the twentieth distance narrows later branches only once twenty are held;
- the Q32.32 centre, `Min/Max` and comparisons keep the native operation order.

The unit tests covering the double buffer and the array's growth directly are
`quadtree_builds_from_the_previous_buffer_but_queries_current_positions` and
`a_split_that_grows_the_node_array_loses_the_list_it_splits`.

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
- a split that doubles the array when its list spreads over several quadrants,
  which sends agents into whatever nodes follow the leaf;
- any agent population the corpus has not reached, buildings with collision
  layers other than those it contains included.

## Unresolved

**Should the constants be configuration or code?** Leaf capacity, maximum
depth, neighbour count and time horizon are native values that happen to be
correct for the build. They are compiled in, so another build needs a rebuild
rather than a config, and a wrong value fails as a silent divergence rather
than as a load error.

**Should the zero-position first tree be modelled or reproduced?** It is
currently reproduced, because the native call order produces it. Whether a
future kernel is allowed to skip building a tree it knows is degenerate, and
still claim identity, depends on whether the first solve can ever observe the
difference. Nobody has shown that it cannot.
