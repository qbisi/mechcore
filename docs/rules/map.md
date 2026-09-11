# 1v1 maps

[简体中文](map.zh.md)

The 1v1 map IDs a single-round layout may select on build `1.11.1.3.2259`, and
what choosing one changes.

## Supported map IDs

| MapID | Map | Scene variant |
| ---: | --- | --- |
| 1001 | 铁道小镇 | day |
| 1011 | 森林巨眼 | ordinary mode |
| 1021 | 训练基地 | ordinary mode |
| 1031 | 铁道小镇 | dusk |
| 1032 | 铁道小镇 | deep night |

Build 2259 also has 1012, 森林巨眼 in competition mode, and 1022, 训练基地 in
tutorial mode. Both reuse the map resources above and carry special match rules,
which puts them outside what an ordinary single-round layout supports.

A layout selects one through its optional top-level `map_id`, which
[layout.md](../spec/document/layout.md) defines:

```yaml
map_id: 1001
seed: 2038621361
round: 7
sides:
  # ...
```

A layout exported from a replay records the original MapID, and `apply_layout`
loads that map before creating the Training Ground. An explicit ID is used as
given. Omitting `map_id` selects 1021, which keeps a result from depending on
whatever default the game itself might change.

## A map changes the outcome

A MapID is not scene decoration. A map brings its own neutral `FightCrystal`
objects, and those with an active RVO controller take part in the RVO spatial
index and neighbour solving as static agents. They therefore change unit
speeds, how long a fight lasts, and both result hashes.

What a map contains is a property of that map:

| MapID | Neutral `FightCrystal` | With an RVO controller |
| ---: | ---: | ---: |
| 1001 | 891 | 73 |
| 1021 | 27 | 0 |

So map objects can neither be deleted wholesale for being unaligned, nor
copied from one map into another. The rule is to load the native map named by
`map_id` and then keep the objects that map generates on its own. A replay and
a Training Ground scene that select the same map reproduce each other; two
scenes that differ only by map do not.

**Not covered.** These counts are for the two maps the recorded corpus
exercises. The remaining supported IDs have not been counted, and nothing here
establishes how many crystals or controllers they carry.

This is a statement about native replay and Training Ground behaviour only. A
`map_id` in a layout is not a claim that a simulator models map crystals or map
collision; loading native map resources is a separate capability with its own
answer.
