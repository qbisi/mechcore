# Opening deal parameters

These rules cover build **1.11.1.3.2259**, standard versus 1v1 with the shipped
opening pools and no game rules. They pin the deal size and the diversity
comparison. They do not determine how many random values reinforcement-pool
initialization consumes before the opening, or define other match modes.

## Deal size

`BattleOpeningController.CalculateChooseCount` returns the minimum of
`Config.advanceTeamSetting.chooseCount` and each pool's size divided by the
number of players sharing it. `AdvanceTeamSetting.chooseCount` is **4** in
`level0`, MonoBehaviour path ID **146**, at byte zero of its serialized body.
The following `unitCountPerTeam` is **2**. The standard pools contain enough
teams and specialists that the cap remains four.

## Diversity comparison

`BattleOpeningController.PrepareData` reads the selected map's `MatchSetting`
through `Config.GetMatchSetting`. It passes the field at native offset `0xE0`,
`advanceSameUnitMaximum`, as `ReinforcePool.RandAdvance`'s `diffUnitLimit`.
The standard versus map rows in `ConfigDataContainer.matchSettings` carry **3**
for that field. Despite its name, this argument is used as a lower bound on
unit diversity, not an upper bound on a repeated unit's squad count.

For each price tier touched by a candidate, the comparison adds the number of
distinct types already held, the new types in the candidate, and the remaining
picks **after** this pick. A total below `diffUnitLimit` rejects the candidate.
The number of remaining picks is `num - taken - 1`, with `taken` starting at
zero. In the native method, `not eax` followed by adding `num` computes
`num + ~taken`; arithmetic negation would count the current pick twice.

The constants are integers with no scaling or precision conversion.

## Evidence and boundary

The field values come from the shipped resources. The field access and
comparison come from the native instructions of
`BattleOpeningController.CalculateChooseCount`,
`BattleOpeningController.PrepareData(PlayerController, List<AdvanceTeam>,
List<IReinforceItem>, int)` and `ReinforcePool.RandAdvance`. Cpp2IL's ISIL labels
the native `not` as `Neg`; the native instruction determines the arithmetic.

Artifact identities (SHA-256):

| Artifact | Hash |
| --- | --- |
| `GameAssembly.dylib` | `9d2e3f163f74728da73b5dac474f76a0ebdaa3852718fbeb45ad8dec6b4360e5` |
| `global-metadata.dat` | `2488ba4661958da42438e91cb382259077ef7077dfa2261e74ade1c9b1b1fb43` |
| `level0` | `9276c12f99c188854c588e603220472d98623a8ffcc8c03f84516f4adbdce265` |
| ConfigDataContainer JSON export, path ID 160 | `92849e4b0cba65bb03448cac0c868ef94fcbbd476eabb8f2bc33d484e820ac4f` |

This establishes these parameters and this comparison on the named build. It
does not establish the full deal from the seed without its starting position,
the behavior of modified pools, or the meaning of an unobserved negative match
seed. Reopen when these artifacts change, a supported map supplies a different
parameter, or a native opening disagrees with this comparison.
