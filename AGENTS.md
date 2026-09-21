# 项目设计规范
- 项目处于敏捷开发阶段，重构时不考虑后向兼容，不保留历史代码

# 目录规范

改动或新建一个文件之前，先从它所在目录逐层往上找最近的一份 `README.md`，找
到就读完再动手；一直找到仓库根还没有，才是真的没有约束。本仓库的 readme 一
律大写命名。

这些 readme 写的是所在目录的准入规则，从文件本身看不出来，而且往往正好禁止
了 agent 默认会做的事。例如：

- `replay/grbr/README.md`：录像是原样拷贝的，不许重写或规范化，哈希一致本身
  就是 fixture 契约的一部分；
- `replay/battle/README.md`：里面的 YAML 只能由 `mechcore replay convert` 重新生成，
  不许手改。值不对是转换器的问题，改 `crates/document/src/convert.rs`；
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
- `scripts`（Linux）：release 版二进制还答不答得出仓库声称的东西——fixture 哈希、
  重新生成 `replay/battle` 并要求结果没有差异、把每一份被跟踪的 `.mcscript` 过一遍
  `--check` 并**实际运行其中不需要游戏的那些**、`scripts/verify-battles.py`。
- `adapter`（macOS）：只查别处查不了的——Adapter 自己的 clippy 和测试、默认
  feature 下 `mechcore` 把 dylib 打包到可执行文件旁边、以及找游戏进程的那段 macOS
  代码。

Adapter 是 `mechcore` 的默认 feature `adapter`；只有它需要 macOS，关掉它
（`--no-default-features`）整个 CLI 在任何平台都能构建和测试。所以新代码若只在
macOS 上成立，要用 `cfg(target_os = "macos")` 隔开，不然 Linux 上的 job 会失败。

`scripts` 里重新生成语料那条意味着改了转换器就必须在同一次提交里重新生成语料；跑离线
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
给人来合并。

# 提交规范

提交信息用英文写，仓库现有日志是英文。

## 智能体署名

智能体或模型创建的每一条提交，都必须在提交信息末尾用 `Co-Authored-By`
署上自己的真实模型名称；知道具体型号时写明型号，不冒用其它模型的署名。
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

**一次提交是让标题成立所需的最小改动集合。** 拿掉其中任何一部分，标题就不再
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

**每次提交都要能独立编译、独立通过测试。** 拆分点不能落在"改了函数没改调用
方""加了行为没加断言"的位置。这条也排除了把测试单独拆成一次提交的做法。
