# 掼蛋多端框架（guandan）

一套**规则驱动**的掼蛋引擎与多端框架：玩法写进 YAML（RuleSet DSL），引擎只解释规则，不做硬编码分支；同一份 Rust 代码同时编出 **native**（服务端 / 测试）与 **wasm**（客户端）两个产物，两端对同一动作序列必须算出**逐字节相同**的 `stateHash`。

> 当前阶段：**M2 进行中**（M1 已收口）。M2 首个切片已落地：`services/server` 联机骨架（axum + tokio，protobuf over WS，入座→发牌→动作→结算→重连），协议层补了传输帧头与快照 `roundNo`。
> M0 五条硬 gate 已全部收口；M1 的牌型全表、枚举路径重构、结算两批、进贡/还贡/抗贡、跨局携带、房间快照 wire、组牌推荐（第三十七次变更）、房间主干（建桌→发牌→行动窗口→出牌→结算→再来一局）、AI-L0/L1/L2（跟牌、留牌、记牌、队友配合）、Web 端（牌桌/大厅/结算/设置）+ IndexedDB 杀进程续局、校验器 L2–L4、接风与进贡后手牌刷新专项均已落地；覆盖率门禁按「行 ≥90% 且区域 ≥90%」实测行 **92.23%** / 区域 **91.61%**，已过线。
> 设计口径的唯一权威是 `docs/designs/2026-11-23-guandan-multiplatform/design_draft.md`（已签署冻结）；本文只做导览，不承载任何设计结论。

---

## 1. 项目概况

| 项 | 值 |
| --- | --- |
| 语言 / 工具链 | Rust（`rust-toolchain.toml` 钉 `stable`，组件 `rustfmt` + `clippy`，target `wasm32-unknown-unknown`） |
| 工作区 | 根 workspace 8 个成员：7 个共享 crate（`gf-engine`、`gf-protocol`、`gf-table-runtime`、`gf-ai`、`gf-client-wasm`、`gf-deckart`、`gf-runtime-spike`）+ 后端 `gf-server`；前端 `apps/web`、桌面壳 `apps/desktop` 不入 workspace |
| 规则载体 | YAML RuleSet（牌型原语/组合、牌力序、阶段表、进贡、结算、AI 推荐权重） |
| 确定性 | 判定链零浮点、零随机、零 IO、不依赖 `HashMap` 迭代序；排序键全部显式 |
| 双产物 | native `rlib` + wasm `cdylib`（裸产物、零 import，Node 可直接加载） |
| 文档 | `docs/`（设计稿、变更台账、施工计划、实测资产） |
| 测试现状 | `cargo test --workspace` **530 通过 / 0 失败**（2026-11-26，含 `gf-server`） |

**玩法要点（v1 已冻结口径，速查）**

- 牌型闭集：单张、对子、三张、三带二、顺子（5 张，含轮子 `A-2-3-4-5`）、三连对、钢板、炸弹 4–8 张、同花顺、四王炸。
- **四王炸只有四张王才成立**（两张小王 + 两张大王）；单张王只能单出，**两张同王成对**（对大王 > 对小王 > 对级牌 > 对 A），三张王不成三张。
- 三带二 = 三张同点 + 一对；对子可以是真牌对，也可以是**两张同色王**（两小王或两大王），**一大一小不成对**。
- 逢人配（红桃级牌）可代牌，张数不限、多孔取 `rankKey` 最大解；可组同花顺。
- 每组局只有一份局级牌（`attackingTeam` 队的级牌）。自然序 `naturalOrder` 写作 `2,3,…,A`：**2 最小、A 最大，王在序尾**。级牌按 `levelInsert: above-A` 从原位抽出、插到 A 之上（小王之前）——所以**打 2 时 2 最大，不打 2 时 2 还是最小**，其余点数依次让位；级牌恰好是 A 时也仍在小王之前，不会越到王后。
- 组牌型（顺子 / 三连对 / 钢板 / 同花顺）只按自然序比较，级牌**不**提升（第二十五次变更）；因此 `A-2-3-4-5` 是最小顺子（轮子 `rankKey` 取保留值 `0`）。散牌型（单张 / 对子 / 三张 / 三带二主牌）与炸弹走合成序。
- 接风、进贡/还贡/抗贡、升级档、过 A 与 `aFailStreak`、局终止与名次都按 RuleSet 声明执行。
- 续局进贡：单下末游 → 头游；双下末游 → 头游、三游 → 二游。贡方自动交手里最大的牌（红桃级牌除外），
  受贡方进入 `tribute` 相位后**自主挑一张点数不大于 10 的牌**还回去；没有 ≤10 时退化为整手可还。
  全部还完才转 `play`，由**贡牌牌力最大的人**先出（双下同力取先配对者）；贡方握两张大王时抗贡，头游先出。
- A 上连续 3 局未过退回起点 2 时，安全局数窗口从下一局重新起算；不会因为回退前已经打到
  `matchCap.rounds` 就直接结束场次（回到 2 后仍有一整轮窗口可以继续冲 A）。

---

## 2. 架构

依赖是**单向**的：引擎不知道网络形状，协议不知道引擎内部。

```
        ┌──────────────────────────── 交付层 ─────────────────────────────┐
        │ apps/web（Web/PixiJS，内嵌 wasm）  apps/desktop（Tauri 壳）        │
        │                     services/server（联机服）                    │
        └───────────────┬───────────────────────────────────────┬──────────┘
                        │  帧消息                                │  入座/发牌/计时/托管
                 ┌──────▼───────┐                        ┌───────▼──────────┐
                 │ gf-protocol  │  proto → prost 生成代码 │ gf-table-runtime │
                 │ (§5.9 协议)  │  两端共用同一份字节      │ 快照 wire + view │
                 └──────────────┘                        └───────┬──────────┘
                                                                │ 只调公开接口
        ┌───────────────────────────────────────────────────────▼──────────┐
        │ gf-engine（纯函数式规则引擎，native/wasm 同一份）                    │
        │  contract    Engine trait：validate / legal_actions / apply / view / recommend    │
        │  engine      M0Engine 实现：状态转移、轮转、结算链、进贡、跨局（含私有 settle）    │
        │  ruleset     model/params/parse/validate(L1) + ladder + shape_plan(L2 前编译) │
        │  patterns    牌型识别 recognize / 枚举 enumerate / 压制 beats           │
        │  hash, view  stateHash 十段拼接、canonical view 编码、viewHash          │
        │  settlement  升级/过 A/名次判定                                        │
        │  recommend   组牌推荐：计数向量精确 DFS + 记忆化 + 两条后处理            │
        │  cases       夹具（m0-minimal / guandan-classic）与黄金用例入口          │
        └────────────────────────────────────────────────────────────────────┘
```

| crate | 职责 | 关键约定 |
| --- | --- | --- |
| `gf-engine` | 规则引擎本体，与网络、时钟、文件系统完全无关 | 判定链禁浮点 / 禁随机 / 禁 `SystemTime::now()` / 禁 IO；所有输出可复算 |
| `gf-protocol` | `crates/protocol/proto/gf/v1.proto` → `prost` 生成代码，**生成物入库**（`src/generated/`），服务端与 wasm 共用同一份 | 禁止手改生成代码；`build.rs` 需要系统 `protoc` |
| `gf-runtime-spike` | M0 断线恢复原型。`recovery` 模块**要迁入 `gf-table-runtime`**，`harness` 是一次性夹具、可丢弃 | 不要把一次性夹具当成交付物 |
| `gf-table-runtime` | 房间快照 wire 编解码 + 服务端 `viewHash` 自证（字段序固定、little-endian、定宽整数） | 客户端只透传 `viewHash`，不自行计算 |

`gf-engine` 内部模块：

| 模块 | 内容 |
| --- | --- |
| `contract.rs` | `Engine` trait（`validate` / `legal_actions` / `apply` / `view` / `recommend`）、`State` / `Action` / `LastPlay` / `Seat` 等契约类型、`EngineError` 与错误码表 |
| `engine.rs` | `M0Engine`：`deal` / `legal_actions` / `apply` / 私有 `transition`·`settle` / `view`，以及跨局 `deal_next_round` |
| `ruleset/` | `model`（数据结构与 `RecommendMode`/`RecommendWeights`）、`params`（原语参数值域）、`parse`（YAML 入口）、`validate`（L1 闸门与错误码）、`ladder`（载入期合成阶梯）、`shape_plan`（原语 → 判定计划，运行期零动态分派） |
| `patterns.rs` | `recognize`（单次判定）、`enumerate`（候选枚举）、`beats`（跨型压制）、`card_id`/`joker_id`/`wildcard_ids` |
| `hash.rs` | `state_hash(&CanonicalState)`：十段固定顺序拼接 + 1 项版本三元组旁挂，canonical 编码 |
| `view.rs` | 每座位视图白名单编码与 `view_hash` |
| `settlement.rs` | 升级档、过 A、名次与 `SettleResult` |
| `recommend.rs` / `recommend_key.rs` | 组牌推荐：手牌压成 60bit 计数向量键 → 精确 DFS + 记忆化 → 三带二/同花顺两条后处理 |
| `cases.rs` | `m0-minimal`、`guandan-classic` 夹具与固定 seed 入口 |

---

## 3. 运行环境

| 依赖 | 版本 / 说明 | 必要性 |
| --- | --- | --- |
| Rust | stable（本机实测 1.98.1）；进目录后 `rustup` 按 `rust-toolchain.toml` 自动装 `rustfmt`/`clippy`/`wasm32-unknown-unknown` | 必需 |
| Node.js | ≥ 18（本机实测 v22.23.2）；只用于 wasm 宿主脚本（`tests/wasm/*.mjs`） | 跑双端同解 gate 时需要 |
| `protoc` | 任意可用的 protobuf 编译器（本机实测 29.3.0）。`gf-protocol/build.rs` 会调用它 | 编译 `gf-protocol` / 跑全工作区 clippy 时必需 |
| Git | 提交纪律要求按路径 `git add` | 必需 |
| PowerShell 7（`pwsh`） | 跑 `.ps1` 双端脚本；Linux/macOS 亦可 | 跑双端脚本时需要 |

安装示例（Windows）：

```powershell
winget install Rustlang.Rustup
winget install OpenJS.NodeJS.LTS
winget install Protobuf.Protobuf      # 或 choco install protoc
# 若 protoc 不在 PATH，可只对本进程生效：
$env:PROTOC = "D:\soft\protobuf\bin\protoc.exe"
```

macOS / Linux：`rustup` 官方脚本装 Rust，`brew install protobuf node` 或 `apt install protobuf-compiler nodejs`。

---

## 4. 安装与构建

```powershell
git clone <repo-url> guandan
cd guandan

# 首次构建（会自动按 rust-toolchain.toml 装组件与 wasm target）
cargo build --workspace

# wasm 产物（客户端用）
cargo build -p gf-engine --target wasm32-unknown-unknown --release
# 产物：target\wasm32-unknown-unknown\release\gf_engine.wasm
```

如果 `cargo build --workspace` 在 `gf-protocol` 处报 `Could not find protoc`，见 §8 第 1 条。

---

## 5. 常用命令

| 用途 | 命令 |
| --- | --- |
| 全量测试 | `cargo test --workspace` |
| 单个 crate | `cargo test -p gf-engine` |
| 单个测试文件 | `cargo test -p gf-engine --test recommend_test` |
| 单个用例 | `cargo test -p gf-engine --test patterns joker_bomb_needs_all_four_jokers -- --nocapture` |
| 只看库内单测（快） | `cargo test -p gf-engine --lib` |
| 格式检查 | `cargo fmt --all -- --check`（修：`cargo fmt --all`） |
| 静态检查 | `cargo clippy --workspace --all-targets -- -D warnings` |
| 协议往返 | `cargo test -p gf-protocol` |
| wasm 端编译检查 | `cargo check -p gf-engine --target wasm32-unknown-unknown` |

**基准（`harness = false`，直接打印数字，不走 criterion）**

| 基准 | 命令 | 看什么 |
| --- | --- | --- |
| M0 地板 | `cargo bench -p gf-engine --bench m0_bench` | `m0-minimal` 单动作延迟 |
| 空转容量 | `cargo bench -p gf-engine --bench idle_1500` | 每房间常驻堆、每房间 tick CPU |
| classic 全表 | `cargo bench -p gf-engine --bench classic_bench` | `legal_actions` / `apply` 的 avg/P50/P90/P99 |
| 组牌推荐 | `cargo bench -p gf-engine --bench recommend_bench` | 1000 手随机 27 张冷调用 avg/P50/P90/P99，及是否越过 2ms 目标线 |

**黄金摘要（互相比对用，不硬编码期望值）**

```powershell
cargo run -p gf-engine --example m0_golden          # m0-minimal 整局摘要
cargo run -p gf-engine --example recommend_golden   # classic 领出态 Layout 摘要
```

**双端同解 gate（native ↔ wasm 逐值比对）**

```powershell
pwsh crates/engine/tests/wasm/verify_m0_golden.ps1
pwsh crates/engine/tests/wasm/verify_recommend_solution.ps1
```

两个脚本都会：① `cargo build -p gf-engine`；② `cargo build -p gf-engine --target wasm32-unknown-unknown --release`；③ 用 `node` 加载 `tests/wasm/*_host.mjs` 跑 wasm，与 native 的输出逐值比较，不一致即 exit 1。

---

## 6. 测试怎么组织

| 文件 | 覆盖 |
| --- | --- |
| `src/**` 的 `#[cfg(test)]` | 单元级：默认权重、tie-break、`Memo` 扩容、候选构造与枚举的同输出差分 |
| `tests/golden_test.rs` | 整局黄金轨迹（步数、`stateHash`、`finishOrder`、牌张守恒） |
| `tests/hash_view_test.rs` | `stateHash` 段集与 canonical view 编码 |
| `tests/ladder_test.rs` | 载入期合成阶梯（级牌插入、同花顺插入位） |
| `tests/patterns.rs` | 牌型全表逐条：正常 / 边界 / 非法三例，压制关系，四王炸边界 |
| `tests/recommend_test.rs` | 组牌推荐：黄金手、两种 mode 同解、同花顺花色层升级、后处理计分、王对附件 |
| `tests/ruleset_fixtures.rs` | 两份合法夹具（`m0-minimal` / `guandan-classic`）加载与语义断言 |
| `tests/ruleset_l1_gates.rs` | L1 闸门：每类非法 RuleSet 必须被拒（码表对照） |
| `tests/fixtures/**` | `guandan-classic`（13 牌型全上）、`guandan-compose`、`invalid/*`（故意造错的坏夹具） |
| `tests/wasm/*.ps1` / `*.mjs` | 双端同解与 wasm 侧基准宿主 |

**写测试的三条要求**

1. 失败要能定位：断言里带上 `mode`、`hand`、实际值（`{:?}`），不要只写「不相等」。
2. 涉及判定链的改动必须**同步**黄金用例与锚点说明，不能只改期望值了事。
3. 「等价重构」必须配**差分测试**（同输出逐条比对），口头声称等价不算数。

---

## 7. 如何加一个玩法 / 改一条规则

规则改动优先动 YAML，不动代码：

1. 复制 `crates/engine/tests/fixtures/guandan-classic/ruleset.yaml` 改造（或新增夹具目录）。
2. 用 `gf_engine::ruleset::from_yaml` 载入，L1 会逐条校验；故意造错的对照见 `tests/fixtures/invalid/`。
3. 新牌型/新阶段尽量用 `pattern.compose`（判断器组合）表达，不要新增硬编码函数；`ShapePlan` 会在载入期编译。
4. 若确实需要改设计（新增字段、改判据），**回 EGM 变更流程**并写进 `decision_record.md`，实现侧不要就地改设计稿。

---

## 8. 常见问题与排查

**1）`failed to run custom build command for gf-protocol ... Could not find protoc`**

`gf-protocol` 的 `build.rs` 要跑 `prost-build`。装 `protoc` 并让它出现在 PATH，或只对本进程指定：

```powershell
$env:PROTOC = "D:\soft\protobuf\bin\protoc.exe"
cargo build --workspace
```

**2）`cargo clippy -p gf-engine` 干净，但 `cargo clippy --workspace --all-targets` 失败**

同上，是 `gf-protocol` 的构建脚本拿不到 `protoc`，不是引擎的问题。设好 `PROTOC` 后全工作区 clippy 应 0 告警。

**3）`src/generated/gf.protocol.v1.rs` 出现 diff**

`build.rs` 每次都会把生成物写回 `src/generated/`（这是刻意的，两端共用同一份字节）。出现 diff 说明 `v1.proto` 与入库的生成代码不同步 —— **把两者一起提交**，不要手改生成代码。

**4）`error: target wasm32-unknown-unknown not found`**

```powershell
rustup target add wasm32-unknown-unknown
```

（`rust-toolchain.toml` 已声明该 target，正常情况下进目录就会装。）

**5）双端脚本报 `node` 找不到 / 版本过低**

装 Node ≥ 18。脚本用 `node <host>.mjs` 加载 wasm，Node 版本过低会因 wasm 支持不足失败。

**6）`cargo test` 全绿，但 `stateHash` 变了**

`stateHash` 变化等于判定链或状态段集变了。排查顺序：

1. 看 `crates/engine/src/hash.rs` 的段拼接顺序与字段；
2. 看是否有新字段进了状态（快照头字段如 `roundNo` **不进** `stateHash`）；
3. 现行锚点的**唯一真值在代码里**：`tests/golden_test.rs` 的 `EXPECTED_STATE_HASH` 与 `tests/wasm/verify_m0_golden.ps1` 的 `$Expected` 必须一致；`docs/开发计划.md` §0 里的哈希是历史批次记录，可能滞后于代码。若漂移是设计变更导致的，两处连同文档说明一起更新，**不许只改期望值凑绿**。

**7）`recommend` 的结果和预期不一样**

先分清是「口径」还是「实现」：

1. 权重与模式：`ai.recommend.weights` / `mode`（`weighted` 默认、`least-hands`）都在 RuleSet 里，`enabled: false` 会让 `Layout.disabled = true`；
2. 打摘要看输入是否一致：`cargo run -p gf-engine --example recommend_golden`；
3. 黄金用例在 `tests/recommend_test.rs`（`golden_user_shape_hand`、`golden_clean_hand_both_modes`），分数与手数都是逐项断言；
4. 改了搜索或后处理，必须同时看 `recommend_bench` 的 `checksum` 是否变化 —— 变了说明推荐结果真的变了，不是单纯提速。

**8）`git` 不在 PATH（部分 Windows 机器）**

用绝对路径调用，或临时加进 PATH：

```powershell
$env:Path = "C:\Program Files\Git\cmd;$env:Path"
```

**9）`powershell` 在自己的 pwsh 里找不到**

在 PowerShell 7 里不能再启动 `powershell`，用 `pwsh` 或 `& "$PSHOME\pwsh.exe"`。

**10）行尾警告 / 整文件 diff 噪音**

仓库 `.gitattributes` 固定 `* text=auto eol=lf`，二进制扩展名单独标 `binary`。若本机 `core.autocrlf=true` 造成 warning，不必改仓库设置。

**11）构建慢、磁盘占用大**

`target/` 已在 `.gitignore`；离线环境可加 `--offline`：`cargo test --workspace --offline`。

---

## 9. 开发纪律（必须遵守）

1. **设计稿不可就地改**：`design_draft.md` 已冻结，`decision_record.md` 是变更台账。发现设计缺陷 → 回 EGM 走变更，由项目方落笔。
2. **禁改清单**：`design_draft.md`、`decision_record.md`、`reviews/**`、`review_packet.md` / `manifest.json`、`concepts_brief.md`、`handoff_boundary.md`、已发布迁移、生产配置与密钥。`docs/开发计划.md` 由项目方维护，实现侧只读、不代填进度、不改判据。
3. **确定性纪律**：判定链路禁用浮点、禁用依赖 `HashMap` 迭代顺序、禁用系统时间/随机数/文件与网络 IO；集合输出必须显式排序。计时只允许出现在 `benches/`。
4. **提交纪律**：中文 Conventional Commits，标题 ≤ 72 字符；`git add` 必须带明确路径，**禁 `git add -A` / `git add .` / `git commit -a`**；提交前 `git show --stat` 自查；禁 force push。
5. **等价重构要带差分测试**：输出必须逐条相同，差异即视为语义变更。
6. **性能口径不改凑线**：容量按 `rooms = 1e6 / (20 × avg_action)`（10Hz × 2 动作/秒/房间）如实报数，同时给「单核 / 核数 / 折算整机 / 机型」，未达 1500 房间整机线就写未达。

---

## 10. 目录结构

```
guandan/
├─ Cargo.toml                 # workspace：7 个共享 crate + 1 个后端服务
├─ rust-toolchain.toml        # stable + rustfmt/clippy + wasm32-unknown-unknown
├─ crates/                    # 共享库（不含 IO、不含服务）
│  ├─ engine/                 # 规则引擎（核心）
│  │  ├─ src/                 # contract / engine / ruleset / patterns / hash / view / settlement / recommend
│  │  ├─ tests/               # 黄金、牌型全表、L1–L4 闸门、推荐、fixtures、wasm 脚本
│  │  ├─ benches/             # m0_bench / idle_1500 / classic_bench / recommend_bench
│  │  └─ examples/            # m0_golden / recommend_golden / classic_golden（L4 黄金生成）
│  ├─ protocol/               # proto + prost 生成代码（入库）；服务端与 wasm 共用
│  ├─ table-runtime/          # 房间主干（建桌/发牌/行动窗口/托管/断线恢复/续局重演）+ 快照 wire
│  ├─ ai/                     # AI-L0/L1/L2（view(seat) + 合法集 + partner.state）
│  ├─ client-wasm/            # 浏览器 wasm 绑定层（零 import，裸加载）
│  ├─ deckart/                # 牌面主题工具包（矢量素材 → 54 面 SVG / .gfdeck 包）
│  │  ├─ themes/              # 主题源
│  │  ├─ pkgs/                # 打包产物 *.gfdeck
│  │  └─ scripts/             # pack-all.ps1
│  └─ runtime-spike/          # M0 断线恢复原型（只剩 harness 夹具）
├─ services/                  # 后端服务（进程、IO、网络）
│  └─ server/                 # gf-server：axum + tokio，protobuf over WS（根 workspace 成员）
├─ apps/                      # 面向用户的端
│  ├─ web/                    # 浏览器端（纯静态站点 + IndexedDB 存档 + 杀进程续局）
│  └─ desktop/                # Tauri 2 桌面壳（独立 workspace，打包时复用 apps/web）
├─ scripts/                   # 仓库级脚本：verify_dual_hash.ps1、coverage.ps1
├─ .github/workflows/ci.yml   # 单元 + 集成、双产物 stateHash 同解、Web 续局 gate
├─ docs/
│  ├─ 开发计划.md              # 排期、任务分解、验收勾选（项目方维护）
│  ├─ designs/2026-11-23-guandan-multiplatform/
│  │  ├─ design_draft.md      # 设计稿（冻结，唯一权威）
│  │  ├─ decision_record.md   # 变更台账（第一…第三十七次变更）
│  │  ├─ handoff_boundary.md  # 交接边界与禁改清单
│  │  └─ ...                  # reviews / 概念简报 / 评审包
│  └─ assets/                 # m0 推演、M1 实测 profile、backlog 等实测资产
└─ target/                    # 构建产物（不入库）
```

分层规矩（评审时按这条卡）：`crates/` 只放可复用的库，不碰文件系统/网络；`services/` 放会起进程、连外部依赖的服务；`apps/` 放交付给用户的前端与壳。新增东西先归位，不往仓库根堆。

---

## 11. 路线图

| 阶段 | 状态 | 内容 |
| --- | --- | --- |
| M0 | ✅ 已收口 | 契约锁定、双端 `stateHash` 一致、断线恢复原型、非法 RuleSet 反证、空转与包体实测 |
| M1 | ✅ 已收口 | 牌型全表、枚举路径重构、结算两批、进贡/还贡/抗贡、跨局携带、快照 wire、组牌推荐、房间主干（含计时/托管/断线恢复/再来一局）、AI-L0/L1/L2（跟牌/留牌/记牌/队友配合）、Web 端（牌桌/大厅/结算/设置）+ IndexedDB 续局、校验器 L2–L4、CI 双产物 gate、性能与容量 gate（353.3µs / 3,952–7,904 房间）、接风与进贡后手牌刷新专项；覆盖率门禁按 **行 ≥90% 且区域 ≥90%**（实测行 92.23% / 区域 91.61%，函数 89.95%，分支 85.05% 作观测项），口径与缺口见 `docs/assets/m1-coverage-baseline.md` |
| M2 | 🟡 进行中 | 已落地：`services/server` 联机骨架（入座/开局/动作/结算）、传输帧头（`gf_protocol::wire`）、快照 `roundNo` 补齐、增量重连（`ReconnectBuffer` → `RoomDelta`，跨局/结算落快照重置）、房间码与匹配队列（内存态）、手机号账号与短信验证码（测试白名单 + 座位绑定）、PG/Redis 落库（战绩/账号/快照/动作流/队列镜像）、回放包导出（`GET /api/replays/{matchId}`，服务端重放自证）与离线回放器 `tools/replay-viewer`；待做：第 24 周联机内测、桌面壳三端产物、回放只读观战 |
| M3 / M4 | 未开始 | 见 `docs/开发计划.md` |

**M1 目前的硬账**

- 单动作延迟：`guandan-classic` 领出态 `legal_actions` P99 **353.3µs（native）/ 359.7µs（wasm）< 1ms**，已过线；`apply` P99 4.2/4.9µs。
- 容量：单核标度 native ≈494 / wasm ≈484 房间；整机折算（i7-13700KF，8P/16 物理核）≈ **3,952–7,904 房间**，已到 1500 整机线（口径见 `docs/assets/m1-bench-profile.md` §10；单核数只作标度，不得单独判过或判不过）。
- `recommend`：1000 手随机 27 张冷调用 avg **315.4µs** / P99 **928µs**，未越 2ms 目标线。
- 无人值守全流程：四 bot（AI-L2）两局 **185 动作**，终局 `stateHash` `55728d55…ef5c0`，native 与 wasm 逐字节同解；Web 端每手动作落 IndexedDB，刷新/杀进程后按日志回放，哈希一致（无头 gate 242 条 / `17345104…7bd39`）。
- bot 手组牌纪律：出牌按 `recommend` 的最优整手拆分走——领出取牌力最小的整组（散张同台比较），跟牌不拆组；`crates/ai/tests/bot_plan_discipline.rs` 在一整局里逐手核对，实测 **73 手 0 拆组 / 0 领出偏离 / 0 非法退化**。
- 覆盖率：`scripts/coverage.ps1`（`cargo-llvm-cov` + nightly，排除 wasm 导出层 / 生成代码 / 测试辅助构造器 / 性能原型）实测 **行 92.23% / 区域 91.61%**（函数 89.95%，分支 85.05% 作观测项）；门禁判据为行与区域 ≥90%，口径与剩余缺口见 `docs/assets/m1-coverage-baseline.md`。

细节与实测账目见 `docs/assets/m1-bench-profile.md`。

---

## 12. 文档地图

| 想了解 | 看 |
| --- | --- |
| 规则 / 契约 / 协议 / 判据 | `docs/designs/2026-11-23-guandan-multiplatform/design_draft.md` |
| 每条设计决策的来历、作废项 | 同目录 `decision_record.md`（按「第 N 次变更」索引） |
| 排期、任务、验收勾选 | `docs/开发计划.md` |
| 交接边界、禁改清单 | 同目录 `handoff_boundary.md` |
| M1 性能实测与分段拆账 | `docs/assets/m1-bench-profile.md` |
| M0 纸面推演与缺口 | `docs/assets/m0-paper-walkthrough.md`、`m0-gap-backlog.md` |
| 素材授权 | `docs/assets/license.md`（待补） |

---

## 13. 联机服务端（`services/server`，M2）

根 workspace 成员（`gf-server`）：同仓同锁、`cargo fmt/clippy/test --workspace` 都会管到它；只想动服务端就用 `-p gf-server` 收窄，覆盖率统计仍按 crate 分开算。桌面壳 `apps/desktop` 引 Tauri 依赖树太重，保持独立 workspace。

```powershell
# 起服务（默认 127.0.0.1:8080）
cargo run -p gf-server --bin gf-server

# 测试（29 条单测 + 6 条房间闭环 + 6 条大厅 HTTP + 6 条账号 HTTP + 1 条落库 + 2 条 WS 冒烟）
cargo test -p gf-server
```

环境变量：`GF_BIND`、`GF_MIN_CLIENT_VERSION`（空串表示不校验）、`GF_TICK_MS`（默认 50）、`GF_NEXT_DEAL_MS`（结算后自动续局间隔，默认 5000）、`GF_HEARTBEAT_TIMEOUT_MS`（默认 30000）、`GF_ROOM_CODE_TTL_MS`（房间码有效期，默认 24h）、`GF_MATCH_TIMEOUT_MS`（匹配等待上限，默认 30s）、`GF_AUTH_SALT` / `GF_JWT_SECRET`（手机号哈希盐与 token 签名密钥，默认值是开发串、启动会告警）、`GF_SMS_TEST_WHITELIST` / `GF_SMS_TEST_CODE`（短信未报备前的测试白名单与固定码）、`GF_REQUIRE_AUTH`（强制玩家侧接口与 WS 带 token）、`GF_DATABASE_URL` / `GF_REDIS_URL`（PG/Redis 连接串，不填即纯内存运行）。

**接口**

| 端点 | 说明 |
| --- | --- |
| `GET /healthz` | 存活与房间数 |
| `POST /api/rooms` | 建房，body `{"match_id":"..."}`（本机联调用，保留） |
| `POST /api/room/create` | 建房出码，body `{gameId, mode, seatsCount}` → `{matchId, inviteCode, matchType}` |
| `POST /api/room/join` | 凭码预占座位，body `{inviteCode}` → `{matchId, seat, accept, availableSeats?, reason?}` |
| `POST /api/match/queue` | 入队，body `{gameId, mode}` → `{ticketId}`（满四人即成房） |
| `GET /api/games` | 公开玩法列表；`?clientVersion=` 或 `X-Client-Version`，版本不足仍返回 200 + `upgrade` 提示 |
| `GET /api/match/queue/{ticketId}` | 轮询 `{state: waiting\|matched\|timeout, matchId?}` |
| `DELETE /api/match/queue/{ticketId}` | 退队 `{ok}` |
| `POST /api/auth/sms` | 发验证码 `{phone}` → `{requestId, cooldownSec}`；超 60s/10 次日限回 429 `RATE_LIMITED` |
| `POST /api/auth/login` | `{phone, code, clientVersion}` → `{token, refreshToken, expiresIn}`；版本不足 426、码错 401 |
| `POST /api/auth/refresh` | `{refreshToken}` → `{token, refreshToken, expiresIn}`；旧 refresh 即刻作废 |
| `GET /api/profile` | `Authorization: Bearer` → `{playerId, nickname, phoneTail4, stats}` |
| `GET /ws` | 牌桌 WS；`?token=` 可选（`GF_REQUIRE_AUTH` 时必填） |

**传输帧头**（`gf_protocol::wire`，§4.1 D5「固定自研」的传输层实现，不是 §5.9 消息契约）

```
[ kind: u16 大端 ][ protobuf body ]      一帧一条消息；不设长度前缀（WS 自带边界）
C→S  1 JoinRoom | 2 Reconnect | 3 SubmitAction | 4 Heartbeat
S→C 11 JoinAck | 12 RoomSnapshot | 13 RoomDelta | 14 ActionAck | 15 PhaseChange | 16 SettleResult | 17 TrusteeState
```

**已实现的语义**

- 满四人发牌；`JoinAck` 必先于快照（新座位不会把快照当第一帧）。
- 动作：`kind` 与 `oneof` 分支双向校验、`requestId` 幂等（同 id 重放回同一份回执、不推进 `roomSeq`）、`seatSeq` 低于水位回 `STALE_SEQ`、请求 `seat` 与绑定座位不符回 `AUTH_INVALID`。
- 结算：`SettleResult.result` 走 canonical JSON（键序 `winner_team, ranks, points`，单行、无空白、不写 `null`）；同一局只广播一次。
- 断线：WS 断开/30s 无消息即记 `Offline`，房间按宽限托管；重连优先补发 `RoomDelta` 增量（水位在 200 条缓冲内且未跨局、未落在结算空窗期），末尾附 `snapshotReset=false` 快照；其余情形只发 `snapshotReset=true` 全量快照。
- 服务端自证：`RoomHost::state_hash()` 与 `replay_hash()` 必须相等，四座位整局用例已锁。
- 大厅：房间码 6 位（去易混字符、大小写不敏感、24h 有效）、凭码预占座位（`reserve()`，60s TTL）；匹配队列按 `(gameId, mode)` FIFO 分池，满四人撮合成同一场次，v1 不推 WS 匹配事件。
- 账号：手机号 + 短信验证码登录；手机号只存 `HMAC-SHA256(盐, 号)` + 尾号 4 位；access 是 15 分钟 HS256 JWT，refresh 30 天单端轮换；短信通道未批前 `GF_SMS_TEST_WHITELIST` 走固定测试码；带 token 的预占与入座把座位绑定到玩家，重连校验同一玩家。

**本切片明确没做**（按开发计划 §3 M2 顺序）：结算重试与死信补偿、回放导出与观战、桌面壳三端产物、第 24 周联机内测。

**三个待裁口径**（代码里留了 TODO 常量）

- `M2-OPEN-01`：联机「再来一局」由谁触发（会动 `actions.kind` 闭集）——当前服务端定时续局。
- `M2-GAP-proto-04`：玩家误点被引擎拒绝时 `ActionAck.reason` 无 §5.7 表内码——暂用 `ILLEGAL_ACTION` 占位，裁决后替换。
- `M2-GAP-05`：邀请码/票号不存在在 §5.7 表里无对应码——暂用 `RS_SCHEMA_INVALID` + `detail`（`INVITE_CODE_UNKNOWN` / `TICKET_UNKNOWN` 等），裁决后替换。


