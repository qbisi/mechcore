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

Build the Nix package with `nix build .#mechcore`, or on ARM macOS build the
CLI and Adapter together with `nix build .#mechcore-with-adapter`.

CI builds and tests the flake's package checks on native `x86_64-linux`,
`aarch64-linux` and `aarch64-darwin` runners only on pushes to `master`;
these builds do not gate pull requests. Master builds upload newly built store
paths to Cachix, using the same configuration as nixos-images and nur-qbisi. In the repository's
**Settings → Environments**, create a `cachix` environment with:

- variable `CACHIX_CACHE_NAME`: the cache name without `.cachix.org`;
- secret `CACHIX_AUTH_TOKEN`: a token with write access to that cache.
