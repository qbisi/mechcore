# mechcore

A deterministic Mechabellum simulator, recording toolkit, and battle player.

Read the [Mechcore Wiki](https://qbisi.github.io/mechcore/), starting with
[How a Match Works](docs/wiki/how-a-match-works.md).

![Rhino Whirlwind in the battle player at tick 178](assets/demo.svg)

Play a pinned corpus round locally in the battle player:

```sh
cargo run --release -p mechcore --no-default-features -- play tests/corpus/67158166-r5.yaml
```

Space pauses, the timeline seeks, and the mouse wheel zooms. The exported HTML
plays offline, with its code and battle data in one file.
The download button exports the current tick as an SVG vector image.

[Wiki](docs/wiki/how-a-match-works.md) · [CLI manual](docs/spec/mechcore/cli.md) ·
[Game rules](docs/rules/combat.md) · [Terminology](docs/terminology/README.md)
