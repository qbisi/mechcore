# Layout fixtures

Each file is a layout document as [layout.md](../../docs/spec/document/layout.md)
defines it, used as input to `apply_layout` against the live game, to
`mechcore fight run`, or to both. A fixture is a scenario someone chose, so the
reason it exists belongs with it.

Most files here are named for what they contain and need no further
explanation. The ones below were built to exercise a specific native path, and
their coordinates are load-bearing: changing a position silently turns the
fixture into a different test that still passes.

`construction-battle.yaml` reproduces build 2227 opening construction group 28
with the `reverse_x` transform observed in replay R002. In blue's side-local
frame the Defensive Wall sits at `(-140, -55)`, the Rapid-Fire Turret at
`(140, -100)` and the Anti-Armor Turret at `(-140, -100)`. Red uses the
side-local coordinates that compile back to R002's recorded world positions.
Magnetic Barrier is absent deliberately: it is supported, but it is not part of
that opening group.

`interceptor-battle.yaml` places one `30 x 30` interceptor and one `50 x 20`
Stormcaller per side. It is the live regression sample for native placement,
recorder readback, shared footprint validation, round transition and shutdown.

`shield-missile-battle.yaml` gives each side a Stormcaller centred at
`(5,-40)`, a shield at `(1,-81)` and a missile at `(1,-11)`. The Stormcaller's
whole `50 x 20` footprint lies inside its own radius-70 shield, and after the
red-side transform each Stormcaller starts about 51.35 m from the opponent's
missile, inside the reference missile's 100 m trigger range. Both contraption
centres deliberately miss the deployment modulo-10 grid. It covers shield and
missile target-region validation, their exclusion from deployment collisions,
recorder readback, the battle interaction, round transition and shutdown.

`crawler-in-face.yaml` places a `50 x 20` Crawler per side at local `y=-20`,
front edge exactly on `y=-10`. Placement succeeds only while the room keeps the
standard 300 m-deep round-one main regions, so a pass also proves the initial
native constructions were cleared and their manager count read back as zero.

`six-unit.yaml` exercises persistent technology state and side-wide tower
state. Sledgehammer, Marksman, Fang, Wasp and Arclight take their Range
Enhancement technologies `10213`, `10202`, `10209`, `10206` and `10215`, and
red takes Improved Wasp Officer `30602`. Blue strengthens its Research Center
to level 2 and holds attack Officer `20311` and defense Officer `20300`; red
strengthens its Energy Tower to level 1 and activates both Energy Tower skills.
Blue releases `missile_strike` at world `(55,60)`, the centre of red's local
`(-55,-60)` front Fang. Red releases `mobile_beacon` at local `(-55,-60)`,
`(-105,-90)` and `(-105,20)`, compiling to world `(55,60)`, `(105,90)` and
`(105,-20)`, so the selected Fang first retreats briefly away from the adjacent
Wasp and the strike point, then advances on the displaced line.
