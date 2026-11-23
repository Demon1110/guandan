# 掼蛋多端游戏框架 · 设计草案

| 项 | 内容 |
| --- | --- |
| 设计标识 | 2026-11-23-guandan-multiplatform |
| 对象类型 | technical |
| 版本 | v1 |
| 建档时间 | 2026-11-23 |
| 状态 | draft |

> 逐节写，每写完一节停下等人反馈；不要一次性甩出全文。
> 每个判断要么给依据，要么标成显式假设（A1、A2……）。

## 1. 背景与目标

### 1.1 问题

一个想长期运营的掼蛋产品，要在 6 类终端上提供"联机 + 离线 bot + 可扩展玩法"，但现状是三选二：多端方案牺牲离线、可扩展方案牺牲规则一致性、单端方案牺牲覆盖面。结果是每加一个平台或玩法，就重做一遍牌桌与联机层，维护成本随平台数 × 玩法数相乘。

### 1.2 已知前提（C1–C9 已由用户确认，U1–U10 为本草案阶段拍板）

| 前提 | 结论 | 来源 |
| --- | --- | --- |
| 产品定位 | 内测起步，按商店上架标准的**架构**设计；**版号暂不申报**，首发只做 Web/H5 + 桌面自主分发 | C1，由 U5、D18 修订 |
| 规则口径 | 默认基线 + 四条核心争议全配置化，不绑定地区或平台 | C2 |
| 技术栈 | Rust 引擎 crate（wasm + native 双产物）；服务端 Rust；不用 Java | C3 |
| 客户端形态 | 单套 Web 渲染（Vue 3 + PixiJS 8）；桌面壳 Tauri 2 + 浏览器直跑；**移动壳押后** | C4，由 U5 修订 |
| 账号 | 内测即提供手机号账号，战绩与存档云端、跨端一致 | C5 |
| 素材 | 开源素材，CC0 优先；有署名要求但允许商用的（如 OFL 字体）可收，但必须随分发保留许可文本 | C6；本条按评审意见放宽，`../../assets/license.md` 的收录规则同步为同一口径 |
| 后台表达力 | **零代码上新玩法**：运营者不发版、不改代码即可配出新玩法 | C7 |
| 零代码实现路径 | **通用规则 DSL**（原语组合 + 声明式配置，不允许自定义逻辑） | C8 |
| 编辑器形态 | 积木为主视图 + 源码模式高级视图，两者读写同一份 RuleSet | C9 |
| 分期 gate | 第 16 周 Web 端 + 本地 bot 能打完整场（不含联机）；第 24 周联机内测 | U1 |
| 玩法范围 | v1 只做掼蛋与斗地主；麻将留 v2，原语保留但不验证 | U2 |
| 租户模型 | 单租户、多玩法，不做多租户隔离 | U3 |
| AI 口径 | 三档（简单/普通/困难）+ UI 标注"AI 陪练"；人机房不计段位与正式战绩 | U4 |
| 变现 | v1 **留支付占位不收费**：订单表与支付回调接口先建，不接真实渠道 | U7 |
| 观战 | **只做回放式观战**，不做实时旁观 | U8 |
| 部署区域 | 国内单区；域名与 ICP 备案为 M0 前置项 | U6 |
| 工期基线 | 承诺 34～38 周，按 M0–M4 五段拆分（见 §2.4） | U9 |
| 规则取值 | 由设计者按淮安主流打法起草《规则取值表》，用户逐条确认后冻结 | U10 |

### 1.3 目标

| # | 目标 | 判定依据 |
| --- | --- | --- |
| G1 | v1 四个产物（Web 静态站 + Windows/macOS/Linux 桌面壳）都能完整打完一场掼蛋 | S2 |
| G2 | 4 人联机可用，含断线重连与超时托管 | S3、S4、S9 |
| G3 | 完全离线也能与 bot 打完一场 | S2 的离线分支 |
| G4 | 新增斗地主只写玩法，不动框架核心 | S5 |
| G5 | 内测版的架构等于商店版：支付、实名、防沉迷只改服务端与壳层 | S7 |
| G6 | 平台化：后台零代码配出新玩法并上线，全程不发版 | S12 |

### 1.4 成功判据

本节自带全部判据的判定语句，不依赖外部文档；编号与概念简报一致，S1 已按 Rust 单栈改写。

| # | 判据 |
| --- | --- |
| S1 | 同一份 `crates/engine` 产出的 `wasm32-unknown-unknown` 与 native 两个产物，跑同一批 action log，`stateHash` 逐字节一致；CI 100% 通过。**唯一例外**：注入 panic 的专门用例只要求"native 捕获并返回 `RS_INTERNAL_PANIC`、wasm 按 §8.6 的 abort 行为记录"，不要求两端同哈希——该用例标记 `panic-injection` 并从 S1 的分母里剔除，其余用例任何一条不一致即 S1 不成立 |
| S2 | v1 四个产物（msi / dmg / AppImage / 静态站）在断网环境下完成一局：入座建桌（场级前置动作）→ 发牌 → 出牌 → 结算，四端各至少 1 次人工验收记录。本判据验收粒度为一局；同场"再来一局"的连续流程见 §2.4 第 16 周 gate |
| S3 | 断网 5 秒后重连，客户端状态与服务端一致（`seatSeq` 与 `viewHash` 校验通过），可继续出牌 |
| S4 | 4 人真实联机连打 30 局，0 次错误判定（合法动作被拒 / 非法动作被接受）、0 次状态漂移；玩家误点被引擎拒绝不计入本判据分母，本判据与 §7.2 的非法率告警口径分开 |
| S5 | 新增斗地主时，`crates/table-runtime` 与 `apps/server` 的改动不超过各自总行数的 10%，框架层单测全绿。**注**：S5 是"代码增量"的验收上限，G6／S12 是"零代码配规则"的验收，两者针对不同交付物，不互相替代 |
| S6 | 由 A4 推出的峰值（并发房间 × 3 倍余量）压测：动作回执 P99 < 100ms，进程内存 24 小时无持续增长 |
| S7 | 接支付、实名、防沉迷时只改 `apps/server` 与壳层，`crates/engine`、`crates/table-runtime` 零改动 |
| S8 | 四条争议（同花顺与炸弹的序 `flushRank`、逢人配能否组同花顺 `inStraightFlush`、过 A 条件 `passLevel`、抗贡条件 `antiTribute`）可配置切换，双口径回归全绿 |
| S9 | 同一手机号账号在 Web 与桌面端登录，战绩与段位一致；断线 10 分钟内重登可恢复到该局 |
| S10 | `stateHash` 有唯一算法定义（§5.12：字段范围、序列化规范、哈希函数、是否含规则版本），两端实现由同一份 Rust 代码产出 |
| S11 | S8 的四条配置各有 ≥3 条黄金用例，任一配置变更触发重跑并给出差异报告 |
| S12 | 运营者不改代码、不发版，只在后台完成"选玩法族 → 配规则 → dry-run → 灰度 → 全量 → 回滚"，配出一款**族内**新玩法（斗地主），全程不重启服务、不更新客户端包。跨族（麻将）不进 S12，见 R13 的纸面推演结论 |

### 1.5 显式假设

正文以 `A` 编号引用假设时，均指向本表；表外的编号不存在。

| # | 假设 | 若不成立的影响 | 验证方式 |
| --- | --- | --- | --- |
| A1 | Rust 单栈可实现且可维护（异步、wasm 构建链由同一人掌握） | 回退"引擎 Rust + 服务端 Node 跑 wasm"或整体降级 TS 引擎 | M0 做一次 Rust 服务端 ↔ wasm 客户端端到端原型，记录编译与调试耗时 |
| A2 | 规则采用"默认基线 + 争议项配置化"即可覆盖内测玩家口径 | 若某口径是结构性差异（如进贡流程本身不同），配置表达不了，引擎返工 | M1 用 12 条歧义清单定默认值并做双口径回归 |
| A3 | 不投专职美术，卡面音效全部用开源素材 | 观感不够需在 M3 前补视觉（+2～3 周）；授权不合格会被审核拦 | M1 建立 `../../assets/license.md` 并归档许可原文 |
| A4 | 首年 DAU < 1 万，单机 + 少量实例足够 | 超出后需分区分服，协议与房间寻址要改 | M3 压测给出基线 |
| A5 | 内测即手机号账号，不接第三方登录与支付 | 短信通道受阻会阻塞 M2；要微信登录需开放平台资质，+2～3 周 | 已确认（C5） |
| A6 | Web/H5 是首发验证端，桌面壳可后置到 M2 | 若首日就要安装包，M1 需并行投入壳工程，+2 周 | 由 C4 与 U5 定的节奏 |
| A7 | 仓库为空，无既有代码与历史包袱 | 有原型代码则需重估选型与迁移成本 | 已查证（§3.1） |
| A8 | 单房间 4 连接，无高频广播需求 | 若做实时旁观并发放大，需房间级扇出优化 | 实时旁观已被 U8 移出 v1，本假设只约束回放式观战 |
| A9 | 版号不在本期关键路径，首发走 Web/H5 + 桌面自主分发 | 若变现只能靠商店渠道，缺版号会直接压住收入 | 已由 U5 取代原假设；每季度看一次渠道机会成本 |
| A10 | 桌面可自主分发、Web 可直接访问，无需商店审核 | 若分发动线要求资质，首发渠道要重排 | M0 核查域名与 ICP 备案政策 |
| A11 | 同一 crate 编出的 wasm 与 native 在相同输入下产生相同 `stateHash` | 两端分叉，联机判定与本地 bot 会不一致，严重时退回单一运行时 | CI 跑双产物比对（S1） |
| A12 | 移动端走 WebView 壳，引擎统一 wasm，不需要原生绑定 | 已由 C4（单套 Web 渲染）消除，本假设在当前范围不生效 | 无需验证，v1 不做移动壳 |
| A13 | 回放式观战足以满足"看高手怎么打"的需求，不做实时旁观也不明显影响留存 | 若观战是核心留存手段，需补实时旁观（房间扇出 + 协议 + 3～4 周） | 内测问卷 + 回放观看次数，M3 复盘；不成立则进 v2 backlog |
| A14 | 通用规则 DSL 能表达目标玩法全部规则结构 | 表达不了（如麻将鸣牌优先级、掼蛋进贡链）只能降级"模板 + 参数"，工期回落但零代码上限降低 | **M0 末纸面推演**：只用 RuleSet（不实现代码）写出斗地主与麻将最小对局，写不出来的结构逐条列出是否需新原语；实现级验证排在 M3 接入斗地主时 |
| A15 | 2 人能在 M0 内跨过 Rust 异步服务端门槛 | 若 M0 末房间并发与断线恢复原型跑不通，基线整体顺延 3～4 周或缩范围 | M0 交付"空转压测"与"断线恢复"两个原型并记录实测 |
| A16 | 解释执行开销可接受：单次动作解释 < 1ms，单核承载 ≥1500 房间（= §3.2 的 3 倍余量基线） | 需引入预编译缓存（+3～4 周），热更新复杂度上升 | M0 基准：同一 RuleSet 在双产物下各跑 10 万次 `legal_actions` 与 `apply`，记录 P99 |
| A17 | 编辑器以积木为主视图，源码模式只作研发高级视图，两者读写同一份 RuleSet | 若源码成为主入口，语法错误会进生产 | 已确认（C9）；M0 出交互草图与"非法结构不可构造"验证 |

### 1.6 术语

同一份文档里最容易打架的四个量词，先在此定死，后文不再换义：

| 术语 | 定义 | 对应标识 |
| --- | --- | --- |
| 场（match） | 一次组局，从入座到所有玩家离桌；一场含若干局 | `matchId` |
| 局（round） | 一局牌，从发牌到结算；掼蛋一局可能打过 A 后继续 | `roundNo` |
| 轮（turn） | 一局内的一次行动权交接，是 `play` phase 内的轮次子阶段，**不是** phase | 不进 ID，只用于"轮转"描述 |
| 端（client build） | v1 四个：Web 静态站、Windows、macOS、Linux；移动两壳与"六端"是 v2 判据 | — |



## 2. 范围与非目标

### 2.1 本次设计覆盖的交付物

| 交付物 | 内容 | 阶段 |
| --- | --- | --- |
| `crates/engine` | 原语库（牌/区域/牌型/轮转/行动仲裁/结算/计分/确定性 RNG/视图裁剪）+ RuleSet 解释器 + 静态校验器；原语由 Rust 实现，RuleSet 只能组合原语 | M0–M1 |
| `crates/table-runtime` | 对局生命周期：入座、发牌、行动窗口、计时、托管、断线恢复、回放录制 | M1–M2 |
| `crates/protocol` | proto 定义与 Rust 类型生成，服务端与 wasm 客户端共用 | M0 |
| `crates/ai` | 与玩法无关的搜索/评估原语；掼蛋三档策略：`easy`→实现层 L0、`normal`→实现层 L1、`hard`→L1 加搜索（映射见 §5.11） | M1 |
| `apps/server` | 账号、短信验证、大厅、匹配、房间托管、战绩、**回放式观战**、回放查询、积分/段位/排行榜；玩法注册表与规则配置版本；支付、实名、防沉迷只留适配接口（U7：订单表与回调先建，不接真实渠道） | M2–M3 |
| `apps/admin` | 运营后台（Vue 3 + 同一套组件库），交付范围以本行清单为唯一定义（§2.2 引用此处，不另写件数）：①玩法注册；②规则配置（schema 生成表单 + 积木/源码双视图）；③dry-run 校验；④灰度/全量发布；⑤回滚；⑥审计日志；⑦房间与账号巡检；⑧踢人/封号、人工改分、结算死信人工补写；⑨手动发码、导出回放、后台账号管理 | M3 |
| `apps/client` | 单套 Vue 3 + PixiJS 渲染，含大厅、牌桌、结算、设置、回放播放器；含本地存档层（**仅纯本地 bot 局**用；载体、内容、写入时机、恢复校验与上限见 §5.9 末行；联机局不写本地存档） | M1–M3 |
| `apps/shell-desktop` / `apps/shell-mobile` | Tauri 2（Win/Linux/macOS）先在 M2 做；Capacitor 6（Android/iOS）**押后**（U5：版号暂缓，商店端不进关键路径） | M2 / 押后 |
| `tools/replay-viewer` | 离线重放器：读导出包复现整局，是故障排查与 S1 比对的实际执行者 | M2 |
| `tools/rule-authoring` | 规则编辑器核心（积木树 ↔ 源码文本双向转换、结构合法性校验），被 `apps/admin` 复用而非另写一套 | M3 |
| 玩法 | 掼蛋完整；斗地主完整可玩（用来验证 DSL 表达力与"零代码上新"）；**麻将不进 v1** | M3–M4 |
| 运维能力 | 结构化日志、房间指标、`stateHash` 不匹配告警、一键导出回放 | M3 |
| 合规技术面 | 实名认证与防沉迷的适配层（接口占位 + 开关）。触发点由"上架前"改为"商业化立项"：U5 把商店端移出计划后，"上架前"不再是计划内的时间点 | M3 占位 |

### 2.2 首版（内测）明确不做

- **收费**：充值、内购、皮肤、广告一律不做（U7）。但要**留占位**：`order`、`payment_callback`、`refund` 三张表与回调接口先建好（字段见 §5.8），不接真实渠道；这样日后接渠道只改 `apps/server`，符合 S7。
- **赛事、赛季、战队、公会、排行榜赛季结算**：不做（积分、段位、总榜要做，见 §2.1 的排行与段位行）。
- **实时语音、3D 牌桌、直播推流**：不做。
- **实时旁观**：不做；只做回放式观战（U8），即只能看已结束房间的回放，不给房间增加广播扇出。
- **好友与私聊**：不做；组局靠房间码。
- **后台只做最小管理台**：交付项与鉴权范围**只以 §2.1 `apps/admin` 行的九件清单为准**（本节引用该处，不再另列件数）；不做 BI 报表、客服工单、活动运营工具。这是对用户"简单的运营后台界面"的落地定义。
- **自动财务对账与冻结**：不做自动冻结结算修正；只出对账报表供人看（首版无值班团队，冻结了没人解冻，见 §7.5）。
- **人工反作弊审核流**：不做；只做异常打点、告警与回放留痕，人工判定在离线进行。
- **麻将**：v1 完全不碰（含最小对局）。`window`（多座位行动窗口）与 `arbitrate` 原语**不进** v1 的 Primitive ADT、`lifecycle` 相位枚举与协议说明；v2 立项前先做"麻将最小对局"纸面推演，确认需要后按 engine 次版本加回（§9.2，见 R13）。
- **多语言**：不做，只做中文（U6）。**皮肤系统**：只做深色/浅色两套主题，不做主题市场与用户自定义皮肤。

### 2.3 不在本设计范围内

以下事项与本设计并行存在，但不由本设计决定，避免评审时把"设计缺陷"与"项目没做"混为一谈：

- 视觉规范与素材采购（开源素材清单由实现阶段归档，`A3`）
- 版号与备案的材料撰写与提报（`A9` 只约束时间是关键路径，不描述流程）
- 服务器采购、域名备案、短信服务商开户
- 法务与版权审查
- 牌桌的交互稿与动效参数（属于视图层规格，另行成文）

### 2.4 阶段边界（防止范围在里程碑间渗漏）

| 阶段 | 周次 | 允许进入的东西 | 明确推迟 |
| --- | --- | --- | --- |
| 前置 | 第 0 周前 | 域名、ICP 备案、短信模板报备、开源素材授权清单启动 | 这些不占人周，但任一延期都会把 M0 起点后移 |
| M0 | 第 1–6 周 | 引擎契约与原型、`crates/protocol`、确定性纪律、双产物 CI、空转压测与断线恢复两个原型 | 掼蛋完整规则、账号服务、UI |
| M1 | 第 7–16 周 | 掼蛋规则、AI L0/L1、客户端牌桌、**Web 端 + 本地 bot 可完整打完一场（第 16 周硬 gate，不含联机）** | 原生壳、账号服务、联机、观战 |
| M2 | 第 17–24 周 | 账号与短信、房间、匹配、断线恢复、桌面壳打包、回放重放器；**第 24 周联机内测** | 支付渠道、移动壳（押后）、社交 |
| M3 | 第 25–32 周 | 回放式观战、战绩与段位、告警、合规适配层占位、**运营后台与玩法配置发布链**、斗地主接入 | 变现渠道、赛事 |
| M4 | 第 33–38 周 | 平台化收尾：斗地主零代码上线演练、发布链压测、文档与交接 | 麻将（v2）、移动壳（押后） |

五段周次首尾相接、不重叠：6 + 10 + 8 + 8 + 6 = 38 周是日历跨度上限；34 周是"每段都能压到下限并复用 M0 骨架"的乐观值。所以口径是 34～38 周，不是"38 减重叠"。任一阶段延期，要么从后续阶段同等工作量里扣，要么整体顺延并重报总工期，不允许只改一段而不重算总和。

**gate 口径**：第 16 周的"完整打完一场"指 建桌 → 发牌 → 出牌 → 结算 → 再来一局 全流程跑通（验收粒度同 S2：一局连续跑通，"再来一局"为同场延续）。断线/崩溃的验收按模式拆成两条，各自挂到适用的阶段：

- **纯本地 bot 局**（wasm 引擎跑在客户端、无服务端判局）：**杀死客户端进程后重开可续局**——本条是第 16 周 gate 的组成部分（本地存档的载体、内容、写入时机、恢复校验与上限见 §5.9 末行）。
- **联机局**（含服务端 bot 房）：**断网 90 秒内可重连继续**——按 §6.2 的宽限期与快照重连口径（`snapshotReset`），客户端不写本地存档、不承诺离线续局；本条随 M2 第 24 周联机内测 gate 验收（第 16 周 gate 仍不含联机、不含账号）。

**合规 gate（U12）**：公网对外开放（不再限于内测白名单）之前，必须拿到"无版号免费内测是否需要实名与防沉迷"的书面答复；答复为"要接"则 `D_compliance`（实名 + 防沉迷接入完成）成为 M3 的硬 gate，未完成不得开放公网注册。M3 的"合规适配层占位"就是为这条留的位置。

**范围冻结规则**：内测开始后新增需求一律进 backlog，不插入当前里程碑；确需插入时，必须同步删掉同等工作量的既有项，并在裁决台账登记。
  依据：U9 已把基线定为 34～38 周 × 2 人，没有余量吸收插入式需求（`A1`、`A15`）。


## 3. 现状与约束

### 3.1 现状（已查证，不是推测）

| 项 | 事实 | 查证方式 |
| --- | --- | --- |
| 代码资产 | `D:\java\git\guandan` 在 2026-11-23 建库时为空目录、0 个文件、连 `.git` 都未初始化；此后已建档 `docs/designs/` 与 `docs/assets/`，仍**无任何源码** | `Test-Path` + 条目计数（2026-11-23） |
| 既有系统 | 无。没有旧接口、旧数据、旧调用方需要迁移 | 同上 |
| 开发环境 | Windows 11 + PowerShell 7；另有 Linux 云主机可用于构建与部署 | 当前执行环境信息 |
| 前端经验 | Vue 3 + Vite + Tauri + Capacitor 均为已用过的栈（WinWidget 等项目） | 用户画像与既往项目 |
| 服务端经验 | Rust 异步与游戏服是**新领域**，此前项目以桌面壳与插件为主（`A15`） | 用户画像推断，待实现阶段验证 |
| CI/CD | 无，需从零建（GitHub Actions 可用） | 仓库为空 |

### 3.2 量级假设（用于压测目标与容量设计，`A4`）

| 指标 | 取值 | 依据 |
| --- | --- | --- |
| 首年 DAU | < 1 万 | `A4` |
| 同时房间数目标 | **由 DAU 推出，不写死**：峰值在线（DAU × 20% 晚高峰）÷ 4 人 ≈ 500 房间，压测取 3 倍余量 = 1500 房间 | 推导见本行；`A4` 的 DAU 上限是 1 万 |
| 单房间连接 | 4（回放式观战不占实时连接，U8） | 掼蛋 4 人 |
| 单局动作数 | 100～300 次 | 掼蛋一付牌的典型量级，实现后以实测校正 |
| 单动作消息体积 | 出牌约 50～200 字节（protobuf） | 4 张牌 ID + 元信息 |
| 单座位行动频率上限 | **≤0.2 次/秒**（相邻两次行动权交接至少 5 秒，这是 §6.3 的 30 秒思考 + 3～8 秒典型节奏共同决定的上界；0.2 是"满负荷"假设，实际约 0.1） | 由"单步超时 30 秒、典型思考 3～8 秒"推出；M2 实测校正 |
| 峰值消息率 | 1500 房间 × 4 人 × 0.2 次/秒 ≈ **1200 msg/s** | 上一行 × 上行；按 1200 设计，不再按 5000 设计 |

按此量级，瓶颈不在吞吐（1200 msg/s × 200B ≈ 0.24 MB/s），而在**正确性**（判定一致、断线恢复、配置发布）与**四端打包**。

### 3.3 硬约束（实现阶段不能突破的）

1. **语言集合**：Rust（引擎 + 服务端）、TypeScript/Vue（客户端 + 后台）。不引入第三语言；不引入 JVM、不引入 Go、不引入 Lua。
2. **引擎确定性纪律**（`crates/engine` 内）：不得出现 `SystemTime::now()`、`rand::thread_rng()`、文件 I/O、网络 I/O、`HashMap` 迭代顺序依赖（用 `BTreeMap` 或显式排序）；**判定链路禁用浮点**（牌力与分值一律整数，比率只在展示层算）；**禁用 std 超越函数**（`pow/sin/log` 之类进入判定即视为违规，需要时改为查表）；**序列化必须按显式字段序**，不得依赖 struct 声明顺序或 map 遍历；**所有排序必须是全序**（主键相同再比唯一 id），否则等价元素在两端排出的顺序不同。
   依据：S1/S10 要求双端逐局同哈希，上面每一条都会在两端产出不同结果。
3. **信息隔离**：引擎内部必须持有完整牌局状态（发牌、合法性判定、结算都要它），但对**外**只开 `view(seat)`；凡是跨进程、跨端、跨房间边界传输的状态，一律是裁剪后的视图，服务端不下发其他座位的手牌。（本条替代原稿"不得构造全量手牌"的表述，那句话与引擎必须能判定直接矛盾）
4. **配置可热更**：规则配置发布不得重启房间进程，运行中的房间继续用旧版本，新房间用新版本。
5. **人力**：2 人，34～38 周（U9）；后台与运维也由这 2 人承担，不额外配人。后台的**使用者是产品负责人本人**（C7 的"运营者"在 2 人团队里就是他自己），因此后台不需要多角色协作与审批链，但需要单人也能防误操作（见 §8.1）。
6. **平台限制**：iOS 无 JIT，移动端若要做只能走 wasm；v1 不做移动壳（U5），桌面壳与 Web 均跑 wasm，因此 v1 不存在 JIT 问题。Android WebView 版本碎片真机回归推迟到移动壳解押时再做。

### 3.4 外部依赖与前置条件

| 依赖 | 需要什么 | 阻塞哪个里程碑 |
| --- | --- | --- |
| 域名与 ICP 备案 | 国内服务器必须备案，域名实名 + 管局审核约 2～4 周（U6 定国内单区） | **M0 起点之前**，未完成则第 1 周无法部署测试环境 |
| 短信服务商 | 企业资质、签名与模板报备（1～2 周） | M2 账号链路（`A5`）；未批则先走测试白名单手机号 |
| 开源素材授权 | CC0 原文或允许商用的许可原文归档到 `../../assets/license.md` | M1 UI（`A3`） |
| 版号 | 材料撰写与提报，6～12 个月。**已由 U5 移出本计划关键路径**，不在 M0–M4 阻塞任何阶段 | 不阻塞；仅当决定上商店时重新进关键路径 |
| 开发者账号 | Apple（年费）、Google（一次性）。v1 不做移动壳与 TestFlight，**M2 不需要** | 移动壳解押时才需要 |

两条口径说明：备案在 M0 之前，而不是"M2 部署前"——测试环境与联调都要能公网访问，晚做会把 M0 整体后移；版号不写"阻塞上架"，因为它已经不在本期路径上。



## 4. 方案与备选

### 4.1 先把"选什么"分清楚

| 层 | 内容 | 本项目结论 |
| --- | --- | --- |
| 平台层 | 账号、短信、大厅、匹配、排行、战绩、后台、支付/实名 | **本节要选的对象** |
| 房间层 | 房间生命周期、状态同步、断线恢复、超时托管 | **本节要选的对象** |
| 传输层 | WebSocket 帧格式、重连语义、心跳 | 固定自研：protobuf over WS |

传输层不参与选型的原因：牌局每人每步只有几十字节，且必须按座位裁剪视图（`view(seat)`）。通用框架的"全量 schema 同步"在这里既无性能收益，又会把不该下发的字段推到客户端，与 §3 约束 3 冲突。

### 4.2 候选与关键事实

| 候选 | 形态 / 许可 | 关键事实（含来源） | 与 Rust 引擎的接法 |
| --- | --- | --- | --- |
| **P1 自建** | Rust：axum + tokio + sqlx + PG + Redis | 无第三方依赖，能力按需实现；账号/匹配/排行都是成熟模式，工作量可估算 | 同进程，直接调用引擎 crate |
| **P2 Nakama** | Go 单体服务，Apache-2.0（[license](https://heroiclabs.com/opensource/)） | 内置 Console 后台（默认 7351 端口），账号/存储/好友/群组/聊天/排行/锦标赛/内购校验齐全；权威房间支持自定义 tick（1–60Hz），但**不自动同步状态**，帧由服务端自行广播（[文档](https://heroiclabs.com/docs/nakama/getting-started/console/)） | 运行时只支持 Go/TS/Lua + 原生 Go 模块，Rust 引擎装不进去；必须做独立房间服 + 跨进程调用 |
| **P3 Colyseus** | Node.js / TS，MIT（[官网](https://colyseus.io/)） | schema 声明式二进制增量同步、内置匹配、自带基础管理面板、0.18 起带预测/回滚（[文档](https://docs.colyseus.io/)） | 引擎只能以 wasm 在 Node 里跑，或用子进程调用 native |
| **P4 Skynet** | C 核心 + Lua，MIT | Actor 模型、单进程多线程、国内棋牌/页游大量使用；逻辑语言与 Lua 深度绑定（[说明](https://ldt.indienova.com/u/levelplay/blogread/5363)） | 引擎需以 C ABI 暴露给 Lua，或把规则重写一遍；两条都与"引擎单源"冲突 |
| **P5 Photon** | 商业托管、闭源 | 按并发计费、全球节点、客户端 SDK 成熟；数据与逻辑不在自己手里（[对比](https://caisual.com/blog/colyseus-vs-playroom-vs-photon-vs-nakama)） | 引擎无法下沉，牌局逻辑要按它的模型重写 |
| **P6 Agones** | Kubernetes 游戏服编排 | 负责"开服/停服/节点调度"，不提供账号与匹配；Nakama 官方有集成指南（[文档](https://heroiclabs.com/docs/nakama/guides/)） | 只解决部署编排，解决不了本项目的核心问题 |
| （P7 Pomelo） | Node.js 老牌分布式框架 | 社区多年少更新，不列入正式候选 | — |

**本表的事实置信度**：以上源信息来自评审期未获人批准的 7 个外部 URL（评审者不得抓取，故本轮无第三方复核）。候选编号 P1–P7 与 §1.4 成功判据 S1–S12 互不重叠；结论（P1 自建）不依赖任何单一数字——三条理由都在本文档内部可验证（§4.4）；但若要把本表当作对外材料引用，必须逐条重查许可条款与计费口径，不凭记忆复述。

### 4.3 八维对比

记法：○ 满足、△ 需额外工作、× 不成立。判据固定为三条，避免同一维度一人判○一人判×：**○** = 该能力在候选自带且无需改协议即可用于本项目；**△** = 候选不带，但可在一个里程碑内自行补齐；**×** = 候选的结构与本项目硬约束冲突，补齐代价超过自建。

| 维度 | P1 自建 | P2 Nakama | P3 Colyseus | P4 Skynet | P5 Photon |
| --- | --- | --- | --- | --- | --- |
| 引擎同进程（native 直调） | ○ | × | × | △（C ABI） | × |
| 平台能力开箱程度 | ×（要自己写） | ○ | △（账号/DB 有，社交弱） | × | △（只做联机） |
| 规则配置后台可定制 | ○ | △（Console 不能管玩法参数，仍要自建） | △（面板基础，玩法配置仍要自建） | × | × |
| 语言栈负担 | ○（Rust 一种） | △（+Go 运行时） | △（+Node 运行时） | ×（+Lua） | ○（但要接 SDK） |
| 运维复杂度 | ○（1 进程 + PG/Redis） | △（Nakama + CockroachDB/PG + Rust 房间服） | △（Node + Rust 子进程） | △（C 服务 + Lua 脚本热更） | ○（托管） |
| 费用 | 仅服务器 | 软件免费，托管按资源计费 | 软件免费，云服务 $15/月起 | 仅服务器 | 按 CCU 计费，规模化后显著 |
| 34～38 周内可落地 | ○（平台自写 4～6 周，已在基线内） | △（集成 + 学 Go，且规则后台仍要写，净增 3～5 周） | △（wasm 调试链变长，净增 2～3 周） | × | × |
| 多玩法扩展 | ○（注册表 + 配置版本） | △（玩法要挂 Nakama 模块） | △ | × | × |

### 4.4 结论：主方案 P1（自建 Rust 平台）

三条决定性理由：

1. **引擎必须同进程**。S10 要求 wasm 与 native 两份产物算出同一个 `stateHash`；若引擎游离在框架运行时之外（Nakama 的 Go/TS/Lua、Colyseus 的 Node），房间状态就要跨进程序列化，`stateHash` 同源这条判据直接失效，断线恢复还要两边对账。
2. **运营后台这块通用框架给不了**。需求是"在后台配置其他棋牌的规则并上线"，Nakama Console 管的是玩家/存储/匹配/排行，Colyseus 的面板管的是房间与连接，**没有一家管"玩法参数"**。接框架后后台照样要从零写，选型收益被削掉一半。
3. **2 人团队维护第三种运行时的成本高于自建平台层**。账号 + 短信验证约 1 周，匹配约 1 周，排行与战绩约 1 周，三项合计 **3 周**；再加上三者联调、权限与运维脚本，实际按 **4～6 周** 计（差额来自集成与返工，不是凭空多出）。相比之下引入 Go/Node 运行时，要额外承担一套构建、一套依赖治理、一套排障手段，且团队此前没有这两类服务的运维经验。

代价（要认账的）：
- 平台能力没有现成的社交/聊天/锦标赛/内购校验，首版只做账号、战绩、排行，其余留接口（见 §2.2）。
- 房间层要自己写：房间生命周期、断线恢复、超时托管、观战扇出，约 3～4 周。
- 没有社区示例可抄，第一次做 Rust 游戏服要踩异步与生命周期设计的坑（`A15`）。

### 4.5 正面回答"一般游戏服务是怎么做的"——照抄范式，不照搬框架

| 能力 | 参考对象 | 本项目落法 | 首版 |
| --- | --- | --- | --- |
| 账号与会话 | Nakama 的 device/social 登录 + session token | 手机号 + 验证码 → 短期 JWT + 刷新 token；设备 ID 仅用于风控 | 做 |
| 玩家存储 | Nakama Storage Collections | PG 表 `player_profile` / `player_stat`，按玩家维度索引 | 做 |
| 匹配 | Nakama matchmaker 的 required / desired / optional 三段参数 | 同样三段：段位区间（required）、延迟区（desired）、偏好标签（optional）。**内测首日没有段位可填，前 10 局走定级局**（required 放宽为"无段位匹配无段位"），定级结束后才进段位池；不足 4 人用 bot 补位，补位房记为 `mixed`，不计段位与正式战绩（§6.6） | 做 |
| 房间生命周期 | Colyseus 的 create → auth → join → drop → reconnect → leave → dispose | 同样七态，钩子化；`drop` 不销毁房间，进 90 秒宽限期。结算子态（`settling` / `round-ended` / `settle-failed`）作为 `join` 之后的子状态并入这一套枚举，转移条件见 §6.6；§10.1 集成测试按同一套枚举验收，不另立第二套状态 | 做 |
| 玩法即数据 | Skynet 的 service 注册表（取其"登记即生效"） | `RuleSetRegistry`：一款玩法是一份声明式 RuleSet，由解释器执行；注册表只管版本、灰度与回滚。C7/C8 之后不再要求"一玩法一个 crate" | 做 |
| 规则配置与上线 | 无直接参考（Nakama/Colyseus 都不管） | 引擎导出 JSON Schema → 后台自动生成表单 → dry-run 跑黄金用例 → 灰度 → 全量 → 回滚 | 做（M3） |
| 后台信息架构 | Nakama Console 的五段（Players / Storage / Matches / Leaderboards / Settings） | 照抄这五段，另加 Games（玩法与规则）与 Audit（审计） | 做 |
| 编排与扩容 | Agones（K8s 调度） | 首版不做编排：**单实例 + 网关粘性路由**（按 `matchId` 哈希把同一房间的连接固定到同一实例），重连只在宿主实例处理。扩容按房间分片，超出单实例实测上限后才引入第二实例与房间迁移（§4.8） | 不做 |
| 社交 / 锦标赛 / 内购校验 | Nakama 现成能力 | 只留接口与表结构，不实现（§2.2） | 不做 |

### 4.6 备选 S2：Nakama 作平台层 + Rust 房间服

**触发条件**（任一成立即改选）：
- 4 周内要上线"好友 / 群组 / 聊天 / 锦标赛 / 内购校验"中的 3 项以上；
- 团队里有人能长期维护 Go 服务；
- 平台要同时承接多款玩法，且运营团队与研发分离。

**改选的代价**：房间状态跨进程，`stateHash` 只能做进程内校验；断线恢复需要 Nakama 与 Rust 房间服双向对账；第三方支付回调要走 Nakama 的 wallet 模型，国内支付接入反而更绕。

### 4.7 备选 S3：Colyseus 作房间层

**触发条件**：放弃"引擎 native 直调"，接受 Node 进程内跑 wasm 引擎。

**代价**：牌局消息量小，schema 增量同步的优势用不上；wasm 的内存与调试链路比 native 长；仍要自建平台与规则后台。

### 4.8 否决项

| 方案 | 否决理由 | 重新考虑条件 |
| --- | --- | --- |
| P4 Skynet | 逻辑绑定 Lua，Rust 引擎需 C ABI 桥接或重写；多一层 Lua 运行时 | 若规则改为用 Lua 编写且放弃 Rust 引擎 |
| P5 Photon | 闭源、按 CCU 计费、逻辑不在自己手里；国内商业化还要单独处理合规 | 若项目改为海外发行且不做数据自持 |
| P6 Agones | 只解决开服编排，账号/匹配/规则后台一个都不解决；本项目单实例即可 | 房间数超过单实例实测上限（M3 压测给出，预估 3000～5000 房间/实例） |
| P7 Pomelo | 社区多年少更新，Node 生态已有 Colyseus 覆盖同类能力 | 不再考虑 |

### 4.9 选型决策记录（汇总进 §12）

| 决策 | 理由 | 代价 | 依据 / 假设 |
| --- | --- | --- | --- |
| 平台层与房间层都自建，不引入通用框架 | 引擎必须同进程；规则后台通用框架不提供；2 人不宜多担一种运行时 | 平台层自写 4～6 周 + 房间层 3～4 周（合计已计入 34～38 周基线）；无社区示例 | S10、C3、C4、`A15` |
| 传输层自研 protobuf over WS | 消息量小，且必须按座位裁剪，通用全量同步不安全 | 需自写重连与心跳语义 | §3 约束 3、§4.1 |
| 玩法以 RuleSet 声明、由解释器执行（不再每玩法一个 crate） | 运营要零代码上新玩法，规则必须是数据而不是代码 | 解释执行有性能与内存开销；规则语言对外发布后语法不能轻易改 | `C7`、`C8`、`A14`、`A16` |
| 解释器在 wasm 与 native 用同一份 Rust 实现，两端加载同一版本 RuleSet | 保证本地 bot 与线上判定同源，`stateHash` 双端可比 | wasm 包体增大；解释器要避免平台相关行为 | S10、`A14` |
| 原语库只能由引擎发版，RuleSet 只能由后台发布 | 否则"零代码"会演变成"任意代码"，沙箱、校验与回滚全部失效 | 原语库不够用时，运营需求要等一次引擎版本 | `A14`、`A17` |

本表是 §12.1 的子集：只列与选型直接相关的五条，最终台账以 §12.1 为准，两表的"代价"字段必须一致（不一致时以 §12.1 为修订依据并回改本表）。

### 4.10 C7/C8 变更后的架构调整（不推翻 §4.4 的结论，只改引擎形态）

| 层 | 变更前 | 变更后 |
| --- | --- | --- |
| 引擎 | `GameDef` trait，每玩法一个 crate，编译期注册 | **规则解释器 + 原语库**：原语用 Rust 实现，RuleSet 用声明式数据组合原语 |
| 玩法来源 | 研发写代码 | 运营在后台用编辑器产出 RuleSet |
| 校验 | 单测与回归 | 静态 schema 校验 → 引用完整性 → dry-run 黄金用例 → 灰度 |
| 后台表单来源 | 每加一款玩法改一次后台 UI | 引擎导出 JSON Schema，后台按 schema 自动生成表单，玩法增删不改后台代码 |
| 版本 | 引擎版本 | 引擎版本 + 解释器版本 + RuleSet 版本，三者写入回放头 |

**不可越界的红线**：DSL 只能组合原语，不能定义新的计算逻辑。一旦允许 RuleSet 里写表达式、循环或自定义函数，沙箱与校验成本会指数上升，"配置即炸弹"就变成"配置即后门"。这条要在 §5 的契约里落成类型约束，不靠约定；具体落地见 §5.1（`RuleSetBody` 用具名变体 ADT、禁 `serde_json::Value` 与 `#[serde(flatten)]`、ID 用 newtype 并在解析期建索引）。


## 5. 契约：接口与数据

### 5.1 RuleSet 顶层结构

一款玩法就是一份 `RuleSet`，它是**数据不是代码**。字段如下（§5.2 的原语只能出现在这里声明的位置）。

| 字段 | 类型 | 草稿态必填 | 发布态必填 | 约束 |
| --- | --- | --- | --- | --- |
| `rulesetId` | string | ✓ | ✓ | 全局唯一，kebab-case，发布后不可改 |
| `version` | u32 | —（草稿态 `RuleSetBody` 不含此字段） | ✓（发布时由服务端注入） | 发布后不可变；编辑期用 `draftId`（见 §5.6/§5.8），版本号只在发布时分配；草稿/校验留档一律以 `draft_id` 关联，不用 `version` |
| `interpreter` | semver range | ✓ | ✓ | 声明所需解释器下限，例如 `>=1.2,<2`；不满足则拒绝加载；发布时把区间解析为具体版本锁定进 `ruleset.pinned_interpreter_version`（§5.8） |
| `seats` | `{count, teams}` | ✓ | ✓ | `count` 为座位数；`teams` 为空表示无阵营（斗地主）。匹配、补偿、脱敏别名一律按 `count` 参数化，不得写死 4 |
| `deck` | `{copies, ranks, suits, jokers, exclude[]}` | ✓ | ✓ | `jokers` **必填**，固定 `2`（= 1 大王 + 1 小王）；`exclude` 缺省空数组。总张数 = `ranks × suits × copies + jokers × copies` = 13×4×2 + 2×2 = **108**，与 `zones.hand` 容量 27×4 对齐 |
| `zones` | `[{id, scope, capacity}]` | ✓ | ✓ | `capacity` 必填且是**上限**（`-1` 表无上限），不是当前牌数。`scope` 只有 `private`（座位私有）与 `shared`（公共）；private 区按 `(seat, zoneId)` 实例化。静态校验、运行时断言与错误码见 §5.4 L2、§5.7 |
| `levelTable` | `{id, initial, ranks[], upgrade[]}` | ○（掼蛋必填） | ○ | 级牌表本体。`levels` 是房间状态字段（**每队一个**，见 §5.13），不是常量；`@level` 按当前行动方的队求值；`upgrade[]` 给出名次组合对应的升级档 |
| `naturalOrder` | `[rankKey]` | ✓ | ✓ | 自然点数有序表（不含级牌位），允许 `little-joker`/`big-joker`；`levelInsert` 给出级牌插入位，引擎载入时合成最终牌力序，合成算法见 §5.13 级牌行 |
| `levelInsert` | 枚举 | ✓ | ✓ | `@level` 的插入位：`above-A`（插在 A 之后、小王之前，掼蛋基线）或 `below-2`；最终序由 `naturalOrder` + `levelInsert` 在载入期唯一合成 |
| `wildcard` | `{source, substitute[], inStraightFlush}` | ○ | ○ | 掼蛋的逢人配；`source` 如 `heart-@level`；`substitute` 用枚举列出可替代范围；`inStraightFlush` 决定能否参与同花顺（C2 争议项之一） |
| `patterns` | `[{id, primitiveId, params}]` | ✓ | ✓ | 每个牌型必须自带 `id`，供 `compare.ladder` 与 `actions.legalFrom` 引用；引用不存在即 `RS_REF_MISSING` |
| `compare` | `{ladder[], bombOrder, flushRank}` | ✓ | ✓ | 跨牌型压制顺序用枚举列表（元素必须是 `patterns[].id`）；`bombOrder` 决定炸弹之间谁大：`count-then-rank` 或 `rank-then-count`；**`flushRank` 是"同花顺与炸弹的序"这个争议项的唯一开关**：取 `below-bomb-5`（插在 5 张炸之前）、`above-bomb-5`（插在 5 张炸之后、6 张炸之前）、`above-bomb-6`（插在 6 张炸之后、7 张炸之前）或 `above-bomb-8`（高于 8 张炸），四值各自产生不同 ladder；**`guandan-classic` v3 取 `above-bomb-5`**（即 4 张炸 < 5 张炸 < 同花顺 < 6 张 < 7 张 < 8 张 < 四王炸），其余三值是留给其他 RuleSet 的可选值；不同取值分别得到不同 `stateHash` 作为生效判据（§10.3），**不需要改 ladder 本身** |
| `tribute` | `{enabled, trigger, pairs[], returnPolicy, resist}` | ○（掼蛋必填） | ○ | `pairs` 是"谁向谁"的**一对一映射数组**（`{payer, receiver, when}`），不是两个独立列表；`payer`/`receiver` 为闭集枚举 `first`/`second`/`third`/`fourth`/`last`，与 §5.13 名次一一映射；`when` 取值 `single-loss`（单贡：末游→头游）或 `double-loss`（双下：对位进贡，末游→头游、三游→二游）；`returnPolicy` 取 `largest-under-ten` 或 `any-when-none`；`resist` 为 `two-big-jokers` 或 `none`，其中 `two-big-jokers` 按贡方两家合计持两张大王判定，不要求在同一人手里；单贡时即贡方自己持两张 |
| `lifecycle` | `[phase]` | ✓ | ✓ | 固定四段 `[deal, tribute, play, settle]`；**有动作的 phase 必须至少一条 `legalFrom` 指向它，零动作 phase（`deal`/`settle`）豁免**（`deal` 由 `deal.shuffle` 自动执行、`settle` 由结算纯函数自动执行）。`turn` 是 `play` 内的轮次子阶段，**不是 phase** |
| `actions` | `[{kind, primitive, legalFrom}]` | ✓ | ✓ | `kind` 是动作类型（`play`/`pass`/`tribute`/`return`/`claim`/`declare`），`primitive` 指向实际执行的原语，`legalFrom` 只允许三种取值：`patterns`（受牌型约束）、某个 zone 的 id、某个 phase 名（如 `tribute`/`play`）；`turn` 不是合法取值。**`kind` 不能当幂等键**（一局内会重复出现），幂等键见 §6.4 的 `requestId` |
| `settle` | `{algorithm, levelTableRef?}` | ✓ | ✓ | 拆两个 ADT 变体：`Settle::LevelUp{levelTableRef}`（`algorithm: level-up`，`levelTableRef` 必填且引用 `levelTable.id`）与 `Settle::ScoreOnly{}`（`algorithm: score-only`，无等级表引用，用于斗地主等无阵营玩法）；不再隐式引用外部表名 |
| `view` | `{mode, fields[]}` | ✓ | ✓ | `mode` 只有 `hide`（黑名单）与 `project`（白名单）；传输出入一律按 `project` 生成，避免"漏写一条就漏牌"。`fields` 只允许三种写法：`seat.levels`、某个 zone 的 id（私有区按 zoneId 引用，不写 `seat.hand`）、`lastPlay`；其它写法一律 `RS_REF_MISSING` |
| `timing` | `{turnTimeSec, graceSec, offlineAllSec}` | ○ | ○ | 计时器可调参数：`turnTimeSec` 5–120 秒（默认 30）、`graceSec` 30–180 秒（默认 90）、`offlineAllSec` 60–600 秒（默认 300）；越界值在 L1 拒绝；缺省取括号内默认值 |
| `ai` | `{features[], hint}` | ○ | ○ | 给 AI 的特征与提示开关，不影响判定。`features` 取值集合见 §5.11 |

一个最小片段（节选，说明"数据化"到什么程度）：

```yaml
rulesetId: guandan-classic
interpreter: ">=1.2,<2"
seats: { count: 4, teams: [[0,2],[1,3]] }
deck: { copies: 2, ranks: [2,3,4,5,6,7,8,9,10,J,Q,K,A], suits: [S,H,D,C], jokers: 2, exclude: [] }
zones:
  - { id: hand,    scope: private, capacity: 27 }   # 上限 27，按 (seat, hand) 实例化
  - { id: discard, scope: shared,  capacity: -1 }   # 无上限
levelTable:
  id: guandan-level
  initial: 2
  ranks: [2,3,4,5,6,7,8,9,10,J,Q,K,A]
  upgrade: [ { combo: { winnerRank: 1, loserRank: 2 }, step: 3 },
             { combo: { winnerRank: 1, loserRank: 3 }, step: 2 },
             { combo: { winnerRank: 1, loserRank: 4 }, step: 1 } ]
naturalOrder: [2,3,4,5,6,7,8,9,10,J,Q,K,A,little-joker,big-joker]
levelInsert: above-A
wildcard: { source: heart-@level, substitute: [2,3,4,5,6,7,8,9,10,J,Q,K,A], inStraightFlush: false }
patterns:
  - { id: single,           primitiveId: pattern.same-rank,           params: { count: 1 } }
  - { id: pair,             primitiveId: pattern.same-rank,           params: { count: 2 } }
  - { id: triple,           primitiveId: pattern.same-rank,           params: { count: 3 } }
  - { id: straight-5,       primitiveId: pattern.straight,            params: { length: 5 } }
  - { id: plate-3,          primitiveId: pattern.consecutive-pairs,   params: { pairs: 3 } }
  - { id: steel-2,          primitiveId: pattern.consecutive-triples, params: { triples: 2 } }
  - { id: bomb-4,           primitiveId: pattern.bomb,                params: { count: 4 } }
  - { id: bomb-5,           primitiveId: pattern.bomb,                params: { count: 5 } }
  - { id: bomb-6,           primitiveId: pattern.bomb,                params: { count: 6 } }
  - { id: bomb-7,           primitiveId: pattern.bomb,                params: { count: 7 } }
  - { id: bomb-8,           primitiveId: pattern.bomb,                params: { count: 8 } }
  - { id: straight-flush-5, primitiveId: pattern.straight-flush,      params: { length: 5 } }
  - { id: joker-bomb,       primitiveId: pattern.joker-bomb,          params: {} }
compare:
  ladder: [single, pair, triple, straight-5, plate-3, steel-2, bomb-4, bomb-5, bomb-6, bomb-7, bomb-8, joker-bomb]
  bombOrder: count-then-rank
  flushRank: above-bomb-5      # 同花顺位次：below-bomb-5 / above-bomb-5 / above-bomb-6 / above-bomb-8（C2 争议项，改这一个值即可切换）
tribute:
  enabled: true
  trigger: last-round-rank
  pairs:
    - { payer: last,         receiver: first,  when: single-loss }
    - { payer: fourth,       receiver: first,  when: double-loss }
    - { payer: third,        receiver: second, when: double-loss }
  returnPolicy: largest-under-ten
  resist: two-big-jokers
lifecycle: [deal, tribute, play, settle]
actions:
  - { kind: play,    primitive: action.require-legal, legalFrom: patterns }
  - { kind: pass,    primitive: action.pass,         legalFrom: play }
  - { kind: tribute, primitive: tribute.pay,          legalFrom: tribute }
  - { kind: return,  primitive: tribute.return,       legalFrom: tribute }
settle: { algorithm: level-up, levelTableRef: guandan-level }
view: { mode: project, fields: [hand, seat.levels, discard, lastPlay] }
```

> 本示例是**草稿态** `RuleSetBody`：不含 `version`，发布时由服务端注入版本号并复制进 `ruleset`（§5.6/§5.8）。

这段 YAML 是**校验基准**：L1–L4 必须能把它跑通；示例过不了自己定的校验，即视为设计缺陷而不是示例笔误。

### 5.2 原语清单（DSL 的能力边界就是这张表）

| 类别 | 原语 | 关键参数 | 允许改状态 |
| --- | --- | --- | --- |
| 发牌 | `deal.shuffle` / `deal.draw` / `deal.reveal` | 数量、目标区、是否公开 | 是 |
| 牌型 | `pattern.same-rank` / `pattern.straight` / `pattern.consecutive-pairs` / `pattern.consecutive-triples` / `pattern.with-attachment` / `pattern.all-same-suit` / **`pattern.bomb`** / **`pattern.straight-flush`** / **`pattern.joker-bomb`** | 张数（`bomb` 支持 4～8）、连续长度、是否允许通配 | 否 |
| 比较 | `compare.by-rank` / `compare.by-count` / `compare.by-ladder` / **`compare.bomb-order`** | 比较维度（枚举）、压制表 id、炸弹序策略 | 否 |
| 行动 | `action.require-legal` / `action.consume-cards` / `action.pass` / **`action.claim`** / **`action.declare`** | 是否强制、消耗区；`claim`/`declare` 对应同名 `kind` | 是 |
| 轮转 | `turn.next` / `turn.after-all-pass` / `turn.skip-finished` / `turn.return-to-last` | 方向、全过后回谁、是否跳过已出完的人 | 是 |
| **进贡** | **`tribute.determine`** / **`tribute.pay`** / **`tribute.return`** / **`tribute.resist`** | 谁是贡方/受方（`pairs[]` 的 `when` 分支）；贡牌与还贡牌**由引擎按策略自动选定**（`pay`：除红桃级牌外点数最大、同点数取牌 id 最小；`return`：按 `returnPolicy` 选定），贡方/受方没有选牌权；选定结果经 `TributeEvent` 回传；`resist` 命中后的分支 | 是 |
| **级牌** | **`level.current`** / **`level.upgrade`** / **`level.pass`** | 当前级牌来源、升级档位表引用、过 A 判定 | 是 |
| 计分 | `score.table` / `score.accumulate` / `score.level-up` | 查表 id、累加维度 | 是 |
| 视图 | `view.project` | 可见字段集合（白名单），与 `view.mode` 配合 | 否 |
| 随机 | `rng.pick` / `rng.range` | 只能调用引擎注入的 RNG | 是 |
| 断言 | `assert.min-players` / `assert.card-count` / **`assert.zone-capacity`** | 用于 dry-run 前置检查 | 否 |

**红线（必须在类型系统里保证，不靠文档约定）**：

1. 没有表达式：所有条件都是"枚举 + 查表"，`compare` 的取值只能是上表列出的维度。
2. 没有循环：批量化靠原语参数（如 `length: 5`），不提供 `for`。
3. 没有用户自定义函数：RuleSet 不能定义新原语，也不能引用外部代码。
4. 引用必须强类型：`PatternId` / `ZoneId` / `ActionId` 是 newtype，反序列化后必须能在索引里查到。

### 5.3 类型约束的落地方式

- `RuleSetBody` 用**固定变体的 ADT**（`enum Primitive { SameRank{..}, Straight{..}, .. }`），`serde` 反序列化失败即拒绝加载。
- 禁用 `serde_json::Value` 与 `#[serde(flatten)]` 作为字段类型，避免出现"任意 JSON 透传"的逃生口。
- ID 引用在解析期构建索引，缺失即 `RS_REF_MISSING`；索引也用于编辑器做下拉候选。

### 5.4 校验四级（后台发布前的闸门）

| 级别 | 检查什么 | 失败表现 | 耗时预算 |
| --- | --- | --- | --- |
| L1 结构 | JSON Schema + ADT 反序列化；`timing` 各字段取值范围校验 | 字段缺失、类型不符、未知原语、计时参数越界 | <10ms |
| L2 引用 | 所有 ID 能在索引里找到；`lifecycle` 每个有动作的 phase 都有可执行原语；**静态牌堆校验**：deck 各点数 × copies × 花色求和 + `jokers` × copies = 总张数 108（13×4×2 + 2×2 = 108，与 §5.1 同式）；每区 `capacity` ≥ 0 或 −1（−1 表无上限，不参与静态求和） | 指向不存在的牌型/区域；phase 无可执行原语；牌堆静态张数不符；容量取值非法 | <50ms |
| L3 可玩性 | 随机发牌 + 随机合法动作跑 200 局：必须全部终局、无死锁、非法动作率 0。必须记录每局 seed 与失败局的最小动作序列，失败可复现 | 死循环、无终局、非法动作 | **预算 ≤ 90 秒（4 并发）**：单局 300 步 × 0.8ms ≈ 0.24s，200 局串行约 48s，4 并发约 12s；90s 预留 7 倍余量给"慢机器 + 失败局留档" |
| L4 回归 | 黄金用例（真实对局录制或手工构造的 action log）逐条重放，`stateHash` 必须一致 | 规则改动破坏了旧局 | 按用例数线性，单条 ≤ 1s |

**运行时牌数断言**（不属于 L2 静态校验，在发牌后与每步 `apply` 后由引擎断言）：Σ 各区**当前**牌数 = deck 总张数，且每区当前牌数 ≤ `capacity`（`capacity = -1` 的区跳过容量检查）。守恒失败报 `RS_ZONE_CONSERVATION`，超上限报 `RS_ZONE_CAPACITY`，两码触发条件与 §5.7 逐字一致。

L3 与 L4 都在**发布时**跑，不在编辑时跑；编辑时只跑 L1、L2 并实时标红。

### 5.5 版本与回放绑定

| 版本项 | 写在哪 | 作用 |
| --- | --- | --- |
| `engineVersion` | 回放头、房间快照 | 判定是否有跨版本行为差异 |
| `interpreterVersion` | 回放头、RuleSet 校验、`ruleset.pinned_interpreter_version` | RuleSet 声明的 `interpreter` 区间在此校验；发布时解析为具体版本号并锁定进 `pinned_interpreter_version` |
| `ruleSetVersion` | 回放头、房间创建时锁定 | 运行中的房间不随发布切换版本 |

一条硬规则：**房间在创建时锁定 RuleSet 版本，直到该场结束**。发布只影响新房间，否则同一局里规则会中途变化，`stateHash` 就不可能稳定。

### 5.6 后台 API 契约

| 接口 | 入参 | 出参 | 语义 |
| --- | --- | --- | --- |
| `POST /admin/rulesets` | `{rulesetId, name, baseVersion?, draftKey?}` | `{draftId}` | 新建草稿；幂等键为调用方显式传入的 `draftKey`（缺省由服务端按 `rulesetId + baseVersion` 生成）；**同一 `baseVersion` 允许并存多份草稿**，以 `draftId` 自身去重，重复 `draftKey` 调用返回同一 `draftId` |
| `PUT /admin/rulesets/{id}/drafts/{draftId}` | `RuleSetBody`（草稿态，无 `version`） | `{draftId, rev}` | 覆盖草稿（**只对草稿生效**），必须带 `If-Match: rev`；`rev` 不匹配返回 409。已发布的 `version` 一律拒绝写 |
| `POST /admin/rulesets/{id}/drafts/{draftId}/validate` | `{levels: [L1,L2,L3,L4]}` | `{taskId}` | **异步**：立即返回任务号，结果轮询 `GET …/tasks/{taskId}`；L3/L4 必然超出 HTTP 超时 |
| `GET /admin/tasks/{taskId}` | — | `{state: running\|done\|failed, findings[], stats}` | 校验与发布任务的统一查询口 |
| `POST /admin/rulesets/{id}/publish` | `{draftId, stage: canary}` | `{releaseId}` | `publish` **不新建 `ruleset_release` 行**：只把该 `(ruleset_id, draft_id, rev)` 行的 `stage` 从 `draft`/`validated` 推进到下一档（首档固定 `canary`），并返回**同一个 `releaseId`**。`Idempotency-Key` 必填 |
| `POST /admin/releases/{releaseId}/promote` | `{stage: full, reason, totp}` | `{releaseId}` | 承担 `canary` → `full` 推进：`canary` 达标后由人工按 TOTP 确认调用本接口写入 `full`；跨级返回 409；`Idempotency-Key` 必填 |
| `POST /admin/rulesets/{id}/rollback` | `{reason}` | `{releaseId}` | 回退到上一 `active`；出参 `releaseId` 为**目标版本**的 release 行（不是坏版本行）；`Idempotency-Key` 必填，**回滚不需要 TOTP**（§8.1） |
| `GET /admin/games` | — | `[{gameId, activeVersion, stage, roomCount, canaryVersion}]` | 展示用，不做判定 |
| `GET /admin/audit` | `{actor, from, to, action}` | `[{ts, actor, action, target, before, after, ip, sessionId}]` | 字段与 `audit` 库的 `audit_log` 表一致（见 §5.8） |
| `GET /api/games` | — | `[{gameId, activeVersion, minClientVersion}]` | **客户端侧**接口；`minClientVersion` 是**独立配置项**（§5.16），不由 `interpreter` 区间推导 |

发布语义：`canary` 只对新建房间的 10% 生效，观察窗口 30 分钟；窗口内出现 `stateHash`/`viewHash` 不匹配、`action_illegal` 超标（**不含 CPU 超预算**，见 §7.2/§8.6）或崩溃率超标，自动回退到上一 `active` 版本并落审计；达标后由人工经 `promote` 接口带 TOTP 确认转 `full`（§9.4）。**不再设 `ramp5`/`ramp25` 两个中间档**：内测规模下单档灰度 + 一次全量已经足够，多两级只增加人工推进成本（§8 的 YAGNI 剪枝）。

**`stage` 只推进、不新建**：`publish` 不新建 `ruleset_release` 行，只把该 `(ruleset_id, draft_id, rev)` 行的 `stage` 从 `draft`/`validated` 推进到下一档并返回同一个 `releaseId`；灰度的 `promote` 与回滚同理，只改该行的 `stage`（回滚目标行的 `release_id` 即 `POST rollback` 的出参）。

**低样本出口**：canary 观察窗口内样本动作数 < 200 时，不按比率判定，改为"零 P0 事件"判定并把窗口延长到 2 小时；满 2 小时仍 < 200 的，由人工 TOTP 决定放行（`promote` 到 `full`）或回滚，决定与理由落审计，不允许流程停在无人拍板的位置。

**发布状态机**（接口 `stage`、表 `stage`、文档阶段名必须一一对应；`draft`/`validated` 也落在 `ruleset_release` 同一行记录里，见 §5.8）：

| 状态 | 进入条件 | 允许的下一步 |
| --- | --- | --- |
| `draft` | 新建或 PUT 草稿（`stage` 落在 `ruleset_release` 的同一 `release_id` 上，`activated_at` 为空） | `validated`（L1–L4 全过） |
| `validated` | 校验任务 `done` 且无 Blocker 级 finding | `canary` |
| `canary` | `publish` 把同一行的 `stage` 由 `validated` 推进为 `canary`（不新建行），10% 新房间 | `full`（经 `promote` + 人工 TOTP 确认） / `rolled_back` / 低样本待决后人工放行或回滚 |
| `full` | 全量，成为 `active` | `retired` |
| `rolled_back` | 回退，`stage` 记录为 `rolled_back`（**不写 `retired`**） | 只读，可重新发布新版本 |
| `retired` | 明确下线（不再被任何房间引用且人工确认） | 只读 |

### 5.7 错误码

| 码 | 触发 | 处置 |
| --- | --- | --- |
| `RS_SCHEMA_INVALID` | L1 失败 | 拒绝保存草稿 |
| `RS_REF_MISSING` | L2 失败 | 拒绝发布，标出缺失引用 |
| `RS_NO_TERMINATION` | L3 出现无法终局的局 | 拒绝发布 |
| `RS_ILLEGAL_ACTION_RATE` | L3 非法动作率 > 0 | 拒绝发布 |
| `RS_INTERPRETER_TOO_OLD` | `interpreter` 区间不满足 | 拒绝加载，提示升级下限或改 RuleSet |
| `RS_VERSION_LOCKED_BY_ROOMS` | 删除/覆盖仍被房间引用的版本 | 拒绝，列出引用房间数 |
| `RS_CANARY_BLOCKED` | 灰度期间指标超标 | 自动回滚并落审计 |
| `RS_L4_MISMATCH` | L4 黄金用例重放 `stateHash` 不一致（与 `RS_GOLDEN_REGRESSION_FAILED` 分工：本码只管黄金用例哈希不符） | 拒绝发布，输出差异局 id 与首个分歧步 |
| `RS_ZONE_CONSERVATION` | **运行时**守恒失败：Σ 各区当前牌数 ≠ deck 总张数（与 §5.4 运行时断言同一条件） | 判该场流局，按 §6.5 处置 |
| `RS_ZONE_CAPACITY` | **运行时**某区当前牌数 > `capacity`（`capacity = -1` 的区跳过本检查；与 §5.4 运行时断言同一条件） | 判该场流局，按 §6.5 处置 |
| `RS_PHASE_UNIMPLEMENTED` | `lifecycle` 里的有动作 phase 没有任何可执行原语（归入 L2 引用完整性校验） | 拒绝加载 |
| `RS_QUOTA_EXCEEDED` | dry-run / 发布校验时 `legal_actions` 候选枚举超过 4096 条静态上限；**不用于运行期单步动作拒绝** | 拒绝发布或判 dry-run 结果无效，记 `action_quota_exceeded_total`；RuleSet 侧按缺陷处理 |
| `RS_GOLDEN_REGRESSION_FAILED` | L4 回归种子库出现失败（与 `RS_L4_MISMATCH` 分工：本码只管回归种子库） | 拒绝发布，输出首个失败用例 id 与 `matchId`/`roomSeq` |
| `RS_INTERNAL_PANIC` | 解释器在 native 侧 panic 且被 `catch_unwind` 捕获 | 该动作返回失败、房间继续（§8.6），并记 `room_panic_total` |
| `RS_SEAT_TAKEN` | 进房时目标座位已被占；或房间已满、无空座可分配 | 409；客户端按返回的可用座位重选，或提示房间已满 |
| `AUTH_INVALID` | token 缺失、过期或签名不符；或请求携带的 `seat` 与 token 绑定座位不一致 | 401，客户端跳登录；座位不一致另记指标（§5.9） |
| `CLIENT_TOO_OLD` | `clientVersion` 低于服务端当前的 `minClientVersion` 下限 | 426，客户端提示升级并停止入座（`minClientVersion` 定义见 §5.16） |
| `RATE_LIMITED` | 单账号动作频率超限 | 429，返回 `retryAfterMs` |
| `STALE_SEQ` | 动作 `seatSeq` 小于该座位当前水位（按座位水位判定，与 `TOO_LATE` 区分） | 409，客户端重新拉增量（见 §6.2） |
| `TOO_LATE` | 动作与超时同 tick 竞态落败：动作在"超时事件"入队之后到达（`STALE_SEQ` 是序号过期，本码是竞态落败，两者不混用） | HTTP 409；丢弃该动作、不推进 `seatSeq`、不计入该座位超时次数；客户端按座位水位重拉增量，不重发同一动作 |
| `VERSION_CONFLICT` | `If-Match` 的 `rev` 与草稿当前 `rev` 不符 | 409，返回当前 `rev` 与字段级差异 |

### 5.8 存储结构

**规则发布链**（4 张）：

| 表 | 关键字段 | 说明 |
| --- | --- | --- |
| `ruleset_draft` | `draft_id, ruleset_id, draft_key, rev, body jsonb, updated_by, updated_at` | 草稿可反复覆盖，`rev` 用于 `If-Match`；同一 `baseVersion` 可并存多份草稿，以 `draft_id` 去重；草稿态 `body` 不含 `version` |
| `ruleset` | `ruleset_id, version, body_hash, body jsonb, pinned_interpreter_version, frozen_at` | 发布时从草稿复制，**此后只读**；`body` 与 `status` 分离，`status` 在 release 表；`pinned_interpreter_version` 是发布时把 `interpreter` 区间解析锁定的具体解释器版本（回放头 `interpreterVersion` 同值） |
| `ruleset_release` | 主键 `(ruleset_id, draft_id, rev)`；`release_id, ruleset_id, draft_id, rev, version(可空), stage(draft/validated/canary/full/rolled_back/retired), activated_at, rolled_back_at, operator` | 状态机唯一真相（§5.6）；`draft`/`validated` 行 `version` 为空，以 `draft_id` 关联，不从 `version` 建唯一键；`activated_at` 为空即未生效。回滚时：目标版本行的 `activated_at` 改写为回滚时刻，坏版本行写 `rolled_back`，`POST rollback` 出参返回目标版本 `release_id` |
| `ruleset_validation` | 主键 `(ruleset_id, draft_id, rev, level)`；`task_id, ruleset_id, draft_id, rev, level, passed, findings jsonb, stats jsonb, seed_list jsonb, ran_at` | L1–L4 留档；`seed_list` 保证可复现；草稿/校验阶段无 `version`，一律按 `draft_id + rev` 关联 |

**审计单独一张库**：`audit_log` 不属于上表（它不是规则发布链的一部分），独立 schema `audit` + 只允许 append 的库角色，业务侧账号无删除权（§8.8）。

**业务与账务**（首版必建，即使不接渠道也要有表）：

| 表 | 关键字段 | 说明 |
| --- | --- | --- |
| `player_profile` | `player_id, phone_hash, phone_tail4, nickname, created_at, status` | 手机号只存哈希 + **尾号 4 位**（展示用，§8.4），原文不落库；`phone_hash = HMAC-SHA256(server_salt, 规范化后的 11 位手机号)`；盐轮换采用双盐列或双次查找过渡窗口，纳入 §9.7 破坏性变更五步 |
| `player_stat` | `player_id, game_id, rating, rating_games, placement_left, rank_level, rounds_total, wins, updated_at` | 段位字段名叫 `rank_level`（**不叫 `level`**，避免与房间级牌混淆）。`rating` 是整数内部评分，新账号初始 **1000**；`rating_games` 是已计入评分的场次数；`placement_left` 是定级赛剩余场次（10 → 0，**0 表示已定级**）。段位算法见 §5.13 后附表：`placement_left > 0` 为定级期，定级期只累计 `rating`、不改 `rank_level`、不进排行；定级赛（前 10 场，人机房 `ai-practice` 与自定义房不计）结束后按这 10 场的**胜率**落到 `rank_level` 1–4，第 5 档只能靠后续 Elo 升上去。`rating` 与 `rank_level` 按**场**结算，写入 §6.6 的结算事务。`human-casual`/`bot`/`mixed` 及 `ai-practice`（离线局）不写本表的段位、评分与排行字段（按 §5.15 真值表判定） |
| `round_record` | `match_id, round_no, players jsonb, result jsonb, levels_after jsonb, ruleset_version, engine_version, interpreter_version, started_at, ended_at` | 一局一行。`players` = `[{seat, player_id, rank}]`；`result`（结构名 `settlement`）= `{winner_team, ranks: [seat...], points}`；`levels_after` 为按队伍序号升序的 `u8` 数组（每队级牌快照）；三处 jsonb 形状以本行为准，对账、战绩查询与 §9.7 迁移断言按同一结构比对 |
| `match_record` | `match_id, game_id, match_type(human-ranked/human-casual/bot/mixed), winner_team(可空), end_reason(passed-a/safety-cap/aborted), seats_count, started_at, ended_at` | 一场一行；`matchType` 只在这里定义，其它地方引用它。`winner_team` 为胜方队序号，平局时为空；`end_reason` 标记场次终止原因（`passed-a` 过 A、`safety-cap` 达安全局数上限、`aborted` 整场流局或全员离线超时中止，按 §6.3/§6.5 处置）；段位与积分按**场**结算 |
| `settlement_dead_letter` | `settlement_id, match_id, round_no, payload jsonb, failed_at, retries, resolved_at` | 结算三次写库失败后的死信，**只用于结算补写**，人工按 `settlementId` 补写（§6.6）；流局补偿不走本表 |
| `settlement_compensation` | `compensation_id, match_id, reason, issued(bool), created_at` | 流局补偿记录表：补偿记录先落库、`issued=false` 不发放；支付接口启用后才置 `issued=true` 并发放（§6.5/§9.5） |
| `invite_code` | `code, created_by, created_at, used_by, used_at, expires_at, status` | 短信降级通道（§6.7）；8 位大写码唯一索引，一码一用，7 天有效，**无批次字段** |
| `order` | `order_id, player_id, sku, amount_cents, currency, status(created/paid/refunded), created_at` | **占位表**（U7）：v1 不产生数据，但表与接口先建 |
| `payment_callback` | `callback_id, order_id, channel, payload jsonb, verified, received_at` | 支付回调占位，验签字段预留 |
| `refund` | `refund_id, order_id, amount_cents, status, reason, created_at` | 退款占位 |
| `external_identity` | `player_id, provider, external_uid, verified_at` | 实名与第三方登录的适配位（v1 为空）；与 §9.8 的预留字段同名 |

Redis 的真实数据清单（原稿"只存两个指针"的表述作废）：

| 键 | 类型 | 用途 | 失效 |
| --- | --- | --- | --- |
| `game:{id}:active_ruleset` | string | 当前生效版本 | 持久 |
| `game:{id}:canary_ruleset` | string | 灰度版本 | 发布流程结束时清理 |
| `room:{matchId}:snapshot` | string(json) | 房间快照（含区域内容与 `seqWatermark[]`） | 30 秒覆盖写，房间结束即删 |
| `room:{matchId}:actions` | stream | 动作日志（`ActionRecord`） | 房间结束后保留 90 天（与回放同周期） |
| `match:queue:{gameId}` | zset | 匹配队列 | 实时 |
| `ratelimit:{playerId}` | string | 动作频率计数 | 1 秒滑窗 |

### 5.9 牌桌协议（`crates/protocol`）

M0 要交付的就是这张表；服务端与 wasm 客户端共用同一份 proto 生成代码。

| 方向 | 消息 | 关键字段 | 语义 |
| --- | --- | --- | --- |
| C→S | `JoinRoom` | `matchId, seat?, clientVersion` | 入座或续座；**座位由服务端分配并在响应里返回 `seat`**，`seat` 字段只用于续座（带回原座位）；目标座已被占返 `RS_SEAT_TAKEN`，房间已满无空座同码拒绝。`clientVersion` 低于 `minClientVersion` 返回 `CLIENT_TOO_OLD`（定义见 §5.16） |
| C→S | `Reconnect` | `matchId, seat, lastSeatSeq` | `lastSeatSeq` 之后的增量由服务端补齐；该座位 `lastSeatSeq` 对应的 `roomSeq` 早于缓冲起点（换算按动作记录双序号列，见 §6.5）时只发 `RoomSnapshot` 并以快照 `roomSeq` 为新起点（§6.2） |
| C→S | `SubmitAction` | `matchId, seat, requestId, seatSeq, kind, payload` | `requestId` 为幂等键（§6.4）；`seatSeq` 为**每座位**单调递增；**服务端以 token 绑定的座位为准**，请求 `seat` 与绑定座位不一致返回 `AUTH_INVALID` 并记指标；`payload` 结构见下 |
| C→S | `Heartbeat` | `matchId, seat, clientSeq` | 客户端每 15 秒一次；服务端 30 秒内未收到该座位任何消息即判掉线（**唯一判定点**，§6.2 同步） |
| S→C | `RoomSnapshot` | `matchId, seat, roomSeq, lastSeatSeq, viewHash, view, snapshotReset` | 全量视图（按 `view.fields` 裁剪）；**不下发 `stateHash`**；`snapshotReset: bool`，缺省 false，true 表示客户端必须丢弃本地状态并接受本快照（§6.2） |
| S→C | `RoomDelta` | `matchId, roomSeq, events[]` | 增量事件；断线期间存最近 200 条（§6.2）。`roomSeq` 是房间广播序；本消息是房间广播，**不带 `seatSeq`**，座位水位只在 `Reconnect`/`ActionAck` 按座位传递 |
| S→C | `ActionAck` | `requestId, seatSeq, roomSeq, accepted, reason?` | 回执；`reason` 取 §5.7 错误码**全表** |
| S→C | `PhaseChange` | `phase, deadlineMs` | 阶段切换（发牌/进贡/出牌/结算） |
| S→C | `SettleResult` | `roundNo, result, levelsAfter, nextDealInMs, viewHash` | 结算广播；`levelsAfter` 是**每队**级牌（§5.13） |
| S→C | `TrusteeState` | `seat, mode(ai\|human), reason` | 托管状态变化 |
| C→S | `ViewMismatchReport` | `matchId, seat, roomSeq, clientViewHash, clientPlatform` | 客户端算出的 `viewHash` 与服务端下发的不一致时上报；服务端只落指标与日志，不回消息。上报按 `(matchId, seat, roomSeq)` 去重、单房间设上限；该信号为 P1 定位线索，P0 只保留服务端重算视图与已下发值不一致（§7.2） |

**`SubmitAction.payload` 按 `kind` 分型**（proto 用 `oneof`，每型独立消息）：

| `kind` | 字段 | 说明 |
| --- | --- | --- |
| `play` | `cards: u16[]`、`patternHint: string?` | 只传牌 id；`patternId` **由服务端判定**，`patternHint` 仅用于对不合法提示给理由，不参与判定 |
| `pass` | 无 | 空结构体，禁止带字段 |
| `tribute` | 无 | 空结构体；贡牌由引擎选定（§5.2/§5.13），选定结果经 `TributeEvent{from, to, card}` 回传，贡方不提交牌 |
| `return` | 无 | 空结构体；还贡牌由引擎按策略选定，经事件回传，受贡方不提交牌 |

**`RoomDelta.events[]` 分型**：`PlayedEvent{seat, cards, patternId}`、`PassedEvent{seat}`、`TributeEvent{from, to, card}`、`LevelChangedEvent{team, from, to}`、`TurnChangedEvent{seat, deadlineMs}`、`RoundSettledEvent{roundNo}`。新增事件类型只允许加 `oneof` 分支（§9.2）。

约定：`stateHash` 永不下发（§5.12）；下发的是 `viewHash`，客户端收到快照与结算后各算一次并回报不一致（§7.2 的 P1 线索指标）；`view` 的字段集合由 §5.1 `view.fields` 决定，服务端不得下发白名单外的字段。

**本地存档契约（按模式分流；§2.4 第 16 周 gate 与离线恢复的载体依据）**

两种模式各管一行，不并存两套口径：

- **联机局（含服务端 bot 房）**：机器崩溃、进程被杀一律按网络断线处理，凭服务端快照重连（§6.2 的重连与 `snapshotReset` 口径）；**客户端不写本地存档、不承诺离线续局**。
- **纯本地 bot 局**（wasm 引擎跑在客户端、无服务端判局）：**必须**本地存档，最小契约写清五件事：
  1. 载体：浏览器用 IndexedDB，桌面壳用本地文件；按 `localMatchId` 隔离，只存本地、不上云。
  2. 内容：`{localMatchId, rulesetVersion, engineVersion, interpreterVersion, seed, snapshotSeq, stateHash 或 viewHash, snapshot 字节, actions 尾段}`。
  3. 写入时机：每 N 个 tick 与每次动作后写；写入用原子替换（先写临时键/临时文件，成功后再改名或提交），避免半写。
  4. 恢复：重开时校验 `rulesetVersion` / `engineVersion` / `interpreterVersion` 三个版本号，并重放 `actions` 尾段重算哈希、与存档内 `stateHash` 或 `viewHash` 比对；三者一致才续局；版本不一致、哈希不一致或无存档，一律判"重开新局"并提示玩家。
  5. 上限与清理：单局存档设容量上限（具体值实现阶段按实测快照体积定，本节不写死），且**只保留最近 1 局**，新局开始时清理旧存档。

纯本地 bot 局的存档不上云、不计战绩（§6.6 三档可信度）。

### 5.10 掼蛋规则取值表（U10）

这是"规则本体"的可执行版本：左列是 RuleSet 字段，右列是取值与来源。本表 15 行取值均已确定，不再有待确认项。**其中 7 项（同花顺位置、进贡配对、还贡、抗贡、升级档、过 A、接风）已于 2026-11-23 由用户逐条确认，随本表冻结进 `guandan-classic` v3；此后任何改动只能走 RuleSet 新版本，不得改已发布版本。**

| 规则项 | 字段 | 取值 | 状态 |
| --- | --- | --- | --- |
| 用牌 | `deck` | 2 副共 108 张（每副 52 张 + 2 王，`jokers: 2`）；合计大王 2、小王 2 | 定 |
| 级牌 | `levelTable.initial` + 房间 `levels` | 首局每队都是 2；级牌牌力高于 A、低于小王；**每队一个 `levels`** | 定 |
| 逢人配 | `wildcard.source` | 红桃级牌（`heart-@level`），每副 1 张共 2 张 | 定 |
| 逢人配能否组同花顺 | `wildcard.inStraightFlush` | `false`（C2 争议项，可切换） | 定 |
| 炸弹序 | `compare.bombOrder` | `count-then-rank`：4 张 < 5 张 < … < 8 张；同张数比点数 | 定 |
| 同花顺位置 | `compare.flushRank` | `above-bomb-5`（5 张炸之上、6 张炸之下），完整阶梯：4 张炸 < 5 张炸 < 同花顺 < 6 张 < 7 张 < 8 张 < 四王炸；其余三值（`below-bomb-5` / `above-bomb-6` / `above-bomb-8`）作为其他 RuleSet 的可选值保留 | 定 |
| 四王炸 | `pattern.joker-bomb` | 最高，压所有炸弹与同花顺 | 定 |
| 单张/对/三/顺子/三连对/二连三 | `patterns` | 顺子 5 张、`plate-3` 为 3 连对、`steel-2` 为 2 组三张连 | 定 |
| 进贡配对 | `tribute.pairs[]` | 单贡：末游→头游；双下（对位进贡）：末游（第 4 名）→头游（第 1 名）、三游（第 3 名）→二游（第 2 名），一对一映射，不交叉 | 定 |
| 还贡 | `tribute.returnPolicy` | 受贡方还一张不大于 10 的牌；全大于 10 时还其中最小的一张 | 定 |
| 抗贡 | `tribute.resist` | 贡方两家合计持有 2 张大王即可抗贡（不要求在同一人手里），本局不进贡 | 定 |
| 升级档 | `levelTable.upgrade` | 双下 +3、1+3 +2、1+4 +1（按队结算） | 定 |
| 过 A | `level.pass` | 打 A 那一局，本方名次组合为 `1+2` 或 `1+3`（本方拿到头游、且末游是对家）即算过 A；`1+4`（本方头游但队友末游）不算，其余组合（本方未拿头游）也不算；未过则本队退回 A 以下一级（A→K，降一档）；跨 A 的超额升级数作废 | 定 |
| 接风 | `turn.skip-finished` 的队友判定参数（引擎原语判定，不进顶层 RuleSet 字段） | 某座位出完牌的同一轮里，若其队友是最后压牌者，则该队友在下一轮直接领出，跳过其余两家；`turn.return-to-last` 只管"全过后回到最后出牌者"，不负责接风 | 定 |
| 托管出牌 | `ai.hint` + 托管策略 | 固定"出最小合法牌"，排序键 = 牌力升序、同牌力按牌 id 升序 | 定 |

确认结果的差异：同花顺位置、进贡配对、抗贡三项按拍板结果实改（`above-bomb-5`、对位进贡、贡方两家合计两张大王），过 A 按用户口径放宽（`1+2` 或 `1+3` 即算过 A、`1+4` 不算），还贡、升级档、接风维持原值。以上取值即 `guandan-classic` v3 的生效口径，与 §5.13 的逐条规格一致。

### 5.11 AI 特征与档位

| 档位 | 取值 | 实现层策略 | 特征集合 | 用途 |
| --- | --- | --- | --- | --- |
| 简单 | `easy` | L0（规则评估，无搜索） | `hand.count`, `last.play` | 只做合法性与最小牌，随机扰动 30% |
| 普通 | `normal` | L1（评估 + 有限扫描） | 上列 + `partner.state`, `opponent.count` | 保留大牌、配合队友 |
| 困难 | `hard` | L1 加搜索（有限深度搜索） | 上列 + `table.history`（近 20 手）, `bomb.count` | 记牌与炸弹时机 |

`ai.features` 的合法取值就是上表"特征集合"里出现过的字符串，未知取值在 L1 拒绝。表内 L0/L1 是 AI 实现层代号，与 §5.4 的校验级别 L1–L4 无关。

### 5.12 状态摘要：`stateHash` 与 `viewHash`

三个哈希各管一件事，混用会让"双端一致"这条判据失效：

| 名字 | 输入 | 谁算 | 用途 |
| --- | --- | --- | --- |
| `stateHash` | 全量状态（含所有座位手牌） | 服务端、CI 的 native/wasm 双跑 | 恢复校验、漂移检测、双端一致门禁。**永不发给客户端** |
| `viewHash(seat)` | `view(seat)` 投影 | 服务端在每次下发前算；客户端收到 `RoomSnapshot` 与 `SettleResult` 后算 | 视图一致性线索指标 `viewhash_mismatch_total`（P1，见 §7.2；P0 只保留服务端重算不一致） |

**纳入 `stateHash` 的字段**（共 8 项，按下列固定顺序拼接，不按 map 迭代顺序）：

1. `engineVersion`、`interpreterVersion`、`rulesetVersion`（三个版本串，UTF-8）
2. `phase`（枚举序号 u16）、`turnSeat`（u8）、`levels`（**每队一个 u8**，按队伍序号升序；§5.13）
3. `seats`（按座位号升序）：`seat(u8)`；**手牌不在本段重复计入**，手牌只在第 4 项的私有区进哈希一次
4. `zones`（按 `(seat u8, zoneId u16)` 升序）：`seat`、`zoneId`、`cards`（牌 id 升序）。private 区按 `(seat, zoneId)` 实例化；u16 区号在 RuleSet 解析期按声明序分配并冻结；同一批手牌只在本段进哈希一次，`seats[].hand` 只是视图字段、不进哈希
5. `lastPlay`：`seat(u8)`、`patternId(UTF-8)`、`patternKey`（`(countKey, rankKey)` 两个 u32）、`cards`（牌 id 升序）
6. `seqWatermark`（每座位 u64，按座位号升序）
7. `tribute`（`fromSeat`、`toSeat`、`card`，进贡阶段结束后归零）
8. `rng`：`counter(u64)`、`seedDigest(前 8 字节)`；`seedDigest` = 对 `seed`（u64 定长小端编码）用 BLAKE3-256 取前 8 字节，快照内同时保存 `seed` 原值，摘要由它派生

**`viewHash(seat)` 定义**（与 `stateHash` 同级闭合，客户端必须能据此算出与服务端逐位一致的值）：

- 纳入字段：`view.fields` 白名单顺序列出的字段 + `seat(u8)` + `roomSeq(u64)`；`levels` 是否在内取决于白名单；`clientPlatform` **必须排除**（纳入则两端值天然不同）。
- 序列化：按白名单固定顺序拼接；字节序 little-endian，整数固定宽度（`u8`/`u16`/`u32`/`u64`/`i16`），字符串 UTF-8 且不做 NFC 归一化。
- 哈希函数与截断：BLAKE3-256 取前 8 字节（16 个十六进制字符），与 `stateHash` 同一函数、同一截断长度。

**明确排除**：任何时间戳、连接/在线状态、托管标记（它只影响 UI 与超时策略，不影响合法动作）、日志与审计、客户端版本、昵称头像。

**规范化与序列化**：

- 用 `crates/engine` 里的 `CanonicalState` 固定结构体序列化，**禁止 `serde_json::Value` 与 `HashMap` 参与**（§3.3 约束 2）。
- 字节序固定 little-endian；整数固定宽度（`u8`/`u16`/`u32`/`u64`/`i16`）；字符串 UTF-8 且不做 NFC 归一化（只允许 ASCII 枚举与版本串）。
- 哈希函数固定 **BLAKE3-256**（32 字节输出）；对外呈现固定取**前 8 字节**，即 16 个十六进制字符。协议字段、日志字段统一是这个 16 字符的定长串，不得出现"16 字节"的表述。
- 计算位置：每次 `apply` 成功后由服务端算一次并留在内存态（写快照与审计）；CI 里 native 与 wasm 各算一次后按 **32 字节全量**逐字节比对（不是比 16 字符的呈现串）。

**`legal_actions` 的排序键**（双端必须同序，否则比对必红）：

`(countKey u32 升, rankKey u32 升, cards 牌 id序列 升, primitiveId 升)`。`countKey` 是该牌型的张数权重、`rankKey` 是点数权重，二者由原语按 `compare.bombOrder` 与载入合成的牌力序（`naturalOrder` + `levelInsert`）给出（`patternKey` 就是这一对 `(countKey, rankKey)`）；四元组全序，重复元素不可能出现。若某原语给不出这一对，L1 判该原语不可用于 `legal_actions`。

### 5.13 掼蛋规则规格（按 phase 逐条可判定）

本节是 `guandan-classic` 的规则本体，与 §5.10 的取值表一一对应；两者冲突时以本节为准并把 §5.10 改正。

**deal（发牌）**

| 规则 | 判定写法 |
| --- | --- |
| 牌堆 | 2 副 108 张：`2×4` 花色 × `2..A` 各 2 张 + 4 张王（每副 `jokers: 2` = 1 大王 + 1 小王，两副合计大王 2、小王 2） |
| 发牌 | 每人 27 张，RNG 唯一来源（`rng.counter` 递增），发完剩余 0 张 |
| 决定首出 | v1 固定座位 0 先出；"按 level 最低座位先出"为未来规则描述，v1 不启用；之后由上局头游先出 |

**tribute（进贡/还贡/抗贡）**

| 规则 | 判定写法 |
| --- | --- |
| 谁进贡 | 上局末游向头游进贡 1 张；上局双下（三、四名同队）时对位进贡：末游（第 4 名）→头游（第 1 名）、三游（第 3 名）→二游（第 2 名），各 1 张，一对一映射，不交叉 |
| 贡牌选择 | 由引擎按"除红桃级牌外点数最大、同点数取牌 id 最小"**自动选定**，贡方没有选择权（避免"必须最大"与"自选"两规则互斥）；选定结果经 `TributeEvent{from, to, card}` 广播，进贡/还贡动作 payload 为空结构 |
| 抗贡 | 贡方两人合计持 2 张大王（单贡时贡方自己持 2 张大王）即可抗贡，本局整段 `tribute` 跳过 |
| 还贡 | 受贡方还 1 张**不大于 10** 的牌；若手牌全大于 10，则还其中最小的一张（`returnPolicy: largest-under-ten` 的兜底分支）。该兜底分支由 **L3 随机对局的种子用例**覆盖，不在 L1 覆盖（L1 只做结构校验） |
| 顺序 | `tribute` 是独立 phase，与 `play` 分离；进贡完成后 `stateHash` 里 `tribute` 段归零 |

**play（出牌）**

| 规则 | 判定写法 |
| --- | --- |
| 牌型 | `single`/`pair`/`triple`/`straight-5`/`plate-3`（3 连对）/`steel-2`（2 组三张连）/`bomb-4..8`/`straight-flush-5`/`joker-bomb`，**id 与 §5.1 示例的 `patterns[].id` 完全一致**（`plate-3` 与 `steel-2` 是唯一写法，不出现"木板/钢板"别名） |
| 顺子边界 | 顺子取 5 张连续自然点数，允许从 A 起（`A-2-3-4-5`）但不允许绕回（`Q-K-A-2-3` 非法）；级牌在顺子里按自然点数参与，不按 `@level` 的高位；大小王不得入顺；逢人配代牌后仍须整体连续 |
| 级牌 | 当前 `levels[team]` 对应的牌在**单张/对/三张/顺子**里按"高于 A、低于小王"的牌力参与比较；作为炸弹时仍按自身点数。牌力序由 `naturalOrder` + `levelInsert` 在 RuleSet 载入期合成：先取 `naturalOrder` 自然序，再按 `levelInsert` 把级牌插到指定位置（`above-A` 即 A 之后、小王之前）；合成结果在任一 `levels` 下唯一，不允许占位符参与运行时排序 |
| 逢人配 | 红桃级牌可替代除大小王外任意一张牌；`wildcard.inStraightFlush=false` 时不得用于同花顺 |
| 比较 | 同牌型比 `patternKey = (countKey, rankKey)`（张数优先、再点数）；炸弹按 `bombOrder`；`straight-flush` 与 `bomb` 的相对位置由 `compare.flushRank` 决定（§5.1）；本版 `flushRank=above-bomb-5`，最终压制阶梯 `bomb-4` < `bomb-5` < `straight-flush-5` < `bomb-6` < `bomb-7` < `bomb-8` < `joker-bomb` |
| 过牌 | 无牌可压或主动不要，`pass` 记入事件；三家连续 `pass` 后由最后出牌者重新领出 |
| 接风 | 含义固定为：某座位出完牌的同一轮里，若其队友是最后压牌者，则该队友在下一轮直接"领出"，跳过其余两家；`turn.return-to-last` 只负责"全过后回到最后出牌者"，**不负责接风**，接风由 `turn.skip-finished` + 队友判定实现 |

**settle（结算与升级）**

掼蛋按**队**升级，所以级牌是"每队一个"（`levels: [u8; teams]`），不是全场单值。

| 名次组合 | 升级数 | 下局 `levels` |
| --- | --- | --- |
| 1+2（双下） | +3 | 胜方队的 `levels[team]` 往上 3 档 |
| 1+3 | +2 | 胜方队 +2 |
| 1+4 | +1 | 胜方队 +1 |

`levels` 的存放位置：房间状态字段 `levels`（每队一个 u8，取值 2..A），**不在手牌区、不在 `player_stat`**（后者的 `rank_level` 是段位，字段名不叫 `level`，见 §5.8）；一场（match）内逐局累积，写进 `round_record.levels_after`。

**升级跨 A 的处置**：从 K 打 `1+2`/`1+3` 升级时，先在 A 停留一局（不越级），下一局在 A 上打 `1+2` 或 `1+3` 即算"过 A"；即 `level` 到达 A 后所有超额升级数作废，不累积。这样"过 A"永远只由"在 A 的那一局名次组合是否为 `1+2` 或 `1+3`"决定。

**过 A**：`levels[team]=A` 的一局，本方名次组合为 `1+2` 或 `1+3` 才算过 A；`1+4` 不算，其余组合（本方未拿头游）也不算；未过 A 时该队 `levels` 退回 A 以下一级（A→K，降一档）；跨 A 的超额升级数作废。

**场次终止**：一场（`match`）的终止条件二选一：①某队过 A；②触达安全局数上限（默认 **12 局**，仅防死循环，**不是胜负判据**）。未过 A 达上限时，以各队当前 `levels` 档位较高者为胜方，档位相同记平局（`winner_team` 为空）。整场流局或全员离线超时导致中止的，记 `aborted`（按 §6.3/§6.5 处置）。结果写入 `match_record.winner_team` 与 `end_reason`（`passed-a` / `safety-cap` / `aborted`）；段位与积分按**场**结算，不出现"连续三次未过 A"的特殊规则。

**段位算法：定级赛分档 + 后续 Elo 式浮动**（三张表一并定义；字段 `rating` / `rating_games` / `placement_left` / `rank_level` 见 §5.8）

*表一：定级分档表*（新账号前 10 场为定级赛，只累计 `rating`、不改 `rank_level`；**人机房（`ai-practice`）与自定义房不计入定级赛**；10 场结束后按这 10 场的**胜率**落位，定级期内的 `rank_level` 显示定级分档结果）：

| 定级赛胜率 | 落位 `rank_level` |
| --- | --- |
| < 30% | 1 |
| 30% – 50% | 2 |
| 50% – 70% | 3 |
| ≥ 70% | 4 |

定级最高只到 4；第 5 档不在定级表里，只能靠后续 Elo 升上去。

*表二：Elo 参数表*

| 参数 | 取值 | 说明 |
| --- | --- | --- |
| `R` | 整数内部评分，新账号初始 **1000** | 即 `player_stat.rating`；`R'` 为结算后的新值 |
| `E` | `E = 1 / (1 + 10^((R_opp − R)/400))` | 期望胜率；团队赛取**两队平均分**作为 `R_opp` |
| `S` | 1（胜）/ 0.5（平）/ 0（负） | 一场一结算，按 `match_record.winner_team` 判该玩家所在队的胜负；平局 `winner_team` 为空 |
| `K` | 定级赛结束后前 20 场 `K = 40`，之后 `K = 32` | 档位按 `rating_games` 判；两端同值，不做平台差异 |
| 更新式 | `R' = R + K × (S − E)` | 按场结算，写回 `player_stat.rating` |

*表三：`rating → rank_level` 阈值表*（**占位，阈值 M1 首轮实测后锁定**；本节只定「由 `rating` 映射到 `rank_level` 1–5」这条规则，不预先写死数字）：

| `rank_level` | 所需 `rating`（累计） |
| --- | --- |
| 1 新手 | 待实测锁定 |
| 2 初级 | 待实测锁定 |
| 3 中级 | 待实测锁定 |
| 4 高级 | 待实测锁定 |
| 5 大师 | 待实测锁定 |

`rating` 与 `rank_level` 按场结算，写入 §6.6 的结算事务；定级期（`placement_left > 0`）只累计 `rating`、不改 `rank_level`、不进排行；`rating_games` 用于判定 `K` 档。评分防刷分口径见 §8.7；`human-ranked` 之外的对局按 §5.15 真值表判定。

### 5.14 生命周期与升级路径

`lifecycle` 取值为 `[deal, tribute, play, settle]` 四段，**v1 只有这四段**；`turn` 是 `play` 内的轮次子阶段，不是 phase。`deal`/`settle` 为零动作 phase，由 `deal.shuffle` 与结算纯函数自动执行，豁免 `legalFrom` 指向要求；`tribute`/`play` 两个有动作的 phase 必须各有至少一条 `legalFrom` 指向它。四段的进入条件、退出条件、超时行为在 §6.3；每段允许的动作集合由 §5.1 的 `actions[].legalFrom` 定义（取值是 `patterns`、某个 zone 的 id 或某个 phase 名，不写自由字符串）。`window`/`arbitrate` 不进 v1 的类型与枚举，v2 立项前纸面推演确认后按 engine 次版本加回（R13）。

### 5.15 玩家侧接口（账号 / 大厅 / 匹配 / 合规）

§5.6 只有后台接口，玩家侧此前只有一句"`GET /games` 被顺带提到"。补齐如下（REST 用 JSON，牌桌仍走 §5.9 的 WS 协议）：

| 接口 | 入参 | 出参 | 语义 |
| --- | --- | --- | --- |
| `POST /api/auth/sms` | `{phone}` | `{requestId, cooldownSec}` | 发验证码；同一手机号 60 秒一次、每日 10 次上限，超限 `RATE_LIMITED` |
| `POST /api/auth/login` | `{phone, code, clientVersion}` | `{token, refreshToken, expiresIn}` | 校验码登录；`clientVersion` 低于 `minClientVersion` 返回 `CLIENT_TOO_OLD` |
| `POST /api/auth/refresh` | `{refreshToken}` | `{token, expiresIn}` | 换新 access token；旧 refresh 立即失效（不允许多端并行刷新同一 refresh） |
| `GET /api/games` | — | `[{gameId, activeVersion, minClientVersion}]` | 大厅玩法列表（§5.6 同一实现） |
| `GET /api/profile` | — | `{playerId, nickname, phoneTail4, stats[{gameId, rankLevel, rating, placementLeft}]}` | 个人资料；`phone_tail4` 来自 `player_profile`（§5.8） |
| `POST /api/match/queue` | `{gameId, mode: ranked\|casual\|mixed}` | `{ticketId}` | 进匹配队列；`mode → matchType` 映射与计段位/排行/写统计真值表见本节末附表 |
| `DELETE /api/match/queue/{ticketId}` | — | `{ok}` | 退出队列 |
| `GET /api/match/queue/{ticketId}` | — | `{state: waiting\|matched\|timeout, matchId?, roomAddr?}` | 轮询结果（v1 不推 WS 匹配事件，降低一条长连接） |
| `POST /api/room/create` | `{gameId, mode, seatsCount}` | `{matchId, inviteCode?}` | 建房；`seatsCount` 取 RuleSet 的 `seats.count`，不由客户端随意指定 |
| `POST /api/room/join` | `{inviteCode}` | `{matchId, seat}` | 凭邀请码进房；座位由服务端分配并返回（§5.9） |
| `GET /api/history/matches` | `{gameId?, cursor, limit}` | `[{matchId, matchType, endReason, winnerTeam, startedAt, rounds}]` | 按玩家分页返回战绩摘要（`match_record`/`round_record` 聚合） |
| `GET /api/replays/{matchId}` | — | 脱敏回放包或下载地址 | 只对已结束房间返回；玩家读取本人回放不属敏感导出（§8.3） |
| `GET /api/compliance/status` | `{playerId}` | `{verified, minor, dailyMinutesLeft}` | **合规适配层的唯一接口**（§2.1 的"接口占位 + 开关"）：开关关闭时恒返回 `{verified:true, minor:false, dailyMinutesLeft:-1}`，U12 答复为"要接"后由适配器填充真实值 |

**错误码**：以上接口全部复用 §5.7 错误码**全表**，不另立一套。

**`mode → matchType` 映射与分流真值表**（`matchType` 定义在 §5.8 `match_record`，其它地方引用不得另造）：

| 入队 `mode` | `matchType` | 写 `player_stat` | 进排行榜 | 计段位 |
| --- | --- | --- | --- | --- |
| `ranked` | `human-ranked` | ✓ | ✓ | ✓（按 §5.13：定级赛按胜率落 1–4，之后 Elo 浮动） |
| `casual` | `human-casual` | ✗（只写 `round_record`） | ✗ | ✗ |
| `mixed`（含 bot 补位） | `mixed` | ✗ | ✗ | ✗ |
| 纯 bot / 离线局（`ai-practice`） | `bot` | ✗ | ✗ | ✗ |

**「计段位」一列按 §5.13 的三张表判**：`human-ranked` 的玩家在 `placement_left > 0` 时只累计 `rating`、`rank_level` 定格为定级分档结果，定级结束后按 Elo 浮动；定级赛只认 `human-ranked`，`ai-practice`（人机房）与自定义房不计入定级赛；其余 `mode` 一律不计段位、不写 `rating`。

`human-ranked` 之外的房间不写段位与排行字段；离线局不上云、不计战绩（§6.6 三档可信度）。

### 5.16 `minClientVersion` 与版本下限

`minClientVersion` 是**独立配置项**（后台可改、落审计），不由 RuleSet 的 `interpreter` 区间推导——两者含义不同：`interpreter` 是引擎侧 semver 区间，`clientVersion` 是客户端构建版本，二者之间没有任何可计算的映射。

- 取值形态：`{platform, minVersion}` 数组（四个平台各一条），`GET /api/games` 按请求方平台取对应值下发。
- 上调时机：协议 sunset 后（§9.2）、或旧版本出现无法在服务端兜底的问题（如 wasm 语义依赖）。
- 上调后旧客户端行为：`JoinRoom`/`login` 直接返回 `CLIENT_TOO_OLD`，客户端只展示"请升级"页，不再进入大厅。
- 下调不提供（历史上调即不可回退）。



## 6. 失败与一致性

### 6.1 失败面在哪

| 失败点 | 典型表现 | 影响面 | 严重度 |
| --- | --- | --- | --- |
| 客户端断网 / 切后台 | 连接静默断开，倒计时继续走 | 单个座位 | 中 |
| 消息重复或乱序 | 同一步动作被提交两次；旧 `seatSeq` 覆盖新 `seatSeq` | 单个座位判定 | 高（会造成非法动作） |
| 伪造座位 | 请求携带的 `seat` 与 token 绑定座位不符，试图取他人视图 | 单座位判定归属 | 高（越权读取） |
| 房间进程崩溃或重启 | 内存态全丢 | 该进程上所有房间 | 高 |
| Redis 不可用 | 快照写不进、匹配队列失效 | 新建房间与恢复 | 中 |
| PG 不可用 | 结算落库失败 | 单局结算与积分 | 高 |
| 规则发布出错 | 新房间规则异常 | 灰度范围内房间 | 高 |
| 客户端版本过旧 | 解析不了新协议字段 | 单端用户 | 中 |

### 6.2 连接与超时

- **心跳**：客户端每 15 秒发一次 `Heartbeat`；服务端以"该座位最后一次收到任何消息"为水位，**30 秒未收到**判该座位掉线（判定动作只有这一处，客户端不做判定）。
- **宽限期**：掉线后房间不销毁，进入 90 秒宽限期；期间该座位状态标 `offline`，倒计时照常推进，超时按 §6.3 处理。
- **重连**：客户端带 `matchId + seat + lastSeatSeq` 重连，服务端下发快照 + 增量。环形缓冲按 `roomSeq` 索引，保留最近 200 条事件，缓冲起点为缓冲内最小 `roomSeq`；**若该座位 `lastSeatSeq` 对应的 `roomSeq` 早于缓冲起点**（长局 300 步或长时间断线；座位序号与房间序号的换算按动作记录里的双序号列，见 §6.5），服务端不回放增量，直接下发新快照，`RoomSnapshot.snapshotReset = true`，令客户端丢弃本地状态并接受新快照。
- **序号作用域**：`seatSeq` 是**每座位单调递增**的，服务端按 `(matchId, seat)` 维护水位；广播顺序用 `roomSeq`（房间内全局递增）。两者字段名不同（§5.9），不混用。`STALE_SEQ` 只按座位水位判定，避免 4 人并发出牌互相误判过期。
- **时钟**：所有计时以服务端单调时钟为准；客户端只做展示，不参与判定。倒计时用房间 tick 驱动，不用系统时间差。

### 6.3 超时与托管

**计时器参数与优先级**（原来只有名字没有秒数，这里补齐；tick 频率 10Hz，所有计时以 tick 计）：

| 计时器 | 参数 | 触发后 | 优先级 |
| --- | --- | --- | --- |
| 单步思考 | `timing.turnTimeSec`，默认 30 秒（取值 5–120，§5.1） | 计 1 次超时，自动执行 §6.3 第 1 行 | 3 |
| 掉线宽限 | `timing.graceSec`，默认 90 秒（取值 30–180） | 座位转托管，房间继续 | 2 |
| 全员离线 | `timing.offlineAllSec`，默认累计 5 分钟（取值 60–600；掉线宽限不计入，从全员 `offline` 起算） | 整场判流局 | **1（最高）** |
| 托管累计 | 同一座位累计 3 局（局，不是轮，见 §1.6） | 移出房间，座位释放 | 4 |

优先级高者先判定：全员离线 5 分钟到点时，即使托管 AI 正在代打，也判流局，且补偿记录只落一条。

| 情形 | 处理 | 可逆性 |
| --- | --- | --- |
| 单步超时 1 次 | 自动执行"过"（无过则出最小合法牌：牌力升序、同牌力按牌 id 升序，见 §5.10 末行） | 玩家回来后恢复手动 |
| 连续 2 次超时 | 进入托管，由 AI 代打（只用 `view(seat)`）；"连续"以同一座位相邻两次行动为界：中间正常出牌一次即清零，跨局清零 | 玩家可随时取消托管 |
| 托管满 3 局 | **局间**移出房间，座位开放给其他玩家或 bot；新入座者继承该座位所在队的 `levels`（队级牌是队的属性，不随人走），战绩按座位累计；该座位已产生的局照常结算 | 不可逆，需重新进房 |
| 全员离线超过 5 分钟 | 整场判"流局"，不计分、不扣分 | 不可逆 |

**竞态的唯一裁决规则**：动作与超时同 tick 到达时，以"服务端单写者队列的入队顺序"为准——房间所有输入（玩家动作、tick、系统事件）都进同一个串行队列，队列里先执行的先生效；若动作在"超时事件"入队之前已入队，则动作有效且不判超时，反之判超时并丢弃动作，返回 `TOO_LATE`（§5.7：HTTP 409，不推进 `seatSeq`、不计入该座位超时次数，客户端按座位水位重拉增量）。同 tick 内**不按时间戳比较**，避免时钟粒度造成两端结论不同。

### 6.4 重复投递与幂等

所有改变状态的请求都带 `(matchId, seat, requestId)`，幂等键是 **`requestId`**（客户端生成，UUID v4），不是 `actionId`：

- `actionId` 在 RuleSet 里是**动作类型**（`play` / `pass`），一局内会重复 N 次；用它去重会让同一局第二次出牌被判成重复并直接返回上一次的回执，牌局当场卡死。
- 服务端以 `requestId` 去重，重复到达返回原处理结果（`200 + 原 response`），不重复执行。
- **去重窗口**：`requestId` 的去重表只保留"本局（round）+ 该局结束后 5 分钟"，随局结束清理；`requestId` 同时写进 `ActionRecord`，所以崩溃恢复重放后去重表能从动作日志重建（不需要单独的持久化表）。
- `seatSeq` 是每座位单调递增的执行序号，只用于**排序与水位**，不承担去重；`seatSeq` 小于该座位水位时返回 `STALE_SEQ` 并令客户端重新拉增量。
- 客户端 3 秒未收到回执会重发**同一个 `requestId`**，因此幂等是硬要求，不是优化。

### 6.5 房间进程崩溃后的恢复

**核心优势**：引擎确定性让"重放"成为可行恢复手段，不必依赖完整快照。

| 手段 | 频率 | 内容 | 恢复耗时（估算） |
| --- | --- | --- | --- |
| 动作日志 | 每步动作后追加到 Redis Stream | `ActionRecord = {seatSeq, roomSeq, seat, requestId, kind, payload, tsMs}`，与回放包 `actions.jsonl` 同一结构（**只此一份**，不再另立第二个记录类型）；重放水位一律用 `roomSeq` | — |
| 房间快照 | 每 30 秒 + 每阶段结束 | `{seed, rulesetVersion, phase, turnSeat, levels, seats[seat], zones[(seat, zoneId, cards)], lastPlay, seqWatermark[], tribute, rng, snapSeq}`——对应 §5.12 第 1–8 项字段（版本串除外），再加 `seed`/`snapSeq`；`tribute` 段进贡结束后归零但字段位保留。**快照字段集与 `stateHash` 字段集必须一一对应**，少一个字段就等于少一段可重建状态 | — |
| 重建方式 | — | 载入快照（已含区域内容到 `snapSeq`）→ 从 `snapSeq+1` 重放到 `lastRoomSeq` → 校验 `stateHash` | 重放步数 ≤ 快照间隔内的动作（通常 < 30 步），预期 < 100ms；最坏全量 300 步 < 200ms |

**恢复失败的处置**：若日志与快照对不上（`stateHash` 不一致），不允许"修补"，直接判该场流局；补偿记录先落 `settlement_compensation` 表（`issued=false`，不发放；支付接口启用后才发放，见 §5.8）。理由：宁可判流局，也不能让两个客户端看到不同的牌。`settlement_dead_letter` 只用于结算补写，不承载流局补偿。

### 6.6 结算的原子性与对账

结算涉及三件事：战绩、积分、房间状态。顺序固定为：

1. 计算结算（纯函数，输入 `state`，输出 `Settlement`）；
2. 以 `settlementId = f"{matchId}:{roundNo}"`（带分隔符，避免 `m1`+`23` 与 `m12`+`3` 撞键）为幂等键写入 PG（事务内写 `round_record` 与 `player_stat`）；
3. 写入成功后才把房间标为 `round-ended`，并广播结算结果；
4. 写库失败则房间停在 `settling`，后台任务按指数退避重试（上限 **3 次**，不是 10 次）；3 次仍失败即**放弃自动重试**：房间转 `settle-failed`，广播"本局结算失败（可重试）"并允许开下一局，失败局写进 `settlement_dead_letter` 表由人工按 `settlementId` 补写，补写成功后才补算战绩与积分——不引入"永久卡在结算中"的状态。

**对账任务**：每日跑两份报表，**只比条数，不比百分比**（"版本号偏差 0.1%"这类说法无法定义分母，原稿作废）：
1. 条数等式：`round_record` 条数 + `settlement_dead_letter` 条数 = 房间结算事件条数。结算事件由服务端在结算事务内落 PG 计数（不依赖日志导出与对象存储；`settlement_dead_letter` 行尚未计入 `round_record`，故在左侧相加）；差异条数 > 0 即告警；
2. `order.status=paid` 金额合计 vs `payment_callback` 验签通过金额合计，差异金额 > 0 即告警（v1 无真实订单，该报表恒为空，**不列入 P0 清单**）。

只告警、不冻结：首版没有值班团队，"冻结当日结算修正"没人解冻，反而会卡住次日结算（见 §7.5）。

**按 `matchType` 分流**：`matchType` 取值 `human-ranked` / `human-casual` / `bot` / `mixed`，定义在 `match_record`，计段位、进排行、写 `player_stat` 的口径以 §5.15 真值表为准：只有 `human-ranked` 写 `player_stat`、进排行、计段位；其余类型只写 `round_record`。匹配两池分开，`mixed` 由"不足 4 人 bot 补位"产生，UI 必须标注"AI 陪练"。

**离线局的三档可信度**（原稿"所有判定都在服务端"与 G3 的离线要求直接冲突，这里分级）：

| 档 | 产生方式 | 判定主体 | 上云 | 计战绩 |
| --- | --- | --- | --- | --- |
| `online-human` | 4 真人联机 | 服务端 | 是 | 计 |
| `online-bot` | 联机且含 bot 补位 | 服务端 | 是 | 不计（`bot`/`mixed`） |
| `offline-local` | 完全离线与 bot 对打 | 客户端 wasm（不可信） | 否，只存本地 | 不计 |

离线局不存在"服务端权威"这个前提：客户端 wasm 可被改写，本地结果只能当娱乐记录，界面明示"本地对局，不计战绩"。

### 6.7 依赖不可用时的降级

| 依赖 | 不可用时 | 做什么 | 不做什么 |
| --- | --- | --- | --- |
| Redis | 匹配队列失效、快照写不进 | 停止接受新建房间；已在内存运行的房间继续打完 | 不重启进程（重启必丢局） |
| PG | 结算与战绩写不进 | 房间停在 `settling` 重试，玩家可见"结算中" | 不判玩家负、不扣分 |
| 规则服务（发布链） | 无法发布/回滚 | 现有 `active` 版本继续服务 | 不自动切换版本 |
| 短信服务商 | 收不到验证码 | 已登录用户不受影响；新注册降级为"管理员手动发码" | 不放开无验证登录 |

邀请码通道不是子系统，是**一条手动兜底**：管理员每次在后台点"发一个码"，系统生成 8 位大写字母数字、有效期 7 天、一码一用，兑换时绑定手机号并写审计。**不做批量生成、不做批次管理**（§8 的 YAGNI 剪枝）：内测期每天新增用户以个位数计，手动发码足够，批量子系统（批量、配额、批次表）是纯负担。表结构见 §5.8 的 `invite_code`（去掉 `batch_id`）。

### 6.8 一致性边界（哪些必须强一致）

| 数据 | 一致性要求 | 实现 |
| --- | --- | --- |
| 牌局判定、`stateHash`、结算结果 | 强一致 | 单房间单写者 + 确定性引擎 |
| 积分与段位变动 | 强一致 | 同结算事务内写 |
| 战绩列表、排行榜 | 最终一致（秒级） | 异步投影，允许短暂延迟 |
| 审计日志、对局日志 | 最终一致 | 批量落盘，允许分钟级延迟 |
| 回放式观战（只看已结束房间） | 最终一致 | 结算落库后由回放包查询接口提供，**不增加房间广播扇出**；不做实时旁观（U8） |



## 7. 可观测性

### 7.1 三个必须回答的问题

| 问题 | 靠什么回答 | 本项目的最低要求 |
| --- | --- | --- |
| 出问题了吗？ | 指标 + 阈值告警 | 5 个 P0 信号必须能在 1 分钟内触发 |
| 问题在哪？ | 结构化日志 + 每局 trace | 任意一局能在 30 秒内定位到"哪一步、哪条规则、哪个座位" |
| 怎么复现？ | 一键导出回放 | 在**有对应镜像的机器上**可完整重放，`stateHash` 逐步对齐；镜像超出 §9.3 的 3 个组合窗口时降级为"数据级重放"（导出 actions + 版本号，需本地构建对应 tag） |

### 7.2 指标清单

| 指标 | 类型 | 维度 | 阈值与动作 |
| --- | --- | --- | --- |
| `statehash_mismatch_total` | Counter | `match_id, seat` | **>0 即 P0**：10 分钟内出现任意一次，立即告警并自动导出该局回放 |
| `viewhash_mismatch_total` | Counter | `match_id, seat, room_seq, client_platform` | **>0 即 P1**：客户端自报的 `viewHash` 仅作定位线索，按 `(matchId, seat, roomSeq)` 去重、单房间设上报上限；P0 只保留服务端重算视图与已下发值不一致（计入 `statehash_mismatch_total` 口径） |
| `action_illegal_total` | Counter | `ruleset_version, action_kind` | 5 分钟窗口内 非法动作 / 该窗口**总动作数** >0.1% → P1（分母见下） |
| `action_cpu_overrun_total` | Counter | `ruleset_version` | >0.1% → P2；**不进非法率、不参与 canary 判定、不作回滚条件**（§5.6/§8.6 同一口径） |
| `action_quota_exceeded_total` | Counter | `ruleset_id` | dry-run / 发布校验候选枚举超 4096 上限即记一次 → P2；运行期不再按步拒绝（§5.7 `RS_QUOTA_EXCEEDED`） |
| `room_panic_total` | Counter | `ruleset_version` | 解释器 native 侧 panic 被 `catch_unwind` 捕获即记一次 → P2，随 `process_crash_total` 一并观察 |
| `action_ack_p99` | Histogram | `game_id` | >300ms 持续 5 分钟 → P2（体验预警） |
| `room_active` | Gauge | `game_id, ruleset_version` | 相对**前 15 分钟滑动均值**下跌 30% → P1 |
| `room_create_fail_rate` | Ratio | `reason` | >5% → P1 |
| `reconnect_success_rate` | Ratio | `client_platform` | <95% → P1 |
| `settlement_pending_age` | Gauge | `match_id` | 单局 >60 秒 → P1（结算卡住） |
| `process_crash_total` | Counter | `instance` | 崩溃率 = 崩溃次数 / 该实例承接**房间创建次数** >0.1% → P1，同时是 §9.5 回滚判据 |
| `ruleset_validation_fail_total` | Counter | `ruleset_id, level` | >0 → P2（研发关注） |
| `canary_illegal_rate` | Ratio | `ruleset_version` | 分母 = **该版本新建房间的动作数**；>0.05% → **自动回滚**（§5.6） |
| `replay_export_total` | Counter | `trigger` | 用于观察人工排查频率 |

**"非法率"只在这里定义一次**：分子 = 引擎判定为非法的动作（含脚本非法），分母 = 同一时间窗的**总动作提交数**；玩家合法提交被拒单独计数观测，不进本非法率分子（与 S4 判据"0 次错误判定"的口径分开：S4 不计玩家误点被拒）。三处使用（P1 告警、canary 自动回滚、L3 dry-run）都必须写明时间窗与分母，dry-run 的分母是干跑产生的动作数。CPU 超预算不计入分子。

**5 个 P0 信号的清单**（原稿只写了"4 个"没列名，这里列全）：`statehash_mismatch_total > 0`、服务端重算 `viewHash` 与已下发值不一致（服务端自证，不靠客户端自报）、结算账不平（对账报表差异非零）、全服无法建房（`room_create_fail_rate` 5 分钟 >50%）、进程崩溃（`process_crash_total > 0` 且伴随房间丢失）。§7.5 的 P0 行与本清单必须逐项一致。

### 7.3 结构化日志的固定字段

每条涉及对局的日志都必须带齐下列字段，缺任一字段视为埋点缺陷：

| 字段 | 示例 | 用途 |
| --- | --- | --- |
| `trace_id` | `gf-7f3a91` | 一局一个，串起所有日志 |
| `match_id` / `round_no` | `m_10293` / `3` | 定位到局与轮 |
| `seat` | `2` | 定位到人 |
| `roomSeq` | `47` | 定位到步（房间广播序；座位动作序另见 `seatSeq`） |
| `action_kind` | `play` | 定位到动作类型（不是唯一动作，幂等键另见 `requestId`） |
| `ruleset_version` / `interpreter_version` / `engine_version` | `3 / 1.2.1 / 0.9.0` | 定位到规则版本（跨版本问题是排查难点） |
| `elapsed_us` | `1840` | 定位到性能 |
| `state_hash` | `9c1e…` | 定位到状态漂移 |

### 7.4 追踪粒度

- 一局 = 一个 trace，每步动作 = 一个 span，span 内再分 `legal_check` / `apply` / `broadcast` 三段。
- 采样率：**成功动作 10%，异常与慢动作 100%**（非法动作、`elapsed_us > 5ms`、`statehash` 不一致、结算失败必全采）。理由：100% 全采在首年 DAU <1 万、无值班团队的前提下只会撑大存储，真正要看的局必须有留档，靠"异常全采"覆盖。
- 追踪数据与回放包同源，导出时一并打包。

### 7.5 告警分级（没有值班团队，所以按"会不会停下来"分级）

| 级别 | 判定 | 渠道 | 响应要求 |
| --- | --- | --- | --- |
| P0 | `statehash_mismatch_total>0`、服务端重算 `viewHash` 与已下发值不一致、结算账不平、全服无法建房、进程崩溃且房间丢失（与 §7.2 清单逐项一致） | 电话/短信 + 飞书 | 立即处理，其他事先停 |
| P1 | 非法动作率超标、重连成功率跌破、结算挂起 | 飞书 + 邮件 | 当日处理 |
| P2 | 延迟升高、校验失败、体验预警 | 飞书日报 | 次日排期 |

P0 之外不做自动干预，只有两项自动动作：`canary` 指标超标自动回滚，`statehash` 不一致自动导出回放。

### 7.6 四条排查路径（写成 checklist，出问题照着走）

| 场景 | 第一步 | 第二步 | 第三步 |
| --- | --- | --- | --- |
| 玩家报"出不了牌" | 用 `matchId + roomSeq` 查日志，看 `legal_check` 的拒绝原因 | 比对客户端提交的 `card_ids` 与服务端手牌 | 若手牌不一致 → 查 `statehash_mismatch`，按漂移处理 |
| 两端看到的牌不一样 | 取该局 trace，比对服务端逐步 `state_hash`（用于漂移检测）与两侧 `viewHash` 上报记录（用于首处分歧定位）；两类哈希先按 §5.12 投影后再相比 | 定位第一个分歧的 `roomSeq` | 导出回放，用离线重放器复现 |
| 结算对不上 | 查 `settlement_pending_age` 与 `round_record` | 核对 `settlement_id` 是否重复写入 | 跑当日对账报表，看条数或金额差异 |
| 发布后新房间异常 | 看 `canary_illegal_rate` 与 `room_active` 按版本的变化 | 确认是否已自动回滚 | 用 L3 dry-run 复跑该 RuleSet，定位具体原语 |

### 7.7 一键导出回放包

一个 `.zip`，内容是：

| 文件 | 内容 |
| --- | --- |
| manifest.json（重放包内） | `matchId`、`seed`、三个版本号、座位与玩家标识（脱敏） |
| `actions.jsonl` | 逐条动作，与 §6.5 的 `ActionRecord` **同构**：`seatSeq / roomSeq / seat / requestId / kind / payload / tsMs`；两列序号都显式保留，重放水位一律用 `roomSeq`，分歧步按 `roomSeq` 对齐 `hash_trace.jsonl` |
| `hash_trace.jsonl` | 每步的 `stateHash` |
| `logs.txt` | 该局的服务端日志（已裁剪到本局 trace） |
| client_summary.json（重放包内） | 各端上报的客户端版本、平台、最后确认的 `roomSeq` 与 `seatSeq` |

导出即完成，不依赖线上环境；离线重放器（`tools/replay-viewer`）读这个包就能复现。

### 7.8 数据保留

| 数据 | 保留期 | 理由 |
| --- | --- | --- |
| 结构化日志 | 14 天 | 排查窗口足够，成本可控 |
| 回放包（自动导出的异常局） | 90 天 | 覆盖投诉与复盘周期 |
| 审计日志（规则变更、发布、回滚） | 1 年 | 商业化后合规与追责需要 |
| 对局记录（`round_record`） | 长期 | 战绩与对账的基础 |



## 8. 安全与权限

### 8.1 角色与权限矩阵

| 动作 | 管理员（产品负责人本人） | 研发 | 只读（给外部审计/临时旁观用） |
| --- | --- | --- | --- |
| 查看 RuleSet 与房间 | ✓ | ✓ | ✓ |
| 编辑 RuleSet 草稿 | ✓ | ✓ | ✗ |
| **发布 / 回滚** | ✓（发布须 TOTP；回滚免 TOTP） | ✗ | ✗ |
| 导出回放包 | ✓ | ✓ | ✗ |
| 踢人 / 封号 | ✓ | ✗ | ✗ |
| 改积分（人工补偿） | ✓（须填理由，进审计） | ✗ | ✗ |
| 结算死信人工补写 | ✓（按 `settlementId`，进审计） | ✗ | ✗ |
| 手动发码 | ✓（一码一用，进审计） | ✗ | ✗ |
| 查看审计日志 | ✓ | ✓ | ✓（只读，不导出） |
| 管理后台账号 | ✓ | ✗ | ✗ |

**为什么砍到三个角色**：2 人团队里，"运营"就是产品负责人，"客服"没有工单系统可挂（§2.2 明确不做工单），五个角色矩阵是纸面设计。**"只读"保留但用途写实**：它给外部审计或临时协助者（例如代办、投资人代看数据）用，一次性开通、到期回收；它不是"研发的降级版"，研发能导出的回放它不能导出（§8.3）。

**单人也能防误操作**（这是本设计的真实护栏，不是"双人复核"）：发布必须过 TOTP + 强制填写 `reason` + 发布前后各自动导出一次当前 `active` 版本；回滚一键可达且不需二次确认（回滚是安全方向，加摩擦反而延误）。"编辑与发布分离"在 2 人团队里做不到，文档不再声称做到。

**导出规则（唯一口径，§8.3 与本表必须一致）**：回放含他人手牌，属敏感数据。首版只允许**管理员与研发**导出，导出即写 `audit_log`（`actor, target, ip, session_id`）。"工单关联的客服"这一条件随工单系统一起删除，全文档不再出现。

### 8.2 后台鉴权

| 项 | 做法 | 理由 |
| --- | --- | --- |
| 入口隔离 | 后台独立子域名，默认不暴露公网；上线时走 IP 白名单或 VPN | 后台一旦可公网访问，攻击面等于整个规则发布链 |
| 登录方式 | 手机号 + TOTP（不共用玩家的账号体系与 token） | 玩家 token 泄露不应导致能改规则 |
| 会话 | 8 小时过期，操作前再校验一次角色 | 防止长期挂着的后台会话被利用 |
| 限流 | 登录 5 次/分钟；发布 10 次/小时 | 降低误操作与撞库风险 |

### 8.3 越权防护

- 全部后台 API 在服务端做角色校验，**不依赖前端隐藏按钮**；前端隐藏只是体验，不是权限。
- 玩家侧接口只能按 `seat` 读取自己的 `view`；任何"按 matchId 拉全量手牌"的接口都不存在（`S7` 约束的延伸）。
- 导出回放包含他人手牌，属敏感操作：仅管理员与研发可导（与 §8.1 矩阵一致），导出行为进审计，客服角色不存在故无"工单关联"条件。**例外**：玩家经 `GET /api/replays/{matchId}` 读取本人已结束房间的回放，不属敏感导出；该接口只返回脱敏回放包或下载地址。

### 8.4 数据最小化与脱敏

| 数据 | 处理 |
| --- | --- |
| 手机号 | 业务表存哈希 + 尾号 4 位；明文仅存在于短信发送链路的内存中；哈希算法与盐策略见 §5.8 `player_profile`（HMAC-SHA256 + `server_salt`，盐轮换走双盐列/双次查找窗口） |
| 玩家标识（导出包） | 替换为 `p1..pN` 别名（N = `seats.count`），保留座位映射表在服务端 |
| 设备信息 | 只存平台、系统版本、屏幕尺寸，不采集 IMEI/MAC/通讯录 |
| 支付信息 | 不落库明文，只存渠道订单号与状态；卡号/凭证一律不进入本系统 |
| 日志 | 手机号、昵称不进日志正文；需要时用哈希前 8 位 |

### 8.5 密钥与凭据

- 短信 AK/SK、PG 密码、JWT 签名密钥全部走环境变量或密钥管理服务，仓库里只留 `.env.example`。
- JWT 采用短期 access（15 分钟）+ 长期 refresh（30 天，可吊销）；签名密钥支持双密钥轮换，旧密钥保留一个周期。
- 客户端不内置任何服务端密钥；wasm 包可被反编译，因此**联机对局的所有判定都在服务端**。离线局是例外：判定在客户端，故按 §6.6 的三档可信度降级处理（不计战绩、不上云）。

### 8.6 沙箱边界（RuleSet 不能变成后门）

| 风险 | 防护 |
| --- | --- |
| RuleSet 注入任意代码 | 类型系统拒绝（§5.3）；解释器只认固定原语 |
| CPU 炸弹（超大枚举、无限递归） | 候选枚举 4096 条为**加载期/发布期静态上限**：超限拒绝加载或拒绝发布（`RS_QUOTA_EXCEEDED` 只用于 dry-run 超限）；运行时单动作预算 **5ms**，超预算记 `action_cpu_overrun_total`（P2），**不判该动作非法、不按步拒绝**——合法性只由规则决定，不能由机器快慢决定 |
| 解释器 panic 拖垮进程 | 服务端 native：动作执行包在 `catch_unwind` 内，panic 转为"该动作失败"并记 `RS_INTERNAL_PANIC`；**wasm 侧没有 unwind**（`panic=abort`），策略是"panic 即整实例重载"。S1 的例外条款写在 §1.4 里（`panic-injection` 用例不参与双端同哈希判定），本表不另立例外 |
| 恶意 RuleSet 拖慢全体 | v1 单实例 + 网关粘性路由，不做房间迁移（§4.5）。CPU 超预算只记 `action_cpu_overrun_total` 并走 P2 告警（§7.2），**不作 canary 回滚条件、不对房间限流**；房间级动作频率仅计数观测，不阻断动作 |
| 越权读取他人手牌 | `view.project` 是唯一出口，引擎不提供全量视图 API |

### 8.7 玩家侧反作弊

| 手段 | 说明 | 边界 |
| --- | --- | --- |
| 服务端权威 | **联机对局**的判定、计时、发牌都在服务端 | 已由架构保证；离线局按 §6.6 降级为本地可信度最低档 |
| 信息裁剪 | 只下发 `view(seat)` | 已由 §5 保证 |
| 异常行为检测 | 出牌耗时分布、连胜异常、同 IP 多账号、`statehash` 不一致计数 | 只做打点与告警，不做自动封禁 |
| 客户端完整性 | 上报客户端版本与包哈希，异常版本进灰度名单 | 不做强校验（会误伤绕开渠道更新的用户） |
| 记牌器 | 属"用公开信息辅助"，不封堵 | 这是合规能力，不是外挂 |
| 评分防刷分 | 同一对对手 24 小时内重复对局：第 3 场起 `K` 减半、第 5 场起 `K = 0`（该场不写 `rating`）；人机房（`ai-practice`）与自定义房不写 `rating`；断线判负按正常 `K` 计；中途主动退出按判负处理、`K` 不变（不做惩罚加倍） | 只改 `rating` 的实际变动量，不改判定与结算口径（§5.13、§6.6） |

### 8.8 审计

- 审计表 append-only（只允许 INSERT，禁止 UPDATE/DELETE），字段与 `audit_log` 表**完全一致**：`ts / actor / action / target / before / after / ip / session_id`（三处不再各说一套）。
- 必审动作：RuleSet 编辑与发布、回滚、踢人封号、人工改分、导出回放、后台账号变更。
- 审计日志保留 1 年（§7.8），并且与业务表分开存储，防止被业务侧误删。

### 8.9 供应链与依赖

- Rust 侧：`cargo deny` 进 CI，禁 GPL 类许可、禁已知高危 advisory；依赖版本锁在 `Cargo.lock`。
- 前端侧：`pnpm audit` + lockfile 提交；不引入来源不明的 npm 包。
- 部署：容器镜像用 digest 固定，不用 `latest`；构建产物带 SBOM。



## 9. 迁移、回滚与兼容

### 9.1 要处理的"迁移"其实是五条版本线

新项目没有旧系统要迁（§3.1），真正的迁移风险来自版本线之间的错配：

| 版本线 | 变更频率 | 谁改 | 兼容对象 |
| --- | --- | --- | --- |
| `engine` | 低（原语库变更） | 研发 | 解释器、回放、客户端 wasm |
| `interpreter` | 中（DSL 语义变更） | 研发 | RuleSet、回放 |
| `ruleset` | 高（运营发布） | 运营 | 只影响新房间 |
| `protocol` | 中（字段增减） | 研发 | 客户端各端壳 |
| `db schema` | 低 | 研发 | 无外部调用方 |

### 9.2 兼容规则矩阵

| 变更类型 | 允许的兼容承诺 | 落地动作 |
| --- | --- | --- |
| engine 补丁版本 | 判定结果不变 | 回放 `stateHash` 必须一致，CI 强制 |
| engine 次版本（新增原语） | 老 RuleSet 行为不变 | 老 RuleSet 不引用新原语即不受影响 |
| interpreter 次版本 | 向后兼容老 RuleSet | RuleSet 的 `interpreter` 区间校验 |
| interpreter 主版本 | **不承诺兼容** | 强制要求老 RuleSet 迁移，或冻结旧解释器版本继续服务（双版本并存上限 3 个月） |
| RuleSet 新版本 | 只影响新房间 | 运行中房间锁版本（§5.5） |
| 协议字段 | 只增不删、不改语义，**但每次增字段同时登记 sunset 条件** | 服务端按 `clientVersion` 做特性开关；**sunset 规则**：某字段上线满 3 个月且旧版本客户端占比 < 1%，即可删除该特性开关与旧字段，`minClientVersion` 同步上调（§5.16），不承诺无限期维护 N 套分支 |
| DB 表结构 | 只前滚迁移 | 破坏性变更走"影子表 → 双写 → 切换 → 观察 → 删旧"五步 |

**最贵的一条**：`interpreter` 主版本变更会让所有历史 RuleSet 失效。因此规则语言的语法要一次设计到位（`A14` 的风险点），而**解释器版本存档**是兜底：保留旧解释器二进制，老 RuleSet 按 `ruleset.pinned_interpreter_version` 选择对应的旧解释器；`interpreter` 区间内多版本并存时，一律以发布时锁定的 `pinned_interpreter_version` 为准。

### 9.3 回放可读窗口与镜像存档

回放包保留 90 天（§7.8），但**不保留每个版本的可运行镜像**——那是给有值班团队、数十万 DAU 的产品定的规矩，本项目没有。这里的可执行做法是：

- 只保留**最新 3 个 `engine + interpreter` 组合**的容器镜像（按 digest 固定），够覆盖 90 天内的绝大多数投诉局。
- 超出 3 个组合的旧局，只能做"数据级重放"（导出 `actions.jsonl` + 版本号），复现依赖本地构建对应 tag，不保证开箱即用。
- 离线重放器按 manifest.json（重放包内）里的版本号选择镜像；找不到镜像时明确报"版本镜像已下线"，而不是静默失败。
- 折中理由：镜像存档是纯运维成本，90 天 × 每次发版 × 3 个产物会线性堆空间，而首年实际投诉量按 DAU 1 万估是几十起。

### 9.4 发布灰度步骤

| 阶段 | 覆盖 | 观察窗口 | 通过条件 | 失败动作 |
| --- | --- | --- | --- | --- |
| `draft` | 0 | — | L1/L2 通过 | 停在草稿 |
| `validated` | 0 | — | L3/L4 通过（§5.4） | 拒绝发布 |
| `canary` | 新建房间的 10% | 30 分钟 | 该版本新建房间内 非法率 ≤0.05%、`statehash` 不一致 0、服务端复核 `viewHash` 不一致 0、`process_crash` 0；CPU 超预算不计入本判定 | 自动回滚 |
| `full` | 100% | 持续 | 指标与 canary 同口径不恶化（P99 ≤ 发布前 7 日均值 ×1.1） | 回滚到上一 `active` |

三处枚举对齐：本节阶段名、§5.6 接口的 `stage`、`ruleset_release.stage` 用同一套字符串（`draft/validated/canary/full/rolled_back/retired`）。**取消 `ramp5`/`ramp25`**：内测规模下多两级只增加人工推进成本，不增加安全性（§8 的 YAGNI 剪枝）。

**内测初期的样本量问题**：canary 覆盖 10% 新建房间，30 分钟仍可能只出个位数房间。因此增加准入条件：**该阶段样本动作数 < 200 时，不按比率判定，改为"零 P0 事件"判定**，并延长观察窗口至 2 小时；满 2 小时仍 < 200 的，由人工 TOTP 决定放行（`promote` 到 `full`）或回滚，决定与理由落审计（出口条件见 §5.6）。

阶段切换的授权口径（与 §8.1 一致）：`canary` 达标后由人工经 `promote` 接口带 TOTP 确认转 `full`；`canary` 失败时**自动回滚**，不需要人确认，回滚后 10 分钟内人要补一条理由落审计。

### 9.5 回滚

| 项 | 规定 |
| --- | --- |
| 触发条件 | P0 状态漂移；canary 内非法率 >0.05%；`process_crash_total / 房间创建次数 > 0.1%`；当日对账报表差异非零 |
| 决策人 | `canary` 阶段系统自动；`full` 阶段由管理员在 5 分钟内决定 |
| 时间预算 | 从决定到新房间不再使用坏版本 ≤5 分钟（切 Redis 指针即可） |
| 已开局房间 | **不重放、不回滚**，按锁定版本打完；若该版本被判定有致命缺陷，强制流局并按 §6.5 处置：补偿记录落 `settlement_compensation` 表（`issued=false` 不发放）；`settlement_dead_letter` 仅用于结算补写 |
| 回滚后的 RuleSet | `stage` 记为 `rolled_back`（**不是 `retired`**），该版本只读、不可再推进；要改就发新版本。`retired` 只用于"明确下线且不再被任何房间引用" |

### 9.6 客户端兼容

- 协议字段只增不删；服务端对每个请求读客户端上报的 `clientVersion`，决定是否返回新字段。
- 旧客户端遇到新玩法：服务端在 `GET /games` 返回 `minClientVersion`，客户端自行隐藏入口，而不是进房后报错。
- 强制更新的唯一场景：安全漏洞或协议语义变更；此时服务端拒绝旧版本登录并给出下载地址。

### 9.7 数据库迁移纪律

- 迁移脚本入库、编号、可重跑（`sqlx migrate`），禁止手工改库。
- 破坏性变更五步：影子表 → 双写 → 读切换 → 观察 7 天 → 删旧列。中间任何一步失败都可回退到"只读旧表"。
- `ruleset.body` 存 `jsonb` 且不可变，迁移风险天然低；风险集中在 `player_stat`、`round_record` 这类会被聚合的历史表。

### 9.8 为日后接其他平台预留

**只有一项真占位**：`external_identity` 空表 + 唯一索引（列名与 §5.8 一致：`player_id, provider, external_uid, verified_at`）。

其余两项**不做**：数据导入/导出脚本与 `round_record.source` 字段都是当下就要写的代码与 DDL，不是零成本占位；本项目没有任何迁移来源，等真出现迁移需求再按 §9.7 的五步做。



## 10. 测试计划

### 10.1 分层与职责

| 层 | 测什么 | 跑多久一次 | 耗时预算 |
| --- | --- | --- | --- |
| 单元 | 每条原语的语义、解释器的调度、校验器四级的判定；**v1 未启用/未实现的原语分支不计入覆盖率分母** | 每次提交 | <30s |
| 契约 | wasm 与 native 双产物同输入同输出（`stateHash`、`legal_actions`） | 每次提交 | <2min |
| 集成 | 房间统一生命周期枚举（含 `settling`/`round-ended`/`settle-failed` 结算子态，见 §4.5/§6.6）、断线重连、托管、结算落库 | 每次合并 | <10min |
| 端到端 | 四端冒烟（Web / Windows / macOS / Linux）：建桌→发牌→出牌→结算 | 每次发布 | <15min（并行） |
| 性能 | 单动作延迟、**由 A4 推出的峰值房间数 × 3 倍余量**空转、内存增长 | 每晚 + 发布前 | <30min |
| 模糊 | L3 随机对局：死锁、不终局、非法动作 | 每晚 | <60min |
| AI 回归 | bot 对 bot 胜率矩阵（固定种子） | 每周 + AI 改动时 | <20min |
| 故障注入 | 杀客户端、杀房间进程、Redis/PG 断连 | 每周 | <30min |

### 10.2 风险到测试的映射（每个已知风险都要有对应测试，缺一项就是缺口；本条允许一条风险对应多条测试，一对多不算缺口；编号与 §11.1 名册一一对应）

| 编号 | 风险（与 §11.1 同名） | 测试用例 | 通过判据 |
| --- | --- | --- | --- |
| R1 | 规则判定写错 + DSL 表达力不够（两条风险共用一个编号，名册见 §11.1 同名行） | ①黄金用例覆盖牌型全表、进贡/还贡/升级档位残局（§10.3 手工构造 40 条）；②M0 末纸面推演斗地主 RuleSet（不实现）；掼蛋基线 RuleSet 走 L1–L4 | ①与 §5.13 规格逐点一致、逐步 `stateHash` 一致；②纸面完整规则不需新增原语；基线校验全过，否则触发降级（`A14`） |
| R2 | 34～38 周基线不成立 | 第 16 周 gate 验收：Web + 本地 bot 打完一场（含再来一局、杀进程后恢复） | 验收记录入台账；未过即按 R2 缩范围 |
| R3 | 解释器性能不达标 | §10.5 基准 + M3 压测；按 A4 峰值 ×3 倍余量空转 | P99 达预算；实测上限写回 §4.8；房间密度不低于实测上限 60% |
| R4 | 规则语法返工 | 表达力矩阵：掼蛋 + 斗地主（纸面）+ 麻将（纸面）对照原语清单 | 语法冻结评审通过；发布后只做加法 |
| R5 | 商店渠道缺位 | Web 分享链路走查：从链接到打完一局 | ≤3 步，且无需安装 |
| R6 | 2 人上下文过载 | 每两周一次"近两周改动 crate 分布"审查记录；owner 分工表（一人引擎 + DSL，一人平台 + 客户端） | 同一周内无人切换三个以上模块；crate 边界即职责边界 |
| R7 | 状态漂移 | §10.4 双端一致性；断线恢复故障注入（含 `snapshotReset` 分支） | 每步 `stateHash` 一致；恢复后哈希与断线前一致 |
| R8 | bot 体验差 | bot 对 bot 对子胜率：固定种子、固定名单文件 `golden/fast20.txt`（§10.3 注册）；三档各跑固定集 | 对子胜率硬断言：`easy` 对 `normal` ≤30%、`normal` 对 `easy` ≥60%、`hard` 对 `normal` ≥60%；胜率区间随版本调整，测试只断言对子胜率，不冻结算法与区间 |
| R9 | 四端打包与签名卡住 | 四端产物构建与安装冒烟（§10.7） | 四产物均可安装并进房 |
| R10 | 素材授权问题 | `../../assets/license.md` 全表可查 + 抽样核对许可原文 | 0 条缺原文 |
| R11 | 人机混合房引发不满 | 集成用例：`mixed` 房结算后查 `player_stat`；AI 座位标注走查 | 该表 diff 为空；AI 座位标注可见 |
| R12 | 短信通道受阻 | 集成：mock 短信通道 + 通道超时/失败注入；测试号白名单与备服务商切换 | 重试/降级链路可用，不阻塞登录续座；失败落日志可告警 |
| R13 | 表达力上限未被验证 | v1 不启用 `window`/`arbitrate`；M0 末纸面推演"麻将最小对局"（不实现） | 输出结构缺口清单（标明是否需新原语）；v2 立项前纸面推演确认后按 engine 次版本加回 |
| 死锁 / 无法终局 | L3 随机对局 200 局（每晚） | 100% 终局，非法动作率 0 |
| 断线恢复出错 | 故障注入：随机断网 5s、随机杀房间进程、**断线超过 200 条缓冲（触发 `snapshotReset`）** | 恢复后 `stateHash` 与断线前一致；`snapshotReset` 分支必须有一条用例断言"客户端丢弃本地状态并接受新快照" |
| 结算账不平 | 集成测试跑"结算 → 落库 → 对账"，并模拟写库失败 | 幂等键不重复；对账报表差异 0 |
| 发布链被绕过 | 每月演练：注入非法 RuleSet 走发布链 | 被 L2/L3 拦下；人为绕过时 canary 自动回滚 |
| 性能回退 | 基准固定基线，超阈值阻断合并 | `legal_actions` P99 退化 >10% 即阻断 |
| 客户端兼容断裂 | 旧版本客户端进房冒烟 | 旧版能看到玩法列表与 `minClientVersion` 提示 |
| 定级赛归属 | 集成：新账号连续 10 场 `human-ranked` 结算，其中第 3 场插入 1 局 `ai-practice`、第 6 场插入 1 局自定义房 | `placement_left` 只被 `human-ranked` 递减；第 10 场结算后 `rank_level` 等于这 10 场胜率对应档；定级期内 `rank_level` 不改、不进排行 |
| 同对手重复对局 K 衰减 | 集成：同一对对手在 24 小时内跑 5 场 `human-ranked`（固定种子、固定 `rating` 初值），逐场断言 `rating` 增量 | 第 1–2 场取当前 `K` 档（40/32）、第 3–4 场 `Δrating` 减半、第 5 场 `Δrating = 0`（不写 `rating`）；24 小时窗口外恢复全 `K` |

### 10.3 黄金用例怎么来

| 来源 | 数量目标 | 说明 |
| --- | --- | --- |
| 手工构造边界（M0–M1 用） | 40 条 | 逢人配组顺子、四王炸对 8 炸、双下升级、抗贡、过 A 失败回退、进贡/还贡；**S10 与 §10.2 在 M0–M1 的用例来源就是这一批**，此时还没有线上对局 |
| 真实对局录制（内测后累积） | 20 条完整局 | 内测开始后从线上异常局与人工导出的回放包累积，用于 L4 回归 |
| 规则变异 | 12 条 | 同一局在四种争议配置下各跑一遍，验证配置真的生效（S8） |
| 回归种子库 | 100 条 | 每条只覆盖一个原语或一条规则，失败时能直接指到原语 |

规则改动、原语改动、解释器改动的合并门槛统一为：**黄金用例 + 回归种子库全绿，双端一致性零差异**。

**用例编号与目录约定**：用例 id 为 `case-<rulesetId>-<NN>`；每条用例目录固定为 `crates/engine/tests/golden/<caseId>/`，内含 `manifest.json`（规则与三版本号、seed）、`actions.jsonl`（与回放包同构，§7.7）、`expected_hashes.jsonl`（逐步 `stateHash`/`viewHash` 期望值）。每日提交跑的快速集为固定名单文件 `golden/fast20.txt`（用例 id 列表，在本节注册，不按动态抽样）；全量集 = 本节上表全部用例 + 回归种子库。

### 10.4 双端一致性测试的具体做法

1. 用例来源：`action log` 集合（`manifest + actions.jsonl`），与回放包同格式。
2. 执行：`native` 直接跑引擎；`wasm` 用 `wasmtime` 加载同一份 `.wasm`。
3. 比对：逐步比对 `stateHash`、`legal_actions` 的排序结果（排序也必须一致，否则客户端提示顺序会不同）。
4. 门禁：任何一步不一致即失败，并输出第一处分歧的 `roomSeq` 与两边状态摘要。
5. 频率：每次提交跑固定名单 `golden/fast20.txt`（§10.3）；每晚跑全量（含所有黄金用例）。
6. 段位与评分的两条用例——**定级赛 10 场归属**、**同对手重复对局 `K` 衰减**（§10.2）——随每晚全量集跑，门禁同第 4 条（任何一步不一致即失败）；它们比对的是结算写入的 `rating` / `rank_level` 终值，不参与逐步 `stateHash` 比对。

### 10.5 性能基准

| 基准 | 指标 | 基线（M0 测得后锁定） | 门禁 |
| --- | --- | --- | --- |
| `legal_actions` | P99 | 初见 <0.3ms，待实测 | 退化 >10% 阻断 |
| `apply` | P99 | 初见 <0.5ms，待实测 | 退化 >10% 阻断 |
| 1000 房间空转 | 常驻内存、CPU | 待实测 | **改为"按 A4 峰值 × 3 倍余量"的房间数空转**（1000 只是占位数字，不作为容量承诺）；内存 24 小时增长 >5% 视为泄漏 |
| wasm 包体 | gzip 后的 wasm 单文件 | **阻断门禁 2MB**；目标 1.5MB | 1.5～2MB 之间不阻断，但发布记录必须写明体积变化与原因；>2MB 阻断合并 |

基线数字在 M0 实测后写回本节，不预先编造。

### 10.6 覆盖率与门槛

- `crates/engine`：分支覆盖 ≥90%，**统计目标固定为 native**（wasm 覆盖率口径不同，只作参考不作门禁），原语每条至少 3 个用例（正常、边界、非法）。
- 解释器：每个调度分支都要有 2 个用例——单人轮转、跳过已出完座位（v1 走两条）。`window`/`arbitrate` 不进 v1 的 Primitive ADT、`lifecycle` 枚举与协议说明，覆盖率分母不含这些未启用分支；v2 纸面推演确认需要后按 engine 次版本加回并补测（§5.14、R13）。
- 校验器：四级各有一个"故意造错"的用例，确保闸门真的会拦。
- 前端：不做覆盖率硬指标，改为四端冒烟清单（§10.7）。

### 10.7 四端冒烟清单（每次发布人工跑一遍）

| 检查项 | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| 首次加载 <3s（4G/家用宽带） | ✓ | ✓ | ✓ | ✓ |
| 建桌 + 4 人进房 | ✓ | ✓ | ✓ | ✓ |
| 拖拽出牌与取消 | ✓ | ✓ | ✓ | ✓ |
| 断网 10s 后恢复 | ✓ | ✓ | ✓ | ✓ |
| 结算弹窗与再来一局 | ✓ | ✓ | ✓ | ✓ |
| 后台音频恢复（切前台后有声） | ✓ | ✓ | ✓ | ✓ |
| 窗口缩放与最小尺寸 | ✓ | ✓ | ✓ | ✓ |

移动两壳的冒烟清单（含安全区/刘海适配、WebView 版本碎片）随移动壳一起押后，解押时补进本节。



## 11. 风险与未决项

### 11.1 风险登记册

概率与影响都用「高 / 中 / 低」三档，不做伪精确。

| # | 风险 | 概率 | 影响 | 触发信号（什么时候知道它成真了） | 缓解手段 | 验证点 |
| --- | --- | --- | --- | --- | --- | --- |
| R1 | **规则判定写错 + DSL 表达力不够**（R1 = 规则判定写错 + DSL 表达力不够，两条风险共用一个编号）：①牌型、进贡、升级档位等规则本体实现与 §5.13 规格不符；②麻将的鸣牌仲裁、掼蛋的进贡链这类结构写不出来 | 高 | 高 | ①黄金用例出现合法被拒 / 非法被接受，或残局结果与规格不符；②M0 末纸面推演斗地主时，"必须加新原语才能表达"出现超过 3 次 | ①黄金用例覆盖牌型全表与进贡/升级残局，并与 §5.13 逐点对照；②降级为"族内模板 + 参数"，工期回落 6～8 周，麻将族推迟 | ①§10.2 R1 的判定用例全绿；②**M0 末（第 6 周）gate**：斗地主能否只靠 RuleSet（纸面）写出完整规则；M3 接入时再实测 |
| R2 | **34～38 周基线不成立**：2 人同时做引擎、DSL、平台、四端、后台 | 高 | 高 | 第 16 周 gate 没达成（Web + 本地 bot 打不完一整场）；或第 20 周时 M2 剩余工作量 > 计划一半 | 三档缩范围：先砍斗地主（只做掼蛋）→ 再砍桌面壳（只留 Web）→ 最后砍回放式观战 | 16 周 gate 硬验收（§2.4），未过即触发缩范围 |
| R3 | **解释器性能不达标**：单动作超预算，单实例房间密度掉到实测上限的 60% 以下 | 中 | 中 | M0 基准测试 P99 超预算；M3 压测房间密度 < 上限 60% | RuleSet 预编译为字节码；原语内枚举加缓存；必要时单实例改多进程 | M0 基准：`legal_actions` / `apply` P99（§10.5）；M3 压测给出单实例上限 |
| R4 | **规则语法返工**：主版本不兼容，历史 RuleSet 全部失效 | 中 | 高 | 表达第二款玩法（斗地主）时需要改已有原语语义 | 语法冻结前用"掼蛋 + 斗地主（纸面）+ 麻将（纸面）"做表达力矩阵；发布后只做加法 | 语法冻结评审（M0 末），都走纸面，不做可运行对局 |
| R5 | **商店渠道缺位**：暂不申报版号，移动端只能侧载/TestFlight，拿不到应用商店的自然流量与内购通道 | 高 | 中 | 上线 3 个月后新增用户主要来自 Web 分享、留存依赖熟人传播 | 首发 Web/H5 + 桌面自主分发；用内测数据验证留存与付费意愿，再决定是否为商店渠道把版号排回关键路径 | 每季度看一次渠道机会成本（`A9` 修订版） |
| R6 | **2 人上下文过载**：引擎、DSL、平台、四端、后台五线并行 | 高 | 中 | 同一周内两人都在切换三个以上模块 | 固定 owner（一人引擎 + DSL，一人平台 + 客户端）；crate 边界即职责边界 | 每两周看一次"近两周改动的 crate 分布" |
| R7 | **状态漂移**：双端或恢复后哈希不一致 | 中 | 高 | `statehash_mismatch_total` > 0 | 确定性纪律 + 双端一致性测试 + 重放恢复 | §10.4 每次提交 |
| R8 | **bot 体验差**：L0/L1 太弱，人机房没人玩 | 中 | 中 | 内测反馈"bot 太傻"占比 >30% | 先做配合规则（不压搭档、保炸弹），再谈搜索 | 内测第 2 周做一次小样本问卷 |
| R9 | **四端打包与签名卡住**：macOS 公证、Linux AppImage 依赖 | 中 | 低 | M2 结束时三端桌面产物仍装不上 | CI 在 M2 第一周就接签名与打包；移动端打包随移动壳押后 | 四端冒烟清单（§10.7） |
| R10 | **素材授权问题**：素材不允许商用，或署名/许可文本要求未满足 | 低 | 中 | 审核或法务提出异议 | 优先 CC0；允许商用但有署名要求的（如 OFL 字体）可收，但必须随分发保留许可文本；清单 `../../assets/license.md`（即仓库内 docs/assets/license.md） | M1 前全表可查 |
| R11 | **人机混合房引发不满**：玩家觉得"bot 假装真人" | 中 | 中 | 投诉集中在"对手是机器人" | UI 明确标注 AI 座位；提供"仅真人房"开关 | 内测问卷单项 |
| R12 | **短信通道受阻**：模板报备失败或限额不够 | 低 | 中 | M2 账号链路阻塞 | 测试号白名单兜底；备一家服务商 | M2 前完成报备 |
| R13 | **表达力上限未被验证**：v1 只做出牌族的两款玩法，`window` / 仲裁原语从未被真实玩法压过 | 中 | 高 | v2 接麻将时才发现结构性缺口，需要改原语语义（连带 R4） | v1 不把 `window.open` / `window.arbitrate` 纳入类型与枚举；v2 启动前先做一次"麻将最小对局"纸面推演，确认需要后按 engine 次版本加回并补测 | v2 立项时用纸面规则逐条对照原语清单 |

### 11.2 未决项（需要人拍板，设计者不代替决定）

| # | 未决项 | 为什么要人定 | 默认选择 | 状态 |
| --- | --- | --- | --- | --- |
| U1 | 是否接受"掼蛋先内测、DSL 后完成"的分期节奏 | 决定现金流与风险暴露顺序 | **已定（2026-11-23）**：接受分期，并把 gate 提前到第 16 周——届时必须能用 Web 端 + 本地 bot 打完整场（不含联机），联机内测 20～24 周 | settled |
| U2 | 麻将族是否进 v1 | 决定 DSL 表达力边界与 R1 的暴露程度 | **已定（2026-11-23）**：v1 完全不碰麻将；`window` / 仲裁原语不纳入 v1 类型与枚举，风险记为 R13，v2 立项前做纸面推演 | settled |
| U3 | 后台是否需要多租户（多团队/多产品共用） | 影响权限模型与数据隔离成本 | **已定（2026-11-23）**：不做多租户，单租户、多玩法 | settled |
| U4 | AI 强度是否对外分档展示 | 影响玩家预期与匹配策略 | **已定（2026-11-23）**：分三档（简单/普通/困难）+ UI 标注"AI 陪练"，**人机房不计段位与正式战绩**（见 §6.6 的 `matchType` 分流） | settled |
| U5 | 版号代理预算与是否委托代办 | 影响时间线与成本 | **已定（2026-11-23）**：暂不申报，首发只做 Web/H5 + 桌面自主分发；版号退出关键路径，商店端与移动壳押后（C1 的"版号进关键路径"由此修订） | settled |
| U6 | 是否计划海外发行 | 影响服务器位置、数据合规与支付渠道 | **已定（2026-11-23）**：只做国内；服务器放国内并走 ICP 备案，产品只做中文 | settled |
| U7 | v1 是否接真实支付 | 决定账务层与合规成本 | **已定（2026-11-23）**：留支付占位不收费（`order`/`payment_callback`/`refund` 三表先建），不接渠道 | settled |
| U8 | 观战留不留 | 影响房间层工作量与协议复杂度 | **已定（2026-11-23）**：只做回放式观战，不做实时旁观 | settled |
| U9 | 工期基线怎么定 | 决定能否对外报交付时间 | **已定（2026-11-23）**：拆到 M0–M4 五段并承诺 34～38 周，阶段延期必须重算总和 | settled |
| U10 | 掼蛋规则本体谁定 | 决定 M1 能否开工 | **已定（2026-11-23）**：由设计者按淮安主流打法起草《规则取值表》（§5.10），用户逐条确认后冻结 | settled（七行待确认） |
| **U11** | 商业化起点与版号提报的触发条件（具体日期、责任人、提报批次） | 决定"何时能收款"这条对外承诺；设计者不知道你的资金安排 | 默认：**v1 上线后观察 8 周留存再决定**；提报日 D 起按 `A9` 的 6～12 个月倒排可收款日 | **open（待人定）** |
| **U12** | 无版号、免费内测（Web/H5 非商店渠道）是否必须接实名与防沉迷 | 这是法务口径，不是技术取舍；接错的代价是下架 | 默认：**内测期不接**，公网开放前由法务/代办给书面答复；答复"要接"则设为硬 gate（`D_compliance`），排在 M3 | **open（待法务/代办）** |

### 11.3 外部依赖与责任方

| 依赖 | 责任方 | 若延迟的影响 |
| --- | --- | --- |
| 域名与 ICP 备案 | 项目方 | **M0 起点后移**（测试环境无法公网访问） |
| 短信服务商报备 | 项目方（需企业资质） | M2 账号链路阻塞 |
| 开源素材授权 | 项目方 | M1 UI 阻塞 |
| macOS CI runner | 项目方（成本项） | macOS 产物只能手工构建 |
| Apple / Google 开发者账号 | 项目方 | v1 不阻塞；移动壳解押时才需要 |
| 版号 | 项目方 + 代办 | v1 不阻塞；决定上商店时才进关键路径 |


## 12. 决策记录

### 12.1 关键取舍

| # | 决策 | 理由 | 代价 | 依据 / 假设 |
| --- | --- | --- | --- | --- |
| D1 | 引擎用 Rust 单 crate，产出 wasm（客户端）与 native（服务端）双产物 | 规则只有一份实现，联机、本地 bot、回放、AI 四方复用；`stateHash` 双端可比 | 双产物一致性要长期守；wasm 包体与优化构建耗时 | C3、S10、`A11` |
| D2 | 服务端单栈 Rust，不引入 Java / Go / Node | 引擎同进程调用，零跨语言序列化；团队只维护一套构建与依赖 | 平台能力全部自写；无社区示例可抄 | C3、`A1`、§4.4 |
| D3 | 客户端单套 Web 渲染 + 桌面壳 Tauri 2 + 浏览器直跑；移动壳押后 | 1 套 UI 覆盖四端；引擎统一 wasm，省掉跨语言绑定层 | 移动端延后（U5）；桌面壳包体略大 | C4、`A12`、U5 |
| D4 | 平台层与房间层自建，不引入 Nakama / Colyseus / Skynet / Photon | 引擎必须同进程；规则后台通用框架给不了；2 人不宜多担一种运行时 | 平台能力需自写 4～6 周；房间层 3～4 周 | §4.3、§4.4、`A15` |
| D5 | 传输层自研 protobuf over WebSocket | 牌局消息量小；必须按座位裁剪视图，通用全量同步不安全 | 重连、心跳、增量语义要自己设计 | §3 约束 3、§4.1 |
| D6 | 引擎形态从"每玩法一个 crate"改为"原语库 + RuleSet 解释器" | 运营要零代码上新玩法，规则必须是数据 | 解释执行开销；规则语法一旦发布难改；相对单玩法基线约 +16 周（已并入 §2.4 的 34～38 周）。**生效判据**：v1 上线后 3 个月内存在第 2 款玩法的上线计划，且至少 1 名非研发能独立写出 RuleSet；判据不成立即回落"每玩法一个 crate" | C7、C8、`A14`、`A16` |
| D7 | 后台编辑器以积木为主，源码模式为辅，两者读写同一份 RuleSet | 非技术人员不碰语法，非法结构在 UI 层就造不出来 | 编辑器开发量 +2 周；**不承诺"两视图无损往返"**——源码视图只保证"能被解析回同一份 RuleSet 的字段级等价"，不保证注释、顺序、格式逐字还原（写源码→再读回积木时按字段重排），所以不需要第二套 TS 解析器常驻关键路径 | C9、`A17` |
| D8 | 房间创建即锁 RuleSet 版本，发布只影响新房间 | 否则同一局中途换规则，`stateHash` 不可能稳定 | 缺陷版本要等房间打完才彻底下线 | §5.5、§9.5 |
| D9 | 发布前跑四级校验（结构 / 引用 / 可玩性 200 局 / 黄金用例） | "配置即炸弹"的唯一有效闸门 | 发布流程变长（分钟级） | §5.4、S12 |
| D10 | 发布需 TOTP 二次确认 + 强制填理由 + 发布前后自动导出 `active` 版本；回滚免二次确认 | 2 人团队做不到编辑/发布分离，护栏只能做"单人也能防误操作" | 每次发布多一步操作；无法防止本人有意发布坏配置 | §8.1、`A17` |
| D11 | 结算以 `settlementId = matchId + roundNo` 幂等，先落库再广播 | 写库失败不能把故障转嫁给玩家（不判负、不扣分） | 房间会短暂停在 `settling` | §6.6 |
| D12 | 崩溃恢复用"快照 + 重放动作日志"，对不上直接判流局，不修补 | 确定性引擎让重放成本极低；修补会导致两端看到不同的牌 | 极端情况下玩家经历一次流局 | §6.5 |
| D13 | 后台独立子域名，默认不暴露公网，手机号 + TOTP 登录 | 规则发布链是最敏感的攻击面；不能复用一个玩家 token 体系 | 运维要多一层网络配置 | §8.2 |
| D14 | 只保留最近 3 个（engine+interpreter）组合的可运行镜像；超出窗口的旧回放降级为"数据级重放"（导出 actions + 版本号，需本地构建对应 tag） | 镜像仓库成本与回放可复现性之间取平衡 | 窗口内镜像可完整重放；超窗口局不保证可运行，仅能做数据级重放或读日志 | §9.3 |
| D15 | 合并门槛 = 黄金用例全绿 + 双端哈希零差异 + 性能退化 ≤10% | 判定错误与状态漂移是本项目最贵的两类缺陷 | 每次提交多 2 分钟 CI | §10.2、§10.5 |
| D16 | 卡面与音效优先 CC0，允许商用且可署名合规的素材 | 商业化上架要过审核，授权不清会直接卡住 | 视觉上限受限，后期可能整体换皮 | C6、`A3`、R10 |
| D17 | v1 只做回放式观战，不做实时旁观 | 实时旁观要新增只读座位、额外裁剪与流控，收益远低于成本 | 玩家不能"围观正在打的牌局" | U8（2026-11-23） |
| D18 | 首发只做 Web/H5 + 桌面自主分发，版号暂不申报，移动壳押后 | 现金流最早；牌类版号周期与不确定性太高，不该压住首发 | 商店自然流量与内购通道暂时拿不到；移动用户只能侧载/TestFlight | U5（2026-11-23）、`A9` 修订版 |
| D19 | 支付只留占位不收费（建 `order` / `payment_callback` / `refund` 三表与回调接口，不接渠道） | 上线时不收款就没有支付合规义务；但表结构一旦缺，后续接入要改库 | 早期无收入；占位表要随版本演进维护 | U7（2026-11-23） |
| D20 | 工期基线 = M0–M4 五段、合计 34～38 周，按阶段承诺 | 单一总数字掩盖了阶段间的依赖；分阶段才能逐段验收 | 阶段间要重算总和，任一段延期都要对外改口径 | U9（2026-11-23）、§2.4 |
| D21 | 掼蛋规则取值由设计者起草《规则取值表》，用户逐条确认后冻结 | 规则口径（同花顺位次、进贡触发、升级档、过 A 条件）逐条都是可争议的，模糊就写不出引擎 | 冻结前 M1 不能开工；确认过程占用用户时间 | U10（2026-11-23）、§5.10 |

### 12.2 需求变更留痕（本设计在同一天内被改过八次，基线因此变了两次，工期口径变了三次）

| 时点 | 变更 | 影响 |
| --- | --- | --- |
| 初版 | 内测起步、按上架标准、版号进关键路径；Rust 引擎；单套 Web + 三壳；手机号账号；开源素材 | 基线 18 周 |
| 第一次变更 | 新增"简单运营后台"需求；定位从单款游戏改为多玩法平台 | 交付物增加 `apps/admin`，M3 增加发布链 |
| 第二次变更 | 后台要求"零代码配置其他棋牌并上线" | 引擎从插件改为解释器，基线 18 → 32～36 周 |
| 第三次变更 | 编辑器定为"积木 + 源码"双视图 | 基线 32～36 → 34～38 周 |
| 第四次变更 | 版号暂不申报（U5），首发只做 Web/H5 + 桌面；移动壳押后；AI 分三档且人机房不计分（U4） | 商店端退出关键路径；移动壳从 M2 移出；结算增加 `matchType` 分流 |
| 第五次变更 | 只做国内，服务器国内部署（U6） | 不做多语言与多区；域名与 ICP 备案重新列为 M0 前置项 |
| 第六次变更 | 工期拆成 M0–M4 并对外承诺 34～38 周（U9）；只做回放式观战不做实时旁观（U8）；支付只留占位不收费（U7）；掼蛋规则取值改为《规则取值表》逐条确认（U10） | §2.4 增加五段周数表与 gate 口径；§6 观战相关工作量从 M2 移出；§5.8 增加三张支付占位表；§5.10 新增掼蛋规则取值表（七行待确认），M1 开工前必须先冻结 |
| 第七次变更 | round-03 评审裁决修订（91 条全部 PROPOSE FIX）：L2 静态校验与运行时守恒/容量断言拆分（`RS_ZONE_CONSERVATION` / `RS_ZONE_CAPACITY`）；`lifecycle` 固定四段、`turn` 为 `play` 子阶段；deck 必填 `jokers: 2`、总张数 108；场次终止 = 过 A 或安全局数上限（默认 12 局，非胜负判据）；动作记录保留 `seatSeq`/`roomSeq` 双序号、重放水位用 `roomSeq`；私有区按 `(seat, zoneId)` 实例化；4096 候选为加载期静态上限；草稿/发布两套 schema（版本号发布时注入）；CPU 超预算仅记 P2 告警、不作回滚；`TOO_LATE` 入错误码表；`RoomSnapshot` 增 `snapshotReset`；后台清单扩为九件；段位/战绩按 `matchType` 分流 | §5.1/§5.2/§5.4/§5.6/§5.7/§5.8/§5.9/§5.10/§5.11/§5.12/§5.13/§5.14/§5.15、§6.1/§6.2/§6.3/§6.5/§6.6、§7.1/§7.2/§7.5/§7.6/§7.7、§8.1/§8.3/§8.4/§8.6、§9.2/§9.4/§9.5、§10.1/§10.2/§10.3/§10.4/§10.6、§11.1、§12.1 |
| 第八次变更（规则取值确认） | 2026-11-23，用户逐条拍板 §5.10 七项待确认取值，随表冻结进 `guandan-classic` v3：其中 3 项与原取值不同而实改（同花顺位置改为 `above-bomb-5`——5 张炸之上、6 张炸之下；进贡配对改为对位进贡——末游→头游、三游→二游；抗贡改为贡方两家合计两张大王即可抗贡）；1 项按用户口径放宽（过 A 由"必须双下"放宽为 1+2 或 1+3 即算过 A，1+4 不算）；3 项维持原值（还贡、升级档、接风） | §5.1（`compare.flushRank` 新增 `above-bomb-5` 取值、`tribute.pairs[]` 对位配对、`resist` 口径与示例 YAML）、§5.10（7 行状态由待确认改为 `定`、全表冻结）、§5.13（进贡配对、抗贡、比较阶梯、升级跨 A 与过 A） |

**第七次变更的收尾拍板（同日）**：round-03 修订者留下的 8 条待拍板项由用户逐条拍板——4 项维持现状（①`legalFrom` 检查与 `RS_PHASE_UNIMPLEMENTED` 留在 L2；②安全局数上限达顶时比 `levels` 档位、高者胜、同档平局；③`end_reason` 保留 `aborted`；④台账收敛记录里 round-03 的备注保留改后版本）；4 项改为：段位算法改为「定级赛按胜率分档 + 后续 Elo 式浮动」（§5.8、§5.13、§5.15、§8.7、§10.2、§10.4）；本地存档按模式分流（§2.1、§2.4、§5.9 末行）；原单列的「规则判定写错」并回 R1、不新增编号（§10.2、§11.1）；`publish` 语义写死为「只推进不新建」（§5.6）。

**留痕的意义**：评审时若发现"为什么一个棋牌项目要 34 周"，答案在 12.2 而不是在某一节的数字里；若未来要缩范围，也按 12.2 的变更顺序倒着砍。


