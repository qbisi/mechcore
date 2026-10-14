# Wiki

The player- and agent-facing English Wiki published to GitHub Pages, building a
human-readable conceptual model of Mechabellum from the native game build that
[`GAME_VERSION`](../../GAME_VERSION) pins.

| File | Role |
| --- | --- |
| [`index.md`](index.md) | the site landing page and navigation entrypoint |
| [`how-a-match-works.md`](how-a-match-works.md) | Getting Started: reverse-causal introduction to a standard 1v1 match and its round timeline |
| [`a-rounds-decisions.md`](a-rounds-decisions.md) | Getting Started: the 14 deployment actions, phase ordering, per-round allowances, upgrade/tech/equipment/tower/blueprint/skill rules |
| [`deployment-and-mobility.md`](deployment-and-mobility.md) | Game Mechanics: deployment regions, automatic 10m-grid landing, collisions, `legacy` movement locks and unlocks, and flank `travelling` |
| `.vitepress/config.mjs` | VitePress site configuration, building `docs/wiki/` into `work/pages/` with `README.md` excluded |

## What a Wiki page may claim

A Wiki page is answerable to the build, as [`rules/`](../rules/combat.md) is,
and written for a reader who wants to understand what the game does and what
follows from a decision without reading decompiled code or simulator contracts:

- **Observable behavior and scope.** Every mechanic claim states its trigger,
  what happens, what follows from it, and the material exceptions that would
  otherwise mislead a player, together with the mode and scope in which it
  holds.
- **Inspect actual configuration values before claiming differences.** A
  data-driven claim is checked against three things before it is written: the
  rule or consumer code, the actual values across the relevant rows of
  [`config/`](../../config/README.md), and the claim's counterexamples. That a
  table or formula indexes by level, flag or category does not show that the
  values differ in the pinned build.
- **Canonical names and numbers.** Unit, technology, officer, equipment,
  commander skill, blueprint, energy tower skill and map names follow
  [`terminology/`](../terminology/README.md) in official English. Constants trace
  to `config/`.
- **Evidence stays in `rules/`.** Decompilation anchors (`Class.member`),
  pinned fight paths under `tests/`, replay corpus scripts, CLI flags, Rust
  crate names, confidence labels and simulator progress do not appear in
  player-facing Wiki prose.
- **No unsupported strategy claims.** A page explains how mechanisms interact
  and what decisions cost or preserve, without claiming matchup superiority
  that no evaluation establishes.
