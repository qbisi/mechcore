# 语料成为游戏 oracle

语料的每一回合都有游戏给的答案，不依赖模拟器。

游戏能不建场景地打一份回放的任意回合（`record_replay_round`），一份 layout 也能写成回放来打
（`replay convert <layout.yaml> <replay.grbr>`，[layout-replay.md](../docs/spec/document/layout-replay.md)）。
`scripts/battle-replays.py` 把这两件事接到语料上：battle 的每一回合写成回放，由游戏无头打完，
同一回合的真实回放与由 battle 写出的回放各打一遍，两份录像应当相等；不相等的地方要么是投影丢了
战斗读的状态，要么是写入器的错。

## 栈

1. **oracle。** 无头打完的结果对照 battle 下一回合记录的状态，语料的每一回合因此都有游戏给的
   答案。

## 停车场

- **被拦截裁剪的油区网格。** 只由单元测试对着回放读取器验证过，还没在对局里验过。
  reopen_when：语料里出现一回合带被裁剪的油区。
- **语料的自动核对。** 现在只在本地跑（`replay.py sync`、`export-replay-corpus.py`、
  `verify-battles.py`）。语料独立增长，不宜挡 PR；待定的做法是一个 master 推送和每日触发的
  workflow，只核 `replays/<GAME_VERSION>/`。reopen_when：本地核对漏掉一次语料的破坏。
