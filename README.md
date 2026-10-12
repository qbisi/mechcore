# mechcore

A deterministic Mechabellum simulator, recording toolkit, and battle player.

[![Rhinos fighting in round 5](assets/demo.png)](https://qbisi.github.io/mechcore/)

[Watch the battle](https://qbisi.github.io/mechcore/) — seven Rhinos with
Whirlwind and Armor Enhancement, from [a pinned corpus round](tests/corpus/67158166-r5.yaml).

Play it locally:

```sh
cargo run --release -p mechcore --no-default-features -- play tests/corpus/67158166-r5.yaml /tmp/mechcore-demo.html
```

Space pauses, the timeline seeks, and the mouse wheel zooms. The exported HTML
plays offline, with its code and battle data in one file.

[CLI manual](docs/spec/mechcore/cli.md) · [Game rules](docs/rules/combat.md) ·
[Terminology](docs/terminology/README.md)
