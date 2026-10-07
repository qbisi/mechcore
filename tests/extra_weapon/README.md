# Extra weapon fixtures

What an extra weapon technology adds beside a unit's main skill, and what sets
that skill apart from the main one. [`extra_weapons.md`](../../docs/rules/extra_weapons.md)
states what these fights settled. Each member of the list is a fight of its
own here once the simulator fights it.

The guns' locks, targets and states are in each recording's `skills` on every
tick, and their searches are `target_search` and `target_candidate` rows:

```sh
scripts/record-fights.py --instrument target_search,target_candidate \
    --out /tmp/mechcore/extra_weapon tests/extra_weapon/fights/*.yaml
```
