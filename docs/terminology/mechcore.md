# Mechcore terms

This repository's own concepts, which the game does not have. Every Chinese
term here is ours.

## Documents

| English | 中文 | Meaning |
| --- | --- | --- |
| layout | 布局 | the board a fight starts from: each side's units, skills and placements |
| fight document | 战斗文档 | a layout with the result a fight reached, as a pinned fixture holds it |
| match document | 对局文档 | a whole match: its header, and a state and an action per round |
| script | 脚本 | an `.mcscript` that drives `mechcore run` |

## Recording

| English | 中文 | Meaning |
| --- | --- | --- |
| recording | 录制 | an MCFR file: what one fight did, tick by tick |
| replay | 回放 | the game's own record of a match, which the corpus is made of |
| corpus | 语料 | the native replays this repository measures the simulator against |
| adapter | 适配器 | the library injected into the game that drives and records it |
| simulator | 模拟器 | this repository's reimplementation of a fight |
| backend | 后端 | what fights a layout: the game or the simulator |
| content hash | 内容哈希 | the hash of what a recording holds, which the scorer compares |
| pin | 钉住 | a fixture whose hash the repository holds, which CI verifies |

## Research

| English | 中文 | Meaning |
| --- | --- | --- |
| divergence | 分歧 | where two recordings of one fight first differ |
| witness | 见证 | the recorded fields or events that would show a mechanism wrong |
| mechanism | 机制 | a behaviour of the game, closed on its decompiled code |
| refusal | 拒绝 | the simulator declining a fight it cannot model, naming why |
| decompilation | 反编译 | the build's code, read as evidence |
