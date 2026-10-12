---
name: Unimplemented
about: A reproducing layout reaches an unimplemented simulator branch
title: ""
labels: unimplemented
assignees: ""
---

## Triggered error

Commit: `<commit>`

```sh
cargo build --release -p mechcore --no-default-features
target/release/mechcore convert layout.yaml --to mcfr
```

```json
<the exact error, including reason and sites when available>
```

## Reproducing layout

<!-- Embed the complete layout, or link to a tracked layout at a pinned commit. -->

```yaml
<layout.yaml>
```
