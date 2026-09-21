# 项目设计规范
- 项目处于敏捷开发阶段，重构时不考虑后向兼容，不保留历史代码

# 目录规范

改动或新建一个文件之前，先从它所在目录逐层往上找最近的一份 `README.md`，找
到就读完再动手；一直找到仓库根还没有，才是真的没有约束。本仓库的 readme 一
律大写命名。

这些 readme 写的是所在目录的准入规则，从文件本身看不出来，而且往往正好禁止
了 agent 默认会做的事。例如：

- `replay/README.md`：录像和转换出的对局文档不在这个仓库里，在 `mechcore-replay`，
  这里只钉一个 commit（`REPLAY_REV`），`scripts/replay.py sync` 取到 `work/replay/`；
  录像不许重写，对局文档只能由转换器生成，值不对改 `crates/document/src/convert.rs`；
- `docs/README.md`：一份新文档算 rules 还是 spec，spec 归到哪个 crate 名下，
  必须写哪几节；
- `work/research/README.md`：一个数值要拿什么才算有据，什么看着像证据其实
  不是，以及"能复现录像不等于证明了机制"。

issue 不在仓库里，它是 GitHub issue。什么够格成为一条 issue、它必须写明哪
几件事、它以什么方式离开，由 `.github/CONTRIBUTING.md` 规定，开 issue 前先
读它。

readme 的效力高于你自己的判断。和你想做的事冲突时按它做，或者先说清楚它为
什么该改，不要绕过去。

反过来同样成立：改动让最近那份 readme 不再成立时，同一次提交里把 readme 一
起改。它描述的是当前状态，不是历史。

# CI 与自动合并

`.github/workflows/ci.yml` 分三个并行的 job，各答一个问题：

- `test`（Linux）：代码本身对不对——`cargo fmt --all -- --check`、
  `cargo clippy -D warnings`、`cargo test`，都是
  `--workspace --exclude mechcore-adapter --no-default-features`。
- `scripts`（Linux）：release 版二进制还答不答得出仓库声称的东西——按
  `replay/REPLAY_REV` 取回 `mechcore-replay` 的语料并核对它的哈希、用当前转换器把语料
  再转一遍并要求逐字节一致、把每一份被跟踪的 `.mcscript` 过一遍 `--check` 并
  **实际运行其中不需要游戏的那些**、`scripts/verify-battles.py`。
- `adapter`（macOS）：只查别处查不了的——Adapter 自己的 clippy 和测试、默认
  feature 下 `mechcore` 把 dylib 打包到可执行文件旁边、以及找游戏进程的那段 macOS
  代码。

Adapter 是 `mechcore` 的默认 feature `adapter`；只有它需要 macOS，关掉它
（`--no-default-features`）整个 CLI 在任何平台都能构建和测试。所以新代码若只在
macOS 上成立，要用 `cfg(target_os = "macos")` 隔开，不然 Linux 上的 job 会失败。

`scripts` 里再转一遍语料那条意味着改了转换器就得把 `mechcore-replay` 的 `MECHCORE_REV`
推到这次改动、再把 `REPLAY_REV` 跟上，不然 CI 过不去；跑离线
脚本那条意味着一份离线脚本里的断言和一份测试同等有效，`tests/modifier/regressions.mcscript`
就是靠它守住的。`docs.yml` 另跑 `scripts/check-docs.py`。

一份 `.mcscript` 要么需要游戏、要么不需要，`run --check` 的 `game` 字段就是答案：
需要游戏的只被解析，不需要的会被跑起来。新增一份离线脚本不用改 CI。

格式这一条曾经不在 CI 里，因为仓库本来就不符合当前 rustfmt 的输出。那次全仓
格式化已经做过了，所以现在它是 CI 的一条硬检查：**提交前跑 `cargo fmt --all`**，
不要再手工比对单个文件。`.githooks/pre-commit` 把同一条检查提前到提交那一刻，
每个 clone 装一次：

```
git config core.hooksPath .githooks
```

用仓库级设置，是因为全局 `core.hooksPath`（Nix 或 home-manager 常设）会盖过
`.git/hooks`。

`.github/workflows/automerge.yml` 在 ci 通过后运行。当一个 PR 的每一条提交都
带 GPT 或 Claude 的 `Co-Authored-By` 落款（允许附带具体型号，忽略大小写）、
来自本仓库的分支、不是草稿、且该 commit 上的其它检查也全绿时，它直接合并
并删除分支。GPT 与 Claude 的提交可以混合；任何一条提交没有上述署名，就留
给人来合并。认领了 issue 的 PR（分支名以 `research/` 开头，或正文带
`Closes #n`）还要等主 agent 打上 `accepted` 标签才合并，见下一节。

# 研究管线

模拟器的机制研究按 `.github/CONTRIBUTING.md` 的 Research 一节并行推进：
一个问题一条 issue。持有游戏的那个会话（维护 `plan.md` 的主 agent，即
keeper）先把问题收成一个数、读反编译、把 fixture 和录制脚本提交到 master、
开 issue，再录像并用 `scripts/oracle.py publish <n>` 发成本仓库的 release
`oracle/issue-<n>`。认领的 agent 从 `research/<n>-<slug>` 分支开草稿 PR
（正文首行 `Closes #n`，草稿即认领），用 `scripts/oracle.py fetch <n>` 取回
录像，离线拟合、实现、钉住、写规则，`scripts/check-scripts.sh` 全过后转正式；
keeper 读过、跑过之后打 `accepted`，automerge 才合并，合并后 release 删除。

证据在三处，都不在这个仓库的历史里：反编译在私有的 `mechcore-decomp`（云端
通过 GitHub 读），录像语料在公开的 `mechcore-replay`（`scripts/replay.py sync`
按 `replay/REPLAY_REV` 取回），每个问题的录像和 sidecar 在它自己的 release。
仓库里固定下来的只有 `tests/<topic>/` 的布阵、脚本和它钉住的哈希。

**游戏只有一个进程，只有 keeper 持有它。** 认领方永远不跑带 `game:` 的
脚本，`mechcore run <script> --check` 会说一份脚本要不要游戏。要新录像，
就把布阵和录制脚本（连预期值）提交到分支上，在 PR 上说明它区分什么并打
`capture` 标签，等 keeper 录完发到同一个 release。
