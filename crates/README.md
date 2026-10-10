# Crates

The Rust workspace. Each crate says what it does in its crate-level comment;
`mechcore` is the binary that every other crate is reached through.

| Crate | What it is |
| --- | --- |
| [`adapter/`](adapter/) | the in-process `cdylib` the game loads, which records and drives fights; the only macOS code |
| [`document/`](document/) | the document formats: layout, fight, state, action and match |
| [`mcfr/`](mcfr/) | the MCFR recording: its data model, writer, reader and hash |
| [`mechcore/`](mechcore/) | the CLI |
| [`player/`](player/README.md) | the page that plays a recording back |
| [`protocol/`](protocol/) | the messages between the CLI and the Adapter |
| [`simulation/`](simulation/) | the simulator: one round fought from a layout into a recording |

## What a test reads

A test reads nothing under the repository's root `tests/`. A fixture there is
the game's evidence, recorded again whenever the rules or the version move,
and `verify` checks every one in CI; a test that read one would fail for a
re-recording rather than for the code it tests, and check again what that
verify already checks. A test that needs a fight writes one, from a layout
under [`layouts/`](../layouts/README.md) or one it states itself, and a test
that needs a fixture's fact states it as the fixture's assert
([fight.md](../docs/spec/document/fight.md#asserts)).

A comment may cite a fixture as the evidence for what the code does; that
cites it and reads nothing. A crate's own test data under `crates/<crate>/tests/`
is the crate's. `scripts/check/check-fixture-readers.py` holds the rule.
