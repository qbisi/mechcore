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

`.github/workflows/ci.yml` 先由 `changes` 算改动范围，再分三个并行的 job，各答一个问题。
只动 `docs/` 的 PR 三个 job 都跳过（跳过算绿，automerge 照合）；动了 `scripts/`、`tests/`、
`replay/`、`layouts/` 或任何 `.mcscript` 跑 `scripts`；动了 `crates/`、`config/`、Cargo 文件
跑 `test` 和 `scripts`；动了 `crates/adapter/`、`crates/mechcore/`、`crates/protocol/` 或 Cargo
文件再跑 `adapter`；动了 workflow 文件全跑。要跑什么由改动决定，不由人决定：想让一个检查
跑，就改它读的东西。三个 job 是：

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

`.github/workflows/automerge.yml` 在 ci 通过或有人提交 review（`review.yml`）后运行。当一个 PR 的每一条提交和它的
正文都带 GPT 或 Claude 的 `Co-Authored-By` 落款（允许附带具体型号，忽略大小写）、
来自本仓库的分支、不是草稿、且该 commit 上的其它检查也全绿时，它以 **squash**
合并并删除分支。GPT 与 Claude 的提交可以混合；任何一条提交或正文没有上述署名，
就留给人来合并。解决 issue 的 PR（分支名以 `research/` 开头，或正文带
`Closes #n`）还要有 committer 在当前 head 上的 approve 才合并，见下一节。

**主线上一个 PR 就是一个 commit。** 仓库只允许 squash 合并：落到 master 的
commit 标题是 PR 标题加 `(#N)`，正文是 PR 正文，和 openai/codex 一样。所以下面
"提交规范"约束的对象是 PR：PR 标题按标题规则写，PR 正文按正文规则写，落款是
正文的最后一段。分支上的提交是工作记录，会被压掉，但每一条仍要带落款，因为
automerge 逐条检查。不要在别的 PR 的分支上再开 PR：父 PR 压成一个 commit 后子
分支的提交对不上，要 `git rebase --onto origin/master <父分支>` 才能合。

# 研究管线

模拟器的机制研究按 `.github/CONTRIBUTING.md` 的 Research 一节推进：一个问题一条
issue。持有游戏的会话（维护 `plan.md` 的主 agent，即 keeper）切问题、录像、发
oracle，再把问题交给自己起的子代理去做，子代理各用一个工作树，直接向 keeper 汇报，
不经过 GitHub 的标签和评论。

- **切问题先排依赖。** 答案会改同一个模块的两个问题先后做，不并行；issue 只写问题、
  假设、预测和可观测的验收，不规定答案走哪个模块、加什么表。`Touches` 是 keeper 的
  预估，不是禁区。
- **先定结构再写代码。** 一个只读子代理读反编译，交回结构图：答案对应 build 的哪些
  方法、build 在哪里按什么分支、模拟器里哪些函数已经对应它们。keeper 按 Acceptance
  的四个问题定下改什么、复用什么、不许新增什么，再交给实现子代理。
- **录像按需、串行。** 实现子代理缺录像时不绕过去（不用模拟器算哈希、不特判恰好录过
  的那个单位），而是交回录制请求；keeper 录完发到同一个 release，再让同一个子代理继续。
  钉住的哈希一律来自录像。
- **阻塞当场决定。** 撞上问题以外的机制时，keeper 在同一个会话里决定：切成新问题、
  换布阵重录，或接受已钉住的部分。
- **另一个子代理审查**，然后 keeper 自己读，再给 committer 发一张变更确认单：行为
  前后（带数字）、结构增删、钉住的哈希及其来源录像、新增或解除的拒绝、未验证的部分。
- **committer approve 才合并。** committer 在 GitHub 上 approve，或者在对话里明确授权
  keeper 对那一个 PR 执行 `gh pr review <n> --approve`；授权只对那一个 PR、那一个
  commit 有效。GitHub 不允许作者 approve 自己的 PR，所以只要 agent 还用 committer 的
  账号开 PR，就没有 review 能生效，这时由 committer 看过确认单后手动合并。

**游戏只有一个进程，只有 keeper 持有它。** 其它 agent 永远不跑带 `game:` 的脚本，
`mechcore run <script> --check` 会说一份脚本要不要游戏。

证据在三处，都不在这个仓库的历史里：反编译在私有的 `mechcore-decomp`，
`scripts/decomp.py sync` 放到 `work/decomp/<build>/`（dump 在 `cpp2il/`，索引
`index.sqlite` 在同一目录，本机已有的不重下）；录像语料在公开的 `mechcore-replay`
（`scripts/replay.py sync` 按 `replay/REPLAY_REV` 取回 `work/replay/`）；每个问题的
录像和 sidecar 在它自己的 release（`scripts/oracle.py fetch <n>` 回到 `/tmp/mechcore/`），
issue 关闭后 release 仍保留，因为它是钉住的哈希的证据。仓库里固定下来的只有
`tests/<topic>/` 的布阵、脚本和它钉住的哈希。

游戏换了版本，持有游戏的会话用 `scripts/decompile.py` 反编译本机装的那一版：它从游戏
本身读出 build 号，缺的工具（Cpp2IL、AssetRipper，版本和 SHA-256 钉在脚本里）自己下到
`work/tools/`，产出和已有 build 同一种形状；`scripts/decomp.py publish <build>` 推进
`mechcore-decomp`，别的机器照常 `sync`。两个 build 之间改了什么，
`scripts/decomp-diff.py <旧> <新>` 逐个声明比，加 `--config` 比两份配置表。

keeper 会话之外的 agent（云端会话、别的机器）仍可以认领带 `claimable` 的 issue：
开草稿 PR 认领，要录像就提交布阵和脚本并打 `capture`，被挡住就开 finding 写
`Blocked by #m`，做完转正式，由 keeper 从审查那一步接手。

**编号只写依赖。** issue、PR、提交里写别的编号会在 GitHub 上双向挂链接，没有依赖的链接只会
把真正的依赖淹掉。所以只在 `Closes #n`、`Blocked by #n` 或推动另一项的决定里写编号；来历用
机制、夹具或文件名交代。被跟踪的文件（文档、脚本、布阵、README）一律不写 issue 编号。

# 提交规范

这里的"提交"指落到主线的那个 commit，也就是一个 PR：标题是 PR 标题，正文是
PR 正文。提交信息用英文写，仓库现有日志是英文。

## 智能体署名

智能体或模型创建的每一条提交，以及它开的 PR 的正文，都必须在末尾用
`Co-Authored-By` 署上自己的真实模型名称；知道具体型号时写明型号，不冒用其它
模型的署名。落款是正文的最后一段，后面不再有别的行，git 才把它认成 trailer。
GPT（包括通过 Codex 工作的 GPT）使用以 `GPT` 开头的模型名称，Claude 使用
以 `Claude` 开头的模型名称，例如：

```text
Co-Authored-By: GPT-6 <noreply@openai.com>
Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
```

只添加实际参与该提交的模型署名；其它模型同样如实署名，但不因此获得自动
合并资格。修改或压缩提交时也必须保留真实的贡献者署名。

## 标题

形如 `type(scope): 一句小写的话，说这次提交换来了什么`。写读者因此得到了
什么，不写你动过哪些文件。不超过 72 字符，结尾不加句号。

| 不要 | 要 |
| --- | --- |
| `Update replay script paths` | `fix(adapter): let a side order its own towers` |
| `Add issue directory` | `docs(issue): give findings a holding pen between discovery and the plan` |
| `Refactor ledger code` | `feat(battle): close the supply ledger` |

左边那些 diff 自己会说，不需要你再说一遍。

## 正文的组成

正文唯一不可替代的用处，是记住 diff 里看不见的东西。半年后有人 `git blame`
到某一行，他看得到代码，看不到你当时知道的事。按顺序写三部分。

**一、之前是什么样、现在是什么样。** 一段，有数字就给数字。

> The ledger closed 314 of 531 decidable round transitions and left 19
> unpriced. It now closes all 550, and none is left unpriced.

**二、每个发现一段，段首第一句要能单独成立。** 例如 `A position does not name
a tower.`、`Releasing a contraption is a purchase.`。读者只扫首句，也能知道这
次提交推翻了哪些原有认识。这部分必须交代：

- 旧做法错在哪，以及是什么证据推翻的：读到的反汇编、实机读数、语料统计；
- 干活过程中撞坏了什么、怎么修的，尤其是只有真跑一次才会暴露的；
- 哪些数字是从游戏数据里读出来的，哪些是量出来的。

**三、验证。** 跑了什么、证明了什么、证据留在哪个目录。"对得上的 tick 数"
"逐字节一致的重新生成"是证明，"测试通过"不是。

没验证的写明没验证，量出来而不是读出来的写明出处不明，只做了一半的写明另一
半没做。提交信息里的每句话以后都会被当成事实引用，包括被你自己引用。

不要罗列改动文件清单，也不要写"改进了""优化了""重构了代码结构"这类不带内容
的话：这两样 diff 都已经说过，而且说得比你准。

正文按 72 字符折行，与标题之间空一行。

## 拆分判据

**一个 PR 是让标题成立所需的最小改动集合。** 分支上怎么分提交无所谓，压掉之后
只剩标题和正文；下面说的"一次提交"都读作"一个 PR"。 拿掉其中任何一部分，标题就不再
成立或变成夸大；能拿掉而标题照样成立的部分，属于另一次提交。

标题写不出来就是拆分信号。需要用逗号罗列、需要 "and also"、需要 "various"，
那不是标题的问题，是这次提交的问题。

**一起被发现不是理由，一起被引起才是。** `fix(adapter): let a side order its
own towers, and find its hidden officers` 用 and 连了两个 bug，因为两个都由同
一次 layout 重构引入、同一次实机运行暴露。只是碰巧在同一个下午撞见的两件事，
分开提交。

**机械改动单独一次，除非它自己引出了修复。** 搬迁、重命名、批量改路径这类
零语义的大 diff，混进去会让真正的改动没法审。`docs: split into rules and
spec` 改了 40 个文件，全是搬迁与重新归类，没有一行新规则；写下那份规范的提
交紧跟在它后面，单独一次。反过来，把脚本搬进 `scripts/` 时修掉搬迁自己弄坏
的两处路径，属于同一次，因为不搬就不会坏。

**不要按层拆。** `docs(action): define the action space` 一次改了 4 个 crate
源文件和 2 份 spec，因为 spec 是代码要满足的契约，拆开之后任一半都不自洽。
`feat(battle): close the supply ledger` 同样横跨 config、crate、docs 六个文件。
文件数、行数、目录、"代码/文档/配置"都不是拆分依据。

**每个 PR 都要能独立编译、独立通过测试。** 拆分点不能落在"改了函数没改调用
方""加了行为没加断言"的位置。这条也排除了把测试单独拆成一个 PR 的做法。
