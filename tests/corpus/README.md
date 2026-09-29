# Corpus rounds

Rounds of the replay corpus that the simulator plays back, pinned so that CI
holds it to them. The corpus itself is not in the repository, and
`ls work/match/<version>/*.yaml | mechcore verify` fights every round of it in
order where it is fetched ([`scripts/corpus/`](../../scripts/corpus/README.md)),
stopping at the first the simulator refuses or gets wrong. A round comes here
once the simulator fights it as the match says: usually when a divergence the
corpus found is fixed, the round that showed it is pinned by the pull request
that fixes it.

A round is pinned from the game's own fight of it, not from the match document:
the game fights the replay's round, and the recording read back as a fight
states its ticks and hash as well as the result, so a pin holds the whole
trajectory.

```sh
mechcore convert <replay.grbr> --to mcfr --backend game --round <n> /tmp/mechcore/corpus/<id>-r<n>.mcfr
mechcore convert /tmp/mechcore/corpus/<id>-r<n>.mcfr --to fight tests/corpus/fights/<id>-r<n>.yaml
```

`<id>` is the replay's match number, the digits after `--` in its file name
under `replays/<version>/`, and `<n>` the round. The comment at the top of the
file names the replay and says what the round showed. A rule the round bears
on cites it from [`docs/rules/`](../../docs/rules/); it is not copied into that
rule's topic.
