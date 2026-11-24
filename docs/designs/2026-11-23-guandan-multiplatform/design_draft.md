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
| 玩法范围 | v1 只做掼蛋；斗地主、麻将留 v2，原语保留但不验证 | U2 |
| 租户模型 | 单租户、多玩法，不做多租户隔离 | U3 |
| AI 口径 | 三档（简单/普通/困难）+ UI 标注"AI 陪练"；人机房不计段位与正式战绩 | U4 |
| 渠道与移动壳 | 暂不申报版号，首发只做 Web/H5 + 桌面自主分发，商店端与移动壳押后 | U5 |
| 变现 | v1 **留支付占位不收费**：订单表与支付回调接口先建，不接真实渠道 | U7 |
| 观战 | **只做回放式观战**，不做实时旁观 | U8 |
| 部署区域 | 国内单区；域名与 ICP 备案为 M0 前置项 | U6 |
| 工期基线 | 承诺 40～44 周，按 M0–M4 五段拆分（见 §2.4） | U9 |
| 规则取值 | 由设计者按淮安主流打法起草《规则取值表》，用户逐条确认后冻结 | U10 |

### 1.3 目标

| # | 目标 | 判定依据 |
| --- | --- | --- |
| G1 | v1 四个产物（Web 静态站 + Windows/macOS/Linux 桌面壳）都能完整打完一场掼蛋 | S2 |
| G2 | 4 人联机可用，含断线重连与超时托管 | S3、S4、S9 |
| G3 | 完全离线也能与 bot 打完一场 | S2 的离线分支 |
| G4 | 新增掼蛋变体只写玩法配置，不动框架核心 | S5 |
| G5 | 内测版的架构等于商店版：支付、实名、防沉迷只改服务端与壳层 | S7 |
| G6 | 平台化：后台零代码配出掼蛋变体配置并上线，全程不发版 | S12 |

### 1.4 成功判据

本节自带全部判据的判定语句，不依赖外部文档；编号与概念简报一致，S1 已按 Rust 单栈改写。

| # | 判据 |
| --- | --- |
| S1 | 同一份 `crates/engine` 产出的 `wasm32-unknown-unknown` 与 native 两个产物，跑同一批 action log，`stateHash` 逐字节一致；CI 100% 通过。**唯一例外**：注入 panic 的专门用例只要求"native 捕获并返回 `RS_INTERNAL_PANIC`、wasm 按 §8.6 的 abort 行为记录"，不要求两端同哈希——该用例标记 `panic-injection` 并从 S1 的分母里剔除，其余用例任何一条不一致即 S1 不成立 |
| S2 | v1 四个产物（msi / dmg / AppImage / 静态站）在断网环境下完成一局：入座建桌（场级前置动作）→ 发牌 → 出牌 → 结算，四端各至少 1 次人工验收记录。本判据验收粒度为一局；同场"再来一局"的连续流程见 §2.4 第 16 周 gate |
| S3 | 断网 5 秒后重连，客户端状态与服务端一致（`seatSeq` 水位与服务端自证的 `viewHash` 校验通过），可继续出牌 |
| S4 | 4 人真实联机连打 30 局，0 次错误判定（合法动作被拒 / 非法动作被接受）、0 次状态漂移；玩家误点被引擎拒绝不计入本判据分母，本判据与 §7.2 的非法率告警口径分开 |
| S5 | 新增掼蛋变体配置时，`crates/table-runtime` 与 `apps/server` 的改动不超过各自总行数的 10%，框架层单测全绿。**注**：S5 是"代码增量"的验收上限，G6／S12 是"零代码配规则"的验收，两者针对不同交付物，不互相替代 |
| S6 | 由 A4 推出的峰值（并发房间 × 3 倍余量）压测：动作回执 P99 < 100ms，进程内存 24 小时无持续增长 |
| S7 | 接支付、实名、防沉迷时只改 `apps/server` 与壳层，`crates/engine`、`crates/table-runtime` 零改动 |
| S8 | 四条争议（同花顺与炸弹的序 `flushRank`、逢人配能否组同花顺 `inStraightFlush`、过 A 条件 `level.pass`、抗贡条件 `tribute.resist`）可配置切换，双口径回归全绿 |
| S9 | 同一手机号账号在 Web 与桌面端登录，战绩与段位一致；联机局断线后在 §6.2 宽限期（默认 90 秒，取值 30–180 秒）内重连可恢复到该局；超宽限按 §6.3 托管与座位释放处置，不承诺离线续局 |
| S10 | `stateHash` 有唯一算法定义（§5.12：字段范围、序列化规范、哈希函数、是否含规则版本），两端实现由同一份 Rust 代码产出 |
| S11 | S8 的四条配置各有 ≥3 条黄金用例，任一配置变更触发重跑并给出差异报告 |
| S12 | 运营者不改代码、不发版，只在后台完成"选玩法族 → 配规则 → dry-run → 灰度 → 全量 → 回滚"，配出一款**族内**掼蛋变体配置，全程不重启服务、不更新客户端包。跨族（斗地主、麻将）不进 S12，见 R13 的纸面推演结论 |

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
| A14 | 通用规则 DSL 能表达目标玩法全部规则结构 | 表达不了（如麻将鸣牌优先级、掼蛋进贡链）只能降级"模板 + 参数"，工期回落但零代码上限降低 | **M0 末纸面推演**：只用 RuleSet（不实现代码）写出掼蛋变体与麻将最小对局，写不出来的结构逐条列出是否需新原语；实现级验证排在 M3 接入族内变体时 |
| A15 | 2 人能在 M0 内跨过 Rust 异步服务端门槛 | 若 M0 末房间并发与断线恢复原型跑不通，基线整体顺延 3～4 周或缩范围 | M0 交付"空转压测"与"断线恢复"两个原型并记录实测 |
| A16 | 解释执行开销可接受：单次动作解释 < 1ms，单核承载 ≥1500 房间（= §3.2 的 3 倍余量基线） | 需引入预编译缓存（+3～4 周），热更新复杂度上升 | M0 基准：在 `m0-minimal` 夹具（§2.4）上双产物各跑 10 万次 `legal_actions` 与 `apply`，记录 P99 |
| A17 | 编辑器以积木为主视图，源码模式只作研发高级视图，两者读写同一份 RuleSet | 若源码成为主入口，语法错误会进生产 | 已确认（C9）；M0 出交互草图与"非法结构不可构造"验证 |

### 1.6 术语

同一份文档里最容易打架的四个量词，先在此定死，后文不再换义：

| 术语 | 定义 | 对应标识 |
| --- | --- | --- |
| 场（match） | 一次组局，从入座到所有玩家离桌；一场含若干局；定级赛、段位、积分一律按场结算，不按局计 | `matchId` |
| 局（round） | 一局牌，从发牌到结算；掼蛋一局可能打过 A 后继续 | `roundNo` |
| 轮（turn） | 一局内的一次行动权交接，是 `play` phase 内的轮次子阶段，**不是** phase | 不进 ID，只用于"轮转"描述 |
| 端（client build） | v1 四个：Web 静态站、Windows、macOS、Linux；移动两壳与"六端"是 v2 判据 | — |
| 房间广播序（`roomSeq`） | 房间内全局递增的广播水位；快照内记 `snapSeq`，即快照覆盖的最大 `roomSeq`（同一 u64 序列） | `roomSeq` / `snapSeq` |
| 座位动作序（`seatSeq`） | 每座位单调递增的动作水位；快照内记 `seqWatermark[seat]` | `seatSeq` / `seqWatermark[]` |



## 2. 范围与非目标

### 2.1 本次设计覆盖的交付物

| 交付物 | 内容 | 阶段 |
| --- | --- | --- |
| `crates/engine` | 原语库（牌/区域/牌型/轮转/行动仲裁/结算/计分/确定性 RNG/视图裁剪）+ RuleSet 解释器 + 静态校验器；原语由 Rust 实现，RuleSet 只能组合原语 | M0–M1 |
| `crates/table-runtime` | 对局生命周期：入座、发牌、行动窗口、计时、托管、断线恢复、回放录制 | M1–M2 |
| `crates/runtime-spike` | M0 一次性原型 crate：断线恢复最小路径（快照载入、增量重放、`stateHash` 校验）；不进 v1 正式交付物，转正时保留模块与测试迁入 `crates/table-runtime`（转正判据见 §2.4 M0 清单） | M0 |
| `crates/protocol` | proto 定义与 Rust 类型生成，服务端与 wasm 客户端共用 | M0 |
| `crates/ai` | 与玩法无关的搜索/评估原语；掼蛋三档策略：`easy`→AI-L0、`normal`→AI-L1、`hard`→AI-L2（AI-L1 加搜索，映射见 §5.11） | M1 |
| `apps/server` | 账号、短信验证、大厅、匹配、房间托管、战绩、**回放式观战**、回放查询、积分/段位/排行榜；玩法注册表与规则配置版本；支付、实名、防沉迷只留适配接口（U7：订单表与回调先建，不接真实渠道） | M2–M3 |
| `apps/admin` | 运营后台（Vue 3 + 同一套组件库），交付范围以本行清单为唯一定义（§2.2 引用此处，不另写件数）：①玩法注册；②规则配置（schema 生成表单 + 积木/源码双视图）；③dry-run 校验；④灰度/全量发布；⑤回滚；⑥审计日志；⑦房间与账号巡检；⑧踢人/封号、人工改分、结算死信人工补写；⑨手动发码、导出回放、后台账号管理 | M3 |
| `apps/client` | 单套 Vue 3 + PixiJS 渲染，含大厅、牌桌、结算、设置、回放播放器；含本地存档层（**仅纯本地 bot 局**用；载体、内容、写入时机、恢复校验与上限见 §5.9 末行；联机局不写本地存档） | M1–M3 |
| `apps/shell-desktop` / `apps/shell-mobile` | Tauri 2（Win/Linux/macOS）先在 M2 做；Capacitor 6（Android/iOS）**押后**（U5：版号暂缓，商店端不进关键路径） | M2 / 押后 |
| `tools/replay-viewer` | 离线重放器：读导出包复现整局，是故障排查与 S1 比对的实际执行者 | M2 |
| `tools/rule-authoring` | 规则编辑器核心（积木树 ↔ 源码文本双向转换、结构合法性校验），被 `apps/admin` 复用而非另写一套 | M3 |
| 玩法 | 掼蛋完整；**斗地主、麻将不进 v1**（v2 候选，纸面推演见 R13） | M1–M3 |
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
- **麻将**：v1 完全不碰（含最小对局）。`window`（多座位行动窗口）与 `arbitrate` 原语**不进** v1 的 Primitive ADT、`lifecycle` 相位枚举与协议说明；v2 立项前先做"麻将最小对局"纸面推演，确认需要后按 engine 次版本加回（§5.14，见 R13）。
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
| M1 | 第 7–16 周 | 掼蛋规则、AI-L0/AI-L1、客户端牌桌、**Web 端 + 本地 bot 可完整打完一场（第 16 周硬 gate，不含联机）** | 原生壳、账号服务、联机、观战 |
| M2 | 第 17–26 周 | 账号与短信、房间、匹配、断线恢复、桌面壳打包、回放重放器；**第 24 周联机内测**（仍在 M2 区间内） | 支付渠道、移动壳（押后）、社交 |
| M3 | 第 27–36 周 | 回放式观战、战绩与段位、告警、合规适配层占位、**运营后台与玩法配置发布链**、族内掼蛋变体接入 | 变现渠道、赛事 |
| M4 | 第 37–44 周 | 平台化收尾：族内掼蛋变体零代码上线演练、发布链压测、文档与交接 | 斗地主/麻将（v2）、移动壳（押后） |

五段周次首尾相接、不重叠：6 + 10 + 10 + 10 + 8 = 44 周是日历跨度上限；40 周是"每段都能压到下限并复用 M0 骨架"的乐观值。所以口径是 40～44 周，不是"44 减重叠"。任一阶段延期，要么从后续阶段同等工作量里扣，要么整体顺延并重报总工期，不允许只改一段而不重算总和。

> 工期备注：因保留 172 条全量测试与八层测试全量回归，工期由 34～38 周调整为 40～44 周（用户 2026-11-23 拍板）。M0（第 1–6 周）与 M1（第 7–16 周）周次一字不动；增量 6 周落在 M2（第 17–26 周，+2）、M3（第 27–36 周，+2）、M4（第 37–44 周，+2），M4 收尾必须落在第 44 周。

**gate 口径**：第 16 周的"完整打完一场"指 建桌 → 发牌 → 出牌 → 结算 → 再来一局 全流程跑通（验收粒度同 S2：一局连续跑通，"再来一局"为同场延续）。断线/崩溃的验收按模式拆成两条，各自挂到适用的阶段：

- **纯本地 bot 局**（wasm 引擎跑在客户端、无服务端判局）：**杀死客户端进程后重开可续局**——本条是第 16 周 gate 的组成部分（本地存档的载体、内容、写入时机、恢复校验与上限见 §5.9 末行）。
- **联机局**（含服务端 bot 房）：**断网 90 秒内可重连继续**——按 §6.2 的宽限期与快照重连口径（`snapshotReset`），客户端不写本地存档、不承诺离线续局；本条随 M2 第 24 周联机内测 gate 验收（第 16 周 gate 仍不含联机、不含账号）。

**合规 gate（U12）**：公网对外开放（不再限于内测白名单）之前，必须拿到"无版号免费内测是否需要实名与防沉迷"的书面答复；答复为"要接"则 `D_compliance`（实名 + 防沉迷接入完成）成为 M3 的硬 gate，未完成不得开放公网注册。M3 的"合规适配层占位"就是为这条留的位置。

**范围冻结规则**：内测开始后新增需求一律进 backlog，不插入当前里程碑；确需插入时，必须同步删掉同等工作量的既有项，并在裁决台账登记。
  依据：U9 已把基线定为 40～44 周 × 2 人，没有余量吸收插入式需求（`A1`、`A15`）。

**M0 落地清单（第 1–6 周，照此开工）**：

1. **Workspace 与 crate 骨架**：Cargo workspace 成员为 `crates/engine`、`crates/protocol`、`crates/runtime-spike`；proto 文件固定路径 `crates/protocol/proto/gf/v1.proto`，package `gf.protocol.v1`，服务端与 wasm 客户端共用同一份生成代码（§5.9）。
2. **引擎最小对外接口**（`crates/engine` 公共 trait，双产物同一份）：`legal_actions(&State) -> Result<Vec<Action>, EngineError>`、`apply(State, Action) -> Result<State, EngineError>`、`view(&State, Seat) -> View`、`validate(&RuleSetBody, levels: &[Level]) -> Validation`；接口签名在 M0 锁定，M1 只补原语实现，不改契约形状。**`legal_actions` 返回 `Result`**：`IllegalAction` 与 `QuotaExceeded` 均由它出，与 §5.4 的 `EngineError` 出口清单一致；本处原写裸 `Vec<Action>` 属与 §5.4 的内部矛盾，2026-11-24 按 M0 实现期口径对齐（§12.2 第十一次变更第 ① 条）。`Validation = { ranLevels: HashSet<Level>, findings: Vec<Finding> }`；`EngineError` 变体清单见 §5.4。
3. **两个原型落点**：空转压测原型落在 `crates/engine` 的 bench；断线恢复原型落在 `crates/runtime-spike`。**转正判据**：原型中进入 §10.1 契约层双产物比对的代码与测试必须保留并迁入 `crates/table-runtime`；压测脚本与一次性夹具可丢弃，不迁入。
4. **最小夹具 `m0-minimal`**：RuleSet 固定路径 `crates/engine/tests/fixtures/m0-minimal/ruleset.yaml`，版本号 `m0-minimal@1.0.0`；只含发牌、单张出牌、score-only 结算三件事，是 M0 端到端原型、双产物基准与 §10.1 契约比对的共同输入。口径说明：本夹具**走真实 DSL 结算路径**——`settle: { algorithm: score-only }`（即 `Settle::ScoreOnly`，§5.1），只记录名次、不升级、不挂等级表；该变体是 v1 在役变体，不依赖任何等级表。
5. **M0 gate 表**：

| 验收项 | 判据 | 产出物路径 | 对应假设 | 未达标处置 |
| --- | --- | --- | --- | --- |
| Rust 服务端 ↔ wasm 端到端原型 | 同一 `m0-minimal` 夹具 RuleSet 两端各跑通一次发牌→出牌→结算，`stateHash` 逐字节一致 | `crates/engine`、`crates/protocol`、`crates/engine/tests/fixtures/m0-minimal/` | A1、A11 | 回退"引擎 Rust + 服务端 Node 跑 wasm"或整体降级 TS 引擎 |
| 掼蛋变体/麻将纸面推演 | 只用 RuleSet（不写代码）写出两款最小对局，结构缺口逐条列出 | M0 末纸面推演记录 | A14 | 表达不了的结构列为新原语候选，不进 M0 编码 |
| 空转压测 + 断线恢复两原型 | 两个原型实测跑通；断线重放后 `stateHash` 与断线前一致 | `crates/engine` bench、`crates/runtime-spike` | A15 | 基线顺延 3～4 周或按 R2 缩范围 |
| 解释执行基准 | 双产物各在 `m0-minimal` 夹具上跑 10 万次 `legal_actions`/`apply` 并记录 P99；目标单次动作解释 <1ms、单核 ≥1500 房间 | §10.5 基准表 | A16 | 引入预编译缓存（+3～4 周），重估工期 |
| 非法结构不可构造 | 反序列化层验证非法 RuleSet 无法构造并通过 L1；出交互草图 | `crates/engine` 校验器、草图 | A17 | 源码升为主入口则重新评估编辑器形态 |

未达标项按上表"未达标处置"回退，并在 §11.1 对应风险行登记；M0 末（第 6 周）逐项勾选，任一硬 gate 未过不得进入 M1。


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
5. **人力**：2 人，40～44 周（U9）；后台与运维也由这 2 人承担，不额外配人。后台的**使用者是产品负责人本人**（C7 的"运营者"在 2 人团队里就是他自己），因此后台不需要多角色协作与审批链，但需要单人也能防误操作（见 §8.1）。
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
| 40～44 周内可落地 | ○（平台自写 4～6 周，已在基线内） | △（集成 + 学 Go，且规则后台仍要写，净增 3～5 周） | △（wasm 调试链变长，净增 2～3 周） | × | × |
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
| 匹配 | Nakama matchmaker 的 required / desired / optional 三段参数 | 同样三段：段位区间（required）、延迟区（desired）、偏好标签（optional）。**内测首日没有段位可填，前 10 场走定级赛**（required 放宽为"无段位匹配无段位"），定级结束后才进段位池；不足 4 人用 bot 补位，补位房记为 `mixed`，不计段位与正式战绩（§6.6） | 做 |
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
| 平台层与房间层都自建，不引入通用框架 | 引擎必须同进程；规则后台通用框架不提供；2 人不宜多担一种运行时 | 平台层自写 4～6 周 + 房间层 3～4 周（合计已计入 40～44 周基线）；无社区示例 | S10、C3、C4、`A15` |
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
| `seats` | `{count, teams}` | ✓ | ✓ | `count` 为座位数，取值 2–8；`teams: [[SeatId]]`，每个座位恰好属于一个队，不重叠、不漏座；覆盖不全或重叠在 L2 报 `RS_TEAM_COVERAGE_INVALID`（§5.7）。无阵营玩法（队伍数为空）属 v2 预留，v1 不开放。匹配、补偿、脱敏别名一律按 `count` 参数化，不得写死 4 |
| `deck` | `{copies, ranks, suits, jokers, exclude[]}` | ✓ | ✓ | `jokers` **必填**，固定 `2`（= 1 大王 + 1 小王）；`ranks: [RankKey]`（`naturalOrder` 子集，字面值 `2..A`）；`suits: [Suit]`（闭集 `S/H/D/C`）；`exclude: [RankKey]`，按点数排除、不含王，缺省空数组（不排除）。总张数 = `(ranks − exclude) × suits × copies + jokers × copies`；掼蛋基线 = 13×4×2 + 2×2 = **108**，与 `zones.hand` 容量 27×4 对齐；其它玩法的总张数必须 ≤ 各 `hand` 区容量之和（`capacity` 是上限，不是当前牌数） |
| `zones` | `[{id, scope, capacity}]` | ✓ | ✓ | `capacity` 必填且是**上限**（`-1` 表无上限），不是当前牌数。`scope` 只有 `private`（座位私有）与 `shared`（公共）；private 区按 `(seat, zoneId)` 实例化。静态校验、运行时断言与错误码见 §5.4 L2、§5.7 |
| `levelTable` | `{id, initial, ranks[], upgradeBasis, upgrade[], rankScores[], upgradeByDiff[]}` | ○（`Settle::LevelUp` 引用时 L2 必填） | ○ | 级牌表本体，全 RuleSet 内唯一引用（L2 校验：`level.upgrade`/`settle.levelTableRef` 只能指向本表，不允许出现第二张级牌表，§5.4 L2）。`levels` 是房间状态字段（**每队一个**，见 §5.13），不是常量；与 `levels` **同按队伍序号索引**的另一个房间状态字段是 `aFailStreak`（`[u8; teams]`，每队一个 u8，取值 **0–3**，记录该队"在 A 上连续未过 A"的局数，某局结算时若该值达到 3，则本队 `levels` 退回 `levelTable.initial`（起点 2）并把该值清零；过 A 即该场结束、计数无意义，见 §5.13）；`@level` 按当前行动方的队求值；**升级判定有两个 basis**，由 `upgradeBasis: rank-combo \| score-diff` 选（默认 `rank-combo`，即 §5.10 冻结口径）。`rank-combo` 下 `upgrade[]` 元素为 `{teamRanks: [u8; teamSize], step: u8}`，`teamRanks` 是**该队的规模个名次**——长度必须等于 `seats.teams[i]` 的队员数（四人两队即 2、六人两队三人即 3），元素取 `1..=seats.count`、互不相同且按升序规范书写（`[2,1]` 非规范、视同与 `[1,2]` 重复），与 §5.2 `level.pass.combos` 共用同一套名次编码；掼蛋基线的 `[1,2]`/`[1,3]`/`[1,4]` 取值逐字未动，只是编码不再写死长度 2；`step` 取值 **0–3**，`0` 表示"该名次组合不升档"（允许作者把全部组合枚举满），同一 `teamRanks` 不得重复、长度不符/元素越界/未升序即 L1 `RS_SCHEMA_INVALID`。`score-diff` 下先按名次给分、再按两队**分值差**升级（供"按积分总和之差升级"这类玩法，如六人两队三人）：`rankScores: [u16]` 长度必须等于 `seats.count`（下标 `i` 是第 `i+1` 名的分值），`upgradeByDiff: [{minDiff: u16, step: u8}]` 按 `minDiff` **降序**排列、必须含一条 `minDiff: 0` 兜底，`diff` = 胜方队各名次分值之和 − 负方队之和，命中首个 `diff ≥ minDiff` 的条目取 `step`（0–3，`0` 同义不升档），全不命中取 0；该 basis 要求 `flow.score.dimension = team` 且 `upgrade[]` 必须缺席，两个 basis 的键集**互斥**、同时出现即 L1 `RS_SCHEMA_INVALID`。`guandan-classic` v3 取 `rank-combo`，`rankScores`/`upgradeByDiff` 不出现（§5.10 升级档取值与状态逐字未动，2026-11-24 第十二次变更）。 |
| `level` | `{pass: {combos[], streakLimit, resetOnFail}, current: {scope}}` | ○（取默认值即为掼蛋基线） | ○ | 过 A 与级牌取值的**可配置入口**，S8 的"过 A 条件"由此切换：`pass.combos: [[u8; teamSize]]`（非空，每个元素是该队规模个名次、取值 `1..=seats.count`、互不相同且升序，长度 ≠ `teamSize` 或元素越界/重复即 L1 `RS_SCHEMA_INVALID`；默认值只在 `teamSize = 2` 时有定义 `[[1,2],[1,3]]`，`teamSize ≠ 2` 时必须显式声明——与 `flow.deal.perSeat` 同一类「默认值只对某一种规模有意义」的处理）、`pass.streakLimit: u8`（1–3，默认 3，即 `aFailStreak` 阈值；上界 3 与 `aFailStreak` 的冻结值域 0–3 同源，要突破 3 须另走 EGM 裁决）、`pass.resetOnFail: bool`（默认 `true`，达阈值退回 `levelTable.initial` 并清零）；`current.scope: by-team\|global`（默认 `by-team`）。三键语义与 §5.2 `level.pass` 同名参数一致，顶层声明即覆盖原语默认值；`settle.algorithm=score-only` 的 RuleSet 可整段省略。 |
| `naturalOrder` | `[rankKey]` | ✓ | ✓ | 自然点数有序表（不含级牌位），允许 `little-joker`/`big-joker`；`levelInsert` 给出级牌插入位，引擎载入时合成最终牌力序，合成算法见 §5.13 级牌行 |
| `levelInsert` | 枚举 | ✓ | ✓ | `@level` 的插入位：`above-A`（插在 A 之后、小王之前，掼蛋基线）或 `below-2`；最终序由 `naturalOrder` + `levelInsert` 在载入期唯一合成 |
| `wildcard` | `{source, substitute[], inStraightFlush}` | ○ | ○ | 掼蛋的逢人配；`source` 为结构化 `{suit: Suit, rank: RankKey 或占位符 "@level"}`：`suit` 取 `S/H/D/C` 闭集，`rank` 取具体 RankKey 或受控占位符 `@level`，L1 按枚举与占位符校验、不接受自由字符串；§5.10 冻结行里的旧写法 `heart-@level` 即 `{suit: H, rank: "@level"}`，两者等价、以后者为准；`substitute` 用枚举列出可替代范围；`inStraightFlush` 决定能否参与同花顺（C2 争议项之一） |
| `patterns` | `[{id, primitiveId, params}]` | ✓ | ✓ | 每个牌型必须自带 `id`，供 `compare.ladder` 与 `actions.legalFrom` 引用；引用不存在即 `RS_REF_MISSING`。**`params` 不是自由对象**：它是按 `primitiveId` 分型的固定参数结构体，字段名/类型/取值/默认值见 §5.2 参数表；未知键、缺失必填键一律在 L1 拒绝（`RS_SCHEMA_INVALID`） |
| `compare` | `{ladder[], bombOrder, flushRank}` | ✓ | ✓ | `ladder` 只列**不参与 `flushRank` 插入**的牌型（元素必须是 `patterns[].id`）；`straight-flush-*` 不得出现在 `ladder` 中（L1 报 `RS_SCHEMA_INVALID`），其位次唯一由 `flushRank` 在载入期合成，做法与级牌 `naturalOrder + levelInsert` 同一套。`bombOrder` 决定炸弹之间谁大，两个取值都是**两级比较**：`count-then-rank` = 先比张数（多者大），张数相同再比点数（大者大）；`rank-then-count` = 先比点数，点数相同再比张数。**点数一律按载入期合成的牌力序（`naturalOrder` + `levelInsert`）比较，不是牌面自然序**：故某队级牌（如 2 当级）在本局处于 A 之上，「4 张级牌」大于「4 张 A」、但小于「5 张任意牌」；王只出现在 `pattern.joker-bomb`，不参与 `pattern.bomb` 的点数比较。`guandan-classic` v3 取 `count-then-rank`（§5.10 炸弹序行冻结值，用户 2026-11-24 复述确认，§12.2 第十一次变更同批补充定稿）；**`flushRank` 是"同花顺与炸弹的序"这个争议项的唯一开关**：取 `below-bomb-5`（插在 5 张炸之前）、`above-bomb-5`（插在 5 张炸之后、6 张炸之前）、`above-bomb-6`（插在 6 张炸之后、7 张炸之前）或 `above-bomb-8`（高于 8 张炸），四值各自在载入期合成不同阶梯；**`guandan-classic` v3 取 `above-bomb-5`**（即 4 张炸 < 5 张炸 < 同花顺 < 6 张 < 7 张 < 8 张 < 四王炸），其余三值是留给其他 RuleSet 的可选值；**`flushRank` 的插入位由「锚点」定位，锚点按语义认、不按 id 名字认**：`flushRank` 值里的张数（`below-bomb-5`/`above-bomb-5` → 5，`above-bomb-6` → 6，`above-bomb-8` → 8）即锚点牌型的张数，载入期在 `ladder` 中查找 `primitiveId = pattern.bomb` 且 `params.count` 等于该张数的那一项作锚点，把 `straight-flush-*` 插到它之前（仅 `below-bomb-5`）或之后（其余三值）；**识别只看 `primitiveId` 与 `params`、不看 `id` 字符串**，作者可自由给牌型起名（依据：炸弹大小由张数决定，锚点随之按张数认，用户 2026-11-24 裁决）。三条判定与码：① RuleSet 未声明任何插入型牌型（无 `straight-flush-*`）时不做校验（惰性跳过，`m0-minimal` 即此情形）；② 声明了插入型牌型但 `ladder` 中找不到该张数的炸弹锚点 → 拒绝加载，报 **`RS_FLUSH_ANCHOR_MISSING`**（**不降级、不插到「最近位置」**，静默偏差比报错更难查）；③ 锚点存在但位次冲突（同一插入位被多个插入型牌型争夺，或插入后与 `ladder` 相邻项的相对序矛盾）→ 拒绝加载，报 **`RS_LADDER_CONFLICT`**；②与③**分开报码**，后台据此分辨「该补炸弹」还是「该改顺序」（§5.7）；不同取值分别得到不同 `stateHash` 作为生效判据（§10.3），**不需要改 ladder 本身** |
| `tribute` | `{enabled, trigger, pairs[], resist, pay, return, firstRound}` | ○（掼蛋必填） | ○ | `enabled=false` 时整段 `tribute` 不运行，且本段其余子键**可整段省略**（`enabled` 之外无必填项，只写 `enabled: false` 即合法；§12.2 第十一次变更第 ⑥ 条）。`trigger` 为闭集枚举，当前唯一取值 `last-round-rank`（按上局名次选贡方，参数结构见 §5.2 `tribute.determine`）；`pairs` 是"谁向谁"的**一对一映射数组**（`{payer, receiver, when}`），不是两个独立列表；`payer`/`receiver` 为**名次数值** `1..=seats.count`（`1` 即头游，与 §5.13 的名次空间一致），不再用 `first`/`second`/`third`/`fourth` 枚举——枚举把四人局写死，六人局的第五、六名无法表达；取值语义不变（单贡末游→头游、双下对位）；`when` 取值 `single-loss`（单贡：末游→头游）或 `double-loss`（双下：对位进贡，末游→头游、三游→二游），`trigger` 只定进贡依据来源，分支由 `when` 选定；还贡参数统一收归 `tribute.return` 三键 `ceiling`/`count`/`fallback`（§5.2）并由本段承载，`return` 在 `enabled=true` 时必填、缺任一键即 L1 `RS_SCHEMA_INVALID`，不再设 `returnPolicy` 重复键；`pay` 形如 `{count: u8（1–2，默认 1）, selection: max-rank-except-wildcard（当前唯一取值，默认该值）}`、`firstRound` 取 `skip`/`play`（默认 `skip`，场次首局是否跳过 `tribute`，§5.13）同样由本段承载；`resist` 为 `two-big-jokers` 或 `none`，其中 `two-big-jokers` 按贡方两家合计持两张大王判定，不要求在同一人手里，单贡时即贡方自己持两张；抗贡成立则进贡与还贡均不发生（判定口径见 §5.13 抗贡行） |
| `lifecycle` | `[phase]` | ✓ | ✓ | 固定四段 `[deal, tribute, play, settle]`；**有动作的 phase 必须至少一条 `legalFrom` 指向它，零动作 phase（`deal`/`tribute`/`settle`）豁免**（`deal` 由 `deal.shuffle` 自动执行、`tribute` 由 `tribute.*` 原语自动执行、`settle` 由结算纯函数自动执行，玩家均不提交动作）。`turn` 是 `play` 内的轮次子阶段，**不是 phase** |
| `actions` | `[{kind, primitive, legalFrom, params}]` | ✓ | ✓ | `kind` 是 v1 闭集枚举，仅 `play`/`pass`；`claim`/`declare` 及其对应原语 `action.claim`/`action.declare` 移出 v1、标 v2 预留，RuleSet 中出现即 L1 拒绝（`RS_SCHEMA_INVALID`）。`tribute`/`return` 不由玩家提交，属引擎自动执行（§5.13/§5.9）。`primitive` 指向实际执行的原语，`legalFrom` 只允许三种取值：`patterns`（受牌型约束）、某个 zone 的 id、某个 phase 名（如 `play`）；`turn` 不是合法取值。`params` 与 `patterns[].params` 同一套分型规则（§5.2）：无参原语必须写空对象 `{}`，有默认值的键可省略，必填键不得省。**`kind` 不能当幂等键**（一局内会重复出现），幂等键见 §6.4 的 `requestId` |
| `settle` | `{algorithm, levelTableRef?}` | ✓ | ✓ | v1 含两个 ADT 变体并列：`Settle::LevelUp{levelTableRef}`（`algorithm: level-up`，`levelTableRef` 必填且引用 `levelTable.id`）与 `Settle::ScoreOnly{}`（`algorithm: score-only`，无等级表引用，只记名次、不升级）；两种变体都不隐式引用外部表名。`score-only` 在 v1 有两个用途：`m0-minimal` 夹具的最小结算路径（§2.4）与 v2 的无阵营玩法；**v1 不开放无阵营玩法本身**（`seats.teams` 仍须覆盖全座，见本节 `seats` 行） |
| `matchCap` | `{rounds: u16}` | ○ | ○ | 安全局数上限（场次终止判据之一，§5.13）；`rounds` 取值 1–99，缺省 12；越界值在 L1 拒绝；仅防死循环，不是胜负判据；随 `ruleset.body` 持久化 |
| `view` | `{fields[], schemaVersion}` | ✓ | ✓ | `fields` 只允许五类写法：`seat.levels`、某个 zone 的 id（私有区按 zoneId 引用，不写 `seat.hand`）、`lastPlay`、`round.level`（本局局级牌的档位下标）、`round.attackingTeam`（本局打级队序号）；其它写法一律 `RS_REF_MISSING`。后两者由第二十二次变更新增，均为桌面公开信息（谁先出、谁打级人人可见），不构成信息泄露。`schemaVersion: u16` 标识这份字段白名单的版本：**字段集合或顺序变化必须 +1，只增不改序**；版本号由 RuleSet 作者分配，后台不统一发号（§5.16/§9.2）。它是房间快照 `viewSchemaVersion` 的来源，房间创建时锁定（§5.9/§5.12/§9.2）；客户端白名单缓存键为 `(rulesetId, viewSchemaVersion)` 二元组（§9.2）。传输出入一律按白名单（`project`）生成，避免"漏写一条就漏牌"
| `timing` | `{turnTimeSec, graceSec, offlineAllSec}` | ○ | ○ | 计时器可调参数：`turnTimeSec` 5–120 秒（默认 30）、`graceSec` 30–180 秒（默认 90）、`offlineAllSec` 60–600 秒（默认 300）；越界值在 L1 拒绝；缺省取括号内默认值 |
| `ai` | `{features[], hint}` | ○ | ○ | 给 AI 的特征与提示开关，不影响判定。`features` 取值集合见 §5.11；`hint: bool`（默认 `false`）是「是否允许开启提示」的**布尔开关**，不是难度档位——AI 难度三档（AI-L0/L1/L2）属实现层选型、不进 RuleSet（§5.11） |
| `flow` | `{deal, turn, score, rng, assert}` | ○ | ○ | 运行期流程参数归集（发牌、轮转、计分、RNG、断言五类原语的顶层默认值）；**整段缺省则五个子对象全取默认值**，各子对象逐键类型/取值/默认值见下表 |

**`flow` 子对象逐键定义**：

| 子对象 | 键 | 类型 | 取值 | 默认值 |
| --- | --- | --- | --- | --- |
| `flow.deal` | `perSeat` | u16 | 1–108 | `deck` 总张数 ÷ `seats.count` 的整除值；不整除时该默认值无定义，必须显式声明 `perSeat`，否则 L1 `RS_SCHEMA_INVALID`（v1 不定义余量发牌）。与 §5.2 `deal.shuffle.perSeat` 是同一个量、同一值域 1–108（第十四次变更把原语侧由 1–27 放宽到与本行一致，映射见下行）；「总张数 = 座位数 × `perSeat`」的守恒校验属 L2，不在 L1 值域闸门内 |
| `flow.deal` | `zone` | `ZoneId` | `zones` 里的 private 区 id | `hand`（缺省时取唯一 private 区；存在多个 private 区且未显式声明则 L1 `RS_SCHEMA_INVALID`） |
| `flow.deal` | `reveal` | bool | `true` / `false` | `false`（`false` = 发牌结果只对各自座位可见，私有区按 `(seat, zoneId)` 实例化；`true` = 整手发牌结果对全桌公开，仅调试与 M0 骨架使用） |
| `flow.turn` | `direction` | 枚举 | `clockwise` / `counterclockwise` | `clockwise` |
| `flow.turn` | `skipFinished` | bool | — | `true` |
| `flow.turn` | `partnerLead` | bool | — | `true`（接风开关；顶层唯一承载位就是本键，§5.10 接风行） |
| `flow.turn` | `afterAllPass` | 枚举 | `last-player`（默认）/ `next-player`（可选玩法，非掼蛋基线） | `last-player` | 两者的取舍只在「接风不成立」时才生效：判定顺序见 §5.13「全过后领出」行（第二十六次变更） |
| `flow.score` | `dimension` | 枚举 | `team` / `player` | `team` |
| `flow.rng` | `stream` | u32 | 0–3 | `0` |
| `flow.assert` | `minPlayers` | u8? | 1–8 | 缺省（不做玩家数断言） |
| `flow.assert` | `cardCount` | u32? | ≥0 | 缺省（不做牌数断言） |
| `flow.assert` | `zoneCapacity` | `[ZoneId]?` | 区号闭集 | 缺省空表 |

键名映射（顶层 `flow` 键与原语字段同义不同名，L1 按本映射对齐，不接受两套写法混用）：`flow.deal.zone`↔`deal.shuffle.zone`、`flow.deal.perSeat`↔`deal.shuffle.perSeat`、`flow.turn.skipFinished`↔`turn.skip-finished.skip`、`flow.turn.afterAllPass`↔`turn.after-all-pass.to`、`flow.assert.cardCount`↔`assert.card-count.expected`、`flow.assert.zoneCapacity`↔`assert.zone-capacity.zone`（顶层是区号集合，逐项对各区断言）、`flow.rng.stream`↔`rng.pick.stream`。

`flow.score` 取舍说明：v1 不建独立计分表本体；`score.table` 原语与其 `tableRef` 键移出 v1 清单、标 v2 预留（§5.2）。v1 计分只按 `flow.score.dimension` 归集名次与队级升级，不挂外部分值表；未来需要按外部分值表查表时，再在 v2 恢复 `score.table`。

必填性图例：✓ 必填；○ 可选；— 该态不存在。条件必填写作 `○（条件式）`，条件式必须能被 L2 判定（如 `levelTable` 行：`Settle::LevelUp` 引用它时由 L2 判为必填），不得出现 L2 判不了的附注。

一个最小片段（节选，说明"数据化"到什么程度）：

```yaml
rulesetId: guandan-classic
interpreter: ">=1.2,<2"
seats: { count: 4, teams: [[0,2],[1,3]] }
deck: { copies: 2, ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"], suits: [S,H,D,C], jokers: 2, exclude: [] }
zones:
  - { id: hand,    scope: private, capacity: 27 }   # 上限 27，按 (seat, hand) 实例化
  - { id: discard, scope: shared,  capacity: -1 }   # 无上限
levelTable:
  id: guandan-level
  initial: "2"
  ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"]
  upgrade: [ { teamRanks: [1,2], step: 3 },
             { teamRanks: [1,3], step: 2 },
             { teamRanks: [1,4], step: 1 } ]
naturalOrder: ["2","3","4","5","6","7","8","9","10","J","Q","K","A",little-joker,big-joker]
levelInsert: above-A
wildcard: { source: { suit: H, rank: "@level" }, substitute: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"], inStraightFlush: true }
patterns:
  - { id: single,           primitiveId: pattern.same-rank,           params: { count: 1 } }
  - { id: pair,             primitiveId: pattern.same-rank,           params: { count: 2 } }
  - { id: triple,           primitiveId: pattern.same-rank,           params: { count: 3 } }
  - { id: triple-pair,      primitiveId: pattern.triple-pair,         params: {} }
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
  ladder: [single, pair, triple, triple-pair, straight-5, plate-3, steel-2, bomb-4, bomb-5, bomb-6, bomb-7, bomb-8, joker-bomb]
  bombOrder: count-then-rank
  flushRank: above-bomb-5      # 同花顺位次：below-bomb-5 / above-bomb-5 / above-bomb-6 / above-bomb-8（C2 争议项，改这一个值即可切换）
tribute:
  enabled: true
  trigger: last-round-rank
  pairs:
    - { payer: 4, receiver: 1, when: single-loss }
    - { payer: 4, receiver: 1, when: double-loss }
    - { payer: 3, receiver: 2, when: double-loss }
  return: { ceiling: under-ten, count: 1, fallback: smallest }
  resist: two-big-jokers
lifecycle: [deal, tribute, play, settle]
actions:
  - { kind: play,    primitive: action.require-legal, legalFrom: patterns, params: { force: true, consume: hand } }
  - { kind: pass,    primitive: action.pass,         legalFrom: play,     params: {} }
flow:
  deal:   { zone: hand, perSeat: 27, reveal: false }
  turn:   { direction: clockwise, skipFinished: true, partnerLead: true, afterAllPass: last-player }
  score:  { dimension: team }
  rng:    { stream: 0 }
  assert: { minPlayers: 4, cardCount: 108 }
matchCap: { rounds: 12 }
settle: { algorithm: level-up, levelTableRef: guandan-level }
view: { schemaVersion: 2, fields: [hand, seat.levels, discard, lastPlay, round.level, round.attackingTeam] }
```

> 本示例是**草稿态** `RuleSetBody`：不含 `version`，发布时由服务端注入版本号并复制进 `ruleset`（§5.6/§5.8）。
> 示例中的点数一律按 `RankKey` 字符串字面值书写（`"2"`…`"10"`、`"J"`、`"Q"`、`"K"`、`"A"`，王为 `little-joker`/`big-joker`），与 L1 类型校验口径一致，不接受数字类型。

这段 YAML 是**校验基准**：L1–L4 必须能把它跑通；示例过不了自己定的校验，即视为设计缺陷而不是示例笔误。

### 5.2 原语清单（DSL 的能力边界就是这张表）

| 类别 | 原语 | 关键参数 | 允许改状态 |
| --- | --- | --- | --- |
| 发牌 | v1：`deal.shuffle`（落点区读顶层 `flow.deal.zone`）；**v1 未启用**：`deal.draw` / `deal.reveal` | 数量、目标区、是否公开 | 是 |
| 牌型 | v1：`pattern.same-rank` / `pattern.straight` / `pattern.consecutive-pairs` / `pattern.consecutive-triples` / **`pattern.triple-pair`** / **`pattern.bomb`** / **`pattern.straight-flush`** / **`pattern.joker-bomb`**；**v2 预留**：`pattern.with-attachment` / `pattern.all-same-suit` | 张数（`bomb` 支持 4～8）、连续长度、是否允许通配 | 否 |
| 比较 | `compare.by-rank` / `compare.by-count` / `compare.by-ladder` / **`compare.bomb-order`** | 比较维度（枚举）、炸弹序策略 | 否 |
| 行动 | v1：`action.require-legal` / `action.consume-cards` / `action.pass`；**v2 预留**：`action.claim` / `action.declare` | 是否强制、消耗区。`action.consume-cards` 在 v1 只有 `count = 0`（按出牌张数）一种用法，`count > 0` 标 v2 预留 | 是 |
| 轮转 | `turn.next` / `turn.after-all-pass` / `turn.skip-finished` / `turn.return-to-last` | 方向、全过后回谁、是否跳过已出完的人 | 是 |
| **进贡** | **`tribute.determine`** / **`tribute.pay`** / **`tribute.return`** / **`tribute.resist`** | 谁是贡方/受方（`pairs[]` 的 `when` 分支）；贡牌与还贡牌**由引擎按策略自动选定**（`pay`：除红桃级牌外点数最大、同点数取牌 id 最小；`return`：按 `tribute.return` 三键选定），贡方/受方没有选牌权；选定结果经 `TributeEvent` 回传；`resist` 命中后的分支 | 是 |
| **级牌** | **`level.current`** / **`level.upgrade`** / **`level.pass`** | 当前级牌来源、过 A 判定；升级档位读顶层唯一 `levelTable` | 是 |
| 计分 | v1：`score.accumulate` / `score.level-up`；**v2 预留**：`score.table` | 累加维度 | 是 |
| 视图 | `view.project` | 可见字段集合（白名单），读顶层 `view.fields` | 否 |
| 随机 | v1：`rng.pick`（`stream` 读顶层 `flow.rng.stream`）；**v1 未启用**：`rng.range` | 只能调用引擎注入的 RNG | 是 |
| 断言 | `assert.min-players` / `assert.card-count` / **`assert.zone-capacity`** | 用于 dry-run 前置检查 | 否 |

**原语参数结构（`params` 的唯一合法形状）**：每个 `primitiveId` 对应一个固定参数结构体，`patterns[].params` 与 `actions[].params` 都只能是该结构体的实例。L1 按 `primitiveId` 分型反序列化：未知键拒绝、缺失必填键拒绝、类型不符拒绝，一律 `RS_SCHEMA_INVALID`；`serde_json::Value` 不能作为参数容器（§5.3 红线）。下表"取值"列写 `必填` 的键不得省略，其余键可省并取默认值。

| 原语 | 参数字段（类型｜取值｜默认值） |
| --- | --- |
| `deal.shuffle` | `zone: ZoneId`｜必填，发牌落点区（v1 由顶层 `flow.deal.zone` 承载，§5.1）；`perSeat: u16`｜**1–108**（与顶层 `flow.deal.perSeat` 是同一个量、同一值域，键名映射见 §5.1；「每座位分得张数」，不是"每轮每人张数"；四座掼蛋的真值 27 仍在本域内，上界放宽到整副牌是为了三座／五座变体不必再改语法，见第十四次变更），默认 = `deck` 总张数 ÷ `seats.count` 的整除值；`reveal: bool`｜默认 `false` |
| `deal.draw` | `zone: ZoneId`｜必填；`count: u16`｜1–108，默认 1；`reveal: bool`｜默认 `false` |
| `deal.reveal` | `zone: ZoneId`｜必填；`toSeats: [u8]`｜默认 `[]`（空表 = 全桌可见） |
| `pattern.same-rank` | `count: u8`｜1–8，必填 |
| `pattern.straight` | `length: u8`｜5–12，必填；`wildcardAllowed: bool`｜默认 `true` |
| `pattern.consecutive-pairs` | `pairs: u8`｜2–10，必填（掼蛋取 3） |
| `pattern.consecutive-triples` | `triples: u8`｜2–6，必填（掼蛋取 2） |
| `pattern.triple-pair` | 无参：固定「3 张同点 + 1 对」共 5 张，只能为空对象 `{}`，出现任何键即拒绝（形状由原语自带，不做参数化；可配带牌的玩法走 v2 的 `pattern.with-attachment`） |
| `pattern.bomb` | `count: u8`｜4–8，必填 |
| `pattern.straight-flush` | `length: u8`｜5–12，必填 |
| `pattern.joker-bomb` | 无参：只能为空对象 `{}`，出现任何键即拒绝 |
| `compare.by-rank` | `direction: asc\|desc`｜默认 `asc` |
| `compare.by-count` | `direction: asc\|desc`｜默认 `asc` |
| `compare.by-ladder` | 无参：读顶层 `compare.ladder` |
| `compare.bomb-order` | 无参：读顶层 `compare.bombOrder` |
| `action.require-legal` | `force: bool`｜默认 `true`（必须能压过才合法）；`consume: ZoneId`｜默认 `hand` |
| `action.consume-cards` | `zone: ZoneId`｜必填；`count: u8`｜**v1 只允许 `0`**（`0` = 按本次出牌张数消耗；非 `0` 一律 L1 `RS_SCHEMA_INVALID`），`count > 0` 的「固定消耗 N 张」语义**标 v2 预留**（第二十七次变更） |
| `action.pass` | 无参：只能为空对象 `{}` |
| `turn.next` | `direction: clockwise\|counterclockwise`｜默认 `clockwise`；`skipFinished: bool`｜默认 `true` |
| `turn.after-all-pass` | `to: last-player\|next-player`｜默认 `last-player` |
| `turn.skip-finished` | `skip: bool`｜默认 `true`；`partnerLead: bool`｜默认 `true`（接风开关，顶层承载位为 `flow.turn.partnerLead`，§5.10 接风行） |
| `turn.return-to-last` | 无参：只能为空对象 `{}` |
| `tribute.determine` | `trigger: last-round-rank`｜默认 `last-round-rank`（当前唯一取值）；`firstRound: skip\|play`｜默认 `skip`（场次首局跳过 `tribute`，§5.13）。v1 承载位为顶层 `tribute.trigger`/`tribute.firstRound`（§5.1） |
| `tribute.pay` | `count: u8`｜1–2，默认 1；`selection: max-rank-except-wildcard`｜默认该值（当前唯一取值）。v1 承载位为顶层 `tribute.pay`（§5.1） |
| `tribute.return` | `ceiling: under-ten\|none`｜默认 `under-ten`；`count: u8`｜1–2，默认 1；`fallback: smallest\|largest`｜默认 `smallest` |
| `tribute.resist` | 无参：读顶层 `tribute.resist` |
| `level.current` | `scope: by-team\|global`｜默认 `by-team`（掼蛋每队一个 `levels`）。v1 承载位为顶层 `level.current`（§5.1） |
| `level.upgrade` | 无参：引用顶层唯一 `levelTable`（L2 校验全 RuleSet 内不出现第二张级牌表） |
| `level.pass` | `combos: [[u8; teamSize]]`｜每个元素是该队规模个名次（长度必须 = `teamSize`、元素 `1..=seats.count`、互不相同且升序）；默认 `[[1,2],[1,3]]` 仅在 `teamSize = 2` 时成立，其余规模必须显式声明（`1+2`/`1+3` 即过 A；编码与 `levelTable.upgrade[].teamRanks` 共用同一套名次编码）；`streakLimit: u8`｜1–3，默认 3（`aFailStreak` 阈值；上界与冻结值域 0–3 同源，>3 须另走 EGM 裁决）；`resetOnFail: bool`｜默认 `true`（达阈值退回 `levelTable.initial` 并清零）。v1 的实际承载位是顶层 `level.pass`（§5.1），本节是同一结构的原语侧定义 |
| `score.accumulate` | `dimension: team\|player`｜默认 `team` |
| `score.level-up` | 无参：结算变体由顶层 `settle.algorithm` 决定 |
| `view.project` | 无参：读顶层 `view.fields`（白名单），不接受额外键 |
| `rng.pick` | `stream: u32`｜0–3，默认 0 |
| `rng.range` | `min: u32`、`max: u32`｜必填（`min ≤ max`）；`stream: u32`｜0–3，默认 0 |
| `assert.min-players` | `count: u8`｜1–8，必填 |
| `assert.card-count` | `expected: u32`｜必填 |
| `assert.zone-capacity` | `zone: ZoneId`｜必填 |

**v2 预留原语**（v1 ADT 不收录，RuleSet 引用即 L1 拒绝 `RS_SCHEMA_INVALID`）：`pattern.with-attachment`、`pattern.all-same-suit`、`action.claim`、`action.declare`、`score.table`。这五个原语在 §5.2 分类表里同样按 v2 标注，v1 的解释器不实现它们。其中 `pattern.with-attachment` 只保留给"带牌数与主牌数都可配"的玩法（三带一、四带二等）；掼蛋的三带二是**固定形状**，由 v1 新原语 `pattern.triple-pair` 承担（第十八次变更）。

**v1 未启用原语**（口径同上：v1 的 Primitive ADT 不收录，RuleSet 引用即 L1 `RS_SCHEMA_INVALID`）：`deal.draw`、`deal.reveal`、`rng.range`。三者参数定义保留在参数表，v2 启用时再补各自的顶层承载位；v1 基线的发牌只走 `deal.shuffle`、随机只走 `rng.pick`，因此 §5.1 不需要为它们新增承载键。不启用的理由：v1 是一次性发牌到 `flow.deal.zone` 指定的区（发完即进入 `play`），观战走回放式重放（§6 观战章 / U8），既没有中场抽牌也没有翻牌需求；`rng.range` 仅在 v2 的随机玩法参数里才需要。

**发牌随机源的确定性定义（N7–N9 固化，2026-11-24 第十二次变更）**：此前 §5.12 只规定「RNG 唯一来源、`rng.counter` 递增」，PRNG、洗牌与发牌顺序三项**没有定义**，实现端只能自选；而这三项一旦被 §10.3 的黄金向量锁死，换算法会让全部 `stateHash` 锚点一起漂移。故在此按用户裁决固化，**实现端不得自选算法**：

| 项 | 固化内容 |
| --- | --- |
| PRNG | 随机字 = `BLAKE3-256(seed_le64 ‖ stream_le32 ‖ counter_le64)` 输出的**前 8 字节**按 little-endian 解释为 u64；输入是 20 字节定长拼接，不含其它字段 |
| 计数 | 每消耗一个随机字（**含被拒绝的那一次**）`counter += 1`；`counter` 即 §5.12 第 8 项进哈希的 `rng.counter`，单调、不跳号；禁止批量预取、禁止并行预取（否则双端消耗序不同） |
| 有界取值 | `bounded(n)`（`n ≥ 1`）：`limit = ⌊2^64 / n⌋ * n`，随机字 `< limit` 则返回 `word % n`，否则 `counter += 1` 取下一个随机字重试（**拒绝采样**，循环到取到为止）；禁止直接用 `word % n` 回避模偏 |
| 首局定先手抽牌 | **仅场次首局执行，且在 `deal.shuffle` 之前**（第十六次变更，2026-11-24 用户追认「抽牌前置」）：先做一次 `bounded(总张数)`，在载入期冻结的牌 id 序 `[0, 总张数)` 上取下标 `i`，即「被抽出的那张牌」的牌 id；该牌**不剔出牌堆**，之后照常洗牌、发牌。发完后按牌 id 定位它落在哪个座位（牌 id 全局唯一，切分后唯一归属），该座位先出、其所属队即 `attackingTeam`。非首局**不做这一步、不消耗任何随机字**（打级队与先出座位都由上一局结果决定，避免重复消耗导致双端序不一致） |
| 洗牌 | `deal.shuffle` 对载入期冻结的牌 id 序 `[0, 总张数)` 做 **Fisher-Yates 降序**：`for i in (1..n).rev() { j = bounded(i + 1); swap(a[i], a[j]); }`（`j` 的取值范围始终是 `0..=i`） |
| 发牌顺序 | 洗牌结果按**座位序号升序**、按 `flow.deal.perSeat` 逐座位**连续切分**（座位 0 取前 `perSeat` 张，座位 1 取紧随的 `perSeat` 张，依次递推），v1 发完必须剩 0 张；掼蛋 108 ÷ 4 = 27 |
| `stream` | `flow.rng.stream`（0–3）参与 PRNG 输入，用于隔离不同用途，默认 0；同一局内 `deal.shuffle` 与 `rng.pick` **共用同一条 `(seed, stream, counter)` 序列**，各自不从 0 重来 |
| 禁止项 | 不取系统熵、不取时间戳、不用浮点（§3.3）；`seed` 由房间创建时生成并写入快照与回放头，重放与崩溃恢复一律用同一 `seed` |

判据：全链路只含整数运算与 BLAKE3-256，同一 `(seed, stream, 牌堆序)` 下双端必须逐字节同序；门禁看 §5.12 第 8 项（`counter` 与 `seedDigest`）加上 §10.3 黄金向量的 `stateHash` 全量比对。改动本节任一行即视为 `stateHash` 口径变更，须重算全部锚点。

**红线（必须在类型系统里保证，不靠文档约定）**：

1. 没有表达式：所有条件都是"枚举 + 查表"，`compare` 的取值只能是上表列出的维度。
2. 没有循环：批量化靠原语参数（如 `length: 5`），不提供 `for`。
3. 没有用户自定义函数：RuleSet 不能定义新原语，也不能引用外部代码。
4. 引用必须强类型：`PatternId` / `ZoneId` / `ActionId` 是 newtype，反序列化后必须能在索引里查到。

### 5.3 类型约束的落地方式

- `RuleSetBody` 用**固定变体的 ADT**（`enum Primitive { SameRank{..}, Straight{..}, .. }`），`serde` 反序列化失败即拒绝加载。
- `params` 同样落在 ADT 上：`enum PrimitiveParams { Pattern(PatternParams), Action(ActionParams), .. }` 按 `primitiveId` 分型，每个变体是具名结构体（如 `SameRank { count: u8 }`）。不给 `Option<Value>`、不给 `extra: BTreeMap<String, Value>` 这类兜底槽；L1 反序列化开 `deny_unknown_fields`，未知键直接失败（`RS_SCHEMA_INVALID`）。
- 禁用 `serde_json::Value` 与 `#[serde(flatten)]` 作为字段类型，避免出现"任意 JSON 透传"的逃生口。
- ID 引用在解析期构建索引，缺失即 `RS_REF_MISSING`；索引也用于编辑器做下拉候选。

### 5.4 校验四级（后台发布前的闸门）

| 级别 | 检查什么 | 失败表现 | 耗时预算 |
| --- | --- | --- | --- |
| L1 结构 | JSON Schema + ADT 反序列化；`seats.count` 不在 2–8 内、`timing` 各字段与 `matchCap.rounds` 取值范围校验；`patterns[].params`/`actions[].params` 按 `primitiveId` 分型校验（§5.2 参数表），未知键、缺失必填键、类型不符一律拒绝；`ladder` 出现 `straight-flush-*` 元素拒绝；`levelTable.upgradeBasis` 不在闭集、两个 basis 的键集同时出现或错位拒绝（`rank-combo` 要求 `upgrade[]` 齐备且无 `rankScores`/`upgradeByDiff`，`score-diff` 反之）；`rank-combo` 下 `levelTable.upgrade[]` 的 `teamRanks` 长度 ≠ 该队 `teamSize`、元素不在 `1..=seats.count`、未升序或与另一条重复，`step` 不在 0–3，一律拒绝；`score-diff` 下 `rankScores` 长度 ≠ `seats.count`、`upgradeByDiff` 未按 `minDiff` 降序或缺 `minDiff: 0` 兜底、`flow.score.dimension ≠ team` 拒绝；`level.pass.combos` 为空表、元素长度 ≠ `teamSize`、元素不在 `1..=seats.count`、未升序或重复、`level.pass.streakLimit` 不在 1–3 拒绝（值域与 §5.1/§5.2 一致，原写 1–9 属笔误，2026-11-24 已对齐；名次编码于同日第十二次变更参数化）；`flushRank` 插入位校验（**仅在 RuleSet 声明了插入型牌型时做**，无 `straight-flush-*` 时惰性跳过）：锚点按 `primitiveId = pattern.bomb` + `params.count` 语义识别，锚点缺失报 `RS_FLUSH_ANCHOR_MISSING`、位次冲突报 `RS_LADDER_CONFLICT`，两者均拒绝且不降级 | 字段缺失、类型不符、未知原语、计时参数越界、参数键越界、`ladder` 混入插入型牌型、升级档重复/越界；分别落 `RS_SCHEMA_INVALID`（结构/枚举/取值）、`RS_FLUSH_ANCHOR_MISSING`（锚点缺失）或 `RS_LADDER_CONFLICT`（位次冲突） | <10ms |
| L2 引用 | 所有 ID 能在索引里找到；`lifecycle` 每个有动作的 phase 都有可执行原语；`seats.teams` 覆盖全部座位且不重叠（`count` 的 2–8 范围已在 L1 判，本级别不重复）；**静态牌堆校验**：`(ranks − exclude) × suits × copies + jokers × copies` = 总张数（掼蛋基线 13×4×2 + 2×2 = 108，与 §5.1 同式），且总张数 ≤ 各 `hand` 区容量之和（容量是上限，不是当前牌数）；每区 `capacity` ≥ 0 或 −1（−1 表无上限，不参与静态求和）；`levelTable` 全 RuleSet 唯一引用，`level.upgrade`/`settle.levelTableRef` 不得指向第二张级牌表 | 指向不存在的牌型/区域（`RS_REF_MISSING`）；phase 无可执行原语（`RS_PHASE_UNIMPLEMENTED`）；队伍覆盖不全或重叠（`RS_TEAM_COVERAGE_INVALID`）；牌堆静态张数不符或总张数超出 `hand` 区容量之和（`RS_DECK_COUNT_MISMATCH`）；容量取值非法（`RS_CAPACITY_INVALID`）；级牌表引用不唯一（`RS_REF_MISSING`） | <50ms |
| L3 可玩性 | 随机发牌 + 随机合法动作跑 200 局：必须全部终局（终局条件即 §5.13「局终止与名次」）、无死锁、非法动作率 0；dry-run 期单次 `legal_actions` 实测候选枚举峰值超 4096 条即判本级别失败。必须记录每局 seed 与失败局的最小动作序列，失败可复现 | 死循环、无终局、非法动作（`RS_NO_TERMINATION`/`RS_ILLEGAL_ACTION_RATE`）；候选枚举峰值超 4096（`RS_QUOTA_EXCEEDED`，归 L3，不在载入期 L2 设闸门） | **预算 ≤ 90 秒（4 并发）**：单局 300 步 × 0.8ms ≈ 0.24s，200 局串行约 48s，4 并发约 12s；90s 预留 7 倍余量给"慢机器 + 失败局留档" |
| L4 回归 | 黄金用例（真实对局录制或手工构造的 action log）逐条重放，`stateHash` 必须一致 | 规则改动破坏了旧局 | 按用例数线性，单条 ≤ 1s |

**运行时牌数断言**（不属于 L2 静态校验，在发牌后与每步 `apply` 后由引擎断言）：Σ 各区**当前**牌数 = deck 总张数，且每区当前牌数 ≤ `capacity`（`capacity = -1` 的区跳过容量检查）。守恒失败报 `RS_ZONE_CONSERVATION`，超上限报 `RS_ZONE_CAPACITY`，两码触发条件与 §5.7 逐字一致；二者与 L2 的 `RS_CAPACITY_INVALID` 明确分工：前者是运行期当前牌数超上限，后者是载入期 `capacity` 配置取值非法。

**校验结果类型与引擎错误**：

- `Validation = { ranLevels: HashSet<Level>, findings: Vec<Finding> }`，`Level = L1|L2|L3|L4`，`Finding = { level: Level, code: ErrorCode, severity: Severity, location: String, message: String }`，其中 `ErrorCode` 是 §5.7 全表 27 个码的枚举、`Severity = Blocker|Major|Minor`。`validate(&RuleSetBody, levels: &[Level]) -> Validation`：入参 `levels` 声明本次要跑的级别集合，`ranLevels` 记录实际跑完的级别；`findings` 元素结构见 §5.6。
- `EngineError` 变体清单（`apply`/`legal_actions` 的错误出口）：`IllegalAction { reason }`（动作非法，运行期拒绝）、`QuotaExceeded`（候选枚举峰值超 4096，对应 `RS_QUOTA_EXCEEDED`）、`Internal { message }`（引擎内部不变量破坏，按 `RS_INTERNAL_PANIC` 同族告警处置）、`Panic`（native 侧 panic 被 `catch_unwind` 捕获，对应 `RS_INTERNAL_PANIC`）、`ZoneConservation`（Σ 各区当前牌数 ≠ deck 总张数，对应 `RS_ZONE_CONSERVATION`）、`ZoneCapacity { zone: ZoneId, count: u32 }`（某区当前牌数 > `capacity`，对应 `RS_ZONE_CAPACITY`）。

L3 与 L4 都在**发布时**跑，不在编辑时跑；编辑时只跑 L1、L2 并实时标红。

### 5.5 版本与回放绑定

| 版本项 | 写在哪 | 作用 |
| --- | --- | --- |
| `engineVersion` | 回放头、房间快照 | 判定是否有跨版本行为差异 |
| `interpreterVersion` | 回放头、RuleSet 校验、`ruleset.pinned_interpreter_version` | RuleSet 声明的 `interpreter` 区间在此校验；发布时解析为具体版本号并锁定进 `pinned_interpreter_version` |
| `rulesetVersion` | 回放头、房间创建时锁定 | 运行中的房间不随发布切换版本 |

一条硬规则：**房间在创建时锁定 RuleSet 版本，直到该场结束**。发布只影响新房间，否则同一局里规则会中途变化，`stateHash` 就不可能稳定。

### 5.6 后台 API 契约

| 接口 | 入参 | 出参 | 语义 |
| --- | --- | --- | --- |
| `POST /admin/rulesets` | `{rulesetId, name, baseVersion?, draftKey?}` | `{draftId}` | 新建草稿；幂等键为调用方显式传入的 `draftKey`（缺省由服务端按 `rulesetId + baseVersion` 生成）；**同一 `baseVersion` 允许并存多份草稿**，以 `draftId` 自身去重，重复 `draftKey` 调用返回同一 `draftId`。`name` 为后台展示名，随草稿 `body` jsonb 一并存储，不单列字段；`baseVersion` 不建独立持久化列，作为 `draftKey` 派生输入，选定基线随 `body` 快照留存 |
| `PUT /admin/rulesets/{id}/drafts/{draftId}` | `RuleSetBody`（草稿态，无 `version`） | `{draftId, rev}` | 覆盖草稿（**只对草稿生效**），必须带 `If-Match: rev`；`rev` 不匹配返回 409。已发布的 `version` 一律拒绝写 |
| `POST /admin/rulesets/{id}/drafts/{draftId}/validate` | `{levels: [L1,L2,L3,L4]}` | `{taskId}` | **异步**：立即返回任务号，结果轮询 `GET …/tasks/{taskId}`；L3/L4 必然超出 HTTP 超时 |
| `GET /admin/tasks/{taskId}` | — | `{state: running\|done\|failed, findings[], stats}` | 校验与发布任务的统一查询口 |
| `POST /admin/rulesets/{id}/publish` | `{draftId, stage: canary}` | `{releaseId}` | 四步事务，任一步失败则 stage 不推进并留审计：①把草稿 `body` 复制进 `ruleset`；②分配不可变 `version`；③解析 `interpreter` 区间并锁定进 `pinned_interpreter_version`；④推进 stage 进 `canary`。`ruleset_release` 只在进 `canary` 时创建行；`draft`/`validated` 不留 release 行，其状态由 `ruleset_validation` 聚合派生。入参 `stage` 固定 `canary`、不接受其它值；`draft → validated` 由校验任务完成且无 Blocker 后推进（见状态机表），publish 不跨级；当前状态非 `validated` 或 `stage` 非法返回 409。`Idempotency-Key` 必填 |
| `POST /admin/releases/{releaseId}/promote` | `{stage: full, reason, totp}` | `{releaseId}` | 承担 `canary` → `full` 推进：`canary` 达标后由人工按 TOTP 确认调用本接口写入 `full`；跨级返回 409；`Idempotency-Key` 必填 |
| `POST /admin/rulesets/{id}/rollback` | `{badReleaseId, reason}` | `{releaseId}` | 入参 `badReleaseId` 指明被回滚的坏版本（服务端也可据此排除当前 active）。回滚候选集 = 同 `ruleset_id` 下曾到过 `full`、未 `retired`，且**排除当前 active**、**排除 `rolled_back` 旧行**的版本，按 `activated_at` 倒序取最近一个；候选为空返回 `RS_NO_ACTIVE_VERSION`（409，无可回滚版本，不切指针）；出参 `releaseId` 为**目标版本**的 release 行（不是坏版本行）；`Idempotency-Key` 必填，**回滚不需要 TOTP**（§8.1） |
| `GET /admin/games` | — | `[{gameId, activeVersion, stage, roomCount, canaryVersion}]` | 展示用，不做判定 |
| `GET /admin/audit` | `{actor, from, to, action}` | `[{ts, actor, action, target, before, after, ip, sessionId}]` | 字段与 `audit` 库的 `audit_log` 表一致（见 §5.8） |
| `GET /api/games` | — | `[{gameId, activeVersion, minClientVersion}]` | **客户端侧**接口；`minClientVersion` 是**独立配置项**（§5.16），不由 `interpreter` 区间推导 |

**`gameId` 与 `rulesetId` 的映射**：`gameId` 标识玩法族（如 `guandan`、`guandan-variant`），一个 `gameId` 在任一时刻对应一份生效的 RuleSet 版本；`rulesetId` 是 RuleSet 身份（kebab-case），同一 `rulesetId` 可有多个不可变 `version`。建房与匹配入参传 `gameId`：新建房间按 `gameId` 取 `game:{id}:active_ruleset` 指向的 `(rulesetId, version)`；10% 灰度房取 `game:{id}:canary_ruleset`。版本选取的唯一真相在 `ruleset_release` 表，Redis 键只是缓存（§5.8/§9.5）。

**校验结果 `findings[]` 结构**（`GET /admin/tasks/{taskId}` 出参与 `ruleset_validation.findings` 同形）：`Finding{level: L1|L2|L3|L4, code: ErrorCode（§5.7 全表枚举）, severity: Severity（Blocker/Major/Minor）, location: string, message: string}`。severity 判定：发布闸门失败类码（`RS_SCHEMA_INVALID`、`RS_REF_MISSING`、`RS_PHASE_UNIMPLEMENTED`、`RS_NO_TERMINATION`、`RS_ILLEGAL_ACTION_RATE`、`RS_L4_MISMATCH`、`RS_GOLDEN_REGRESSION_FAILED`、`RS_QUOTA_EXCEEDED`、`RS_INTERPRETER_TOO_OLD`、`RS_DECK_COUNT_MISMATCH`、`RS_CAPACITY_INVALID`、`RS_TEAM_COVERAGE_INVALID`、`RS_FLUSH_ANCHOR_MISSING`、`RS_LADDER_CONFLICT`）为 **Blocker**，阻塞进 `canary`；运行期与操作类码（`RS_ZONE_CONSERVATION`、`RS_ZONE_CAPACITY`、`RS_INTERNAL_PANIC`、`RS_SEAT_TAKEN`、`AUTH_INVALID`、`CLIENT_TOO_OLD`、`RATE_LIMITED`、`STALE_SEQ`、`TOO_LATE`、`RS_CANARY_BLOCKED`、`RS_VERSION_LOCKED_BY_ROOMS`、`VERSION_CONFLICT`、`RS_NO_ACTIVE_VERSION`）为 Major/Minor，不进发布闸门判定。`validated` 进入条件"无 Blocker 级 finding"即按本表判定。

发布语义：`canary` 只对新建房间的 10% 生效，观察窗口 30 分钟；窗口内出现 `stateHash`/`viewHash` 不匹配、`action_illegal` 超标（CPU 超预算按 §8.6 处置，不计入本判定）或崩溃率超标，自动回退到上一 `active` 版本（候选集口径见 §5.6 rollback 行）并落审计；达标后由人工经 `promote` 接口带 TOTP 确认转 `full`（§9.4）。**不再设 `ramp5`/`ramp25` 两个中间档**：内测规模下单档灰度 + 一次全量已经足够，多两级只增加人工推进成本（§8 的 YAGNI 剪枝）。

**发布事务与行创建口径**：`ruleset_release` 只在进 `canary` 时创建行；`draft`/`validated` 阶段不建 release 行，其阶段状态由 `ruleset_validation` 聚合派生，不单独存 `stage` 列。`publish` 四步（复制 body → 分配 `version` → 锁定 `pinned_interpreter_version` → 推进 stage）在同一事务内完成，任一步失败则 stage 不推进、已写中间痕迹留审计；`promote` 与回滚只改既有 release 行的 `stage`（回滚目标行的 `release_id` 即 `POST rollback` 的出参）。

**低样本出口**：canary 观察窗口内样本动作数 < 200 时，不按比率判定，改为"零 P0 事件"判定并把窗口延长到 2 小时；满 2 小时仍 < 200 的，由人工 TOTP 决定放行（`promote` 到 `full`）或回滚，决定与理由落审计，不允许流程停在无人拍板的位置。

**发布状态机**（接口 `stage`、表 `stage`、文档阶段名必须一一对应；`draft`/`validated` 由 `ruleset_validation` 聚合派生，不落 release 行，见 §5.8）：

| 状态 | 进入条件 | 允许的下一步 |
| --- | --- | --- |
| `draft` | 新建或 PUT 草稿（`ruleset_draft` 行；`activated_at` 为空） | `validated`（L1–L4 全过） |
| `validated` | 校验任务 `done` 且无 Blocker 级 finding | `canary` |
| `canary` | `publish` 事务推进进 `canary`，此时创建 `ruleset_release` 行，10% 新房间 | `full`（经 `promote` + 人工 TOTP 确认） / `rolled_back` / 低样本待决后人工放行或回滚 |
| `full` | 全量，进入 active 候选；**active 唯一判定式 = `stage=full` 且 `activated_at` 最大** | `superseded`（被新的 `full` 取代） / `retired`（人工确认下线） / `rolled_back` |
| `superseded` | 曾到 `full`，已被更新的 `full` 版本取代；仍可作为回滚目标 | `retired` / 经回滚选为目标后回到 `full` 指针 |
| `rolled_back` | 回退旧行（**不写 `retired`**） | 重新校验通过后可合法转移回 `canary` / `full`；否则只读，可重新发布新版本 |
| `retired` | 人工确认下线（不再被任何房间引用且人工确认）；`retired` 只表示人工确认下线，与"被取代"无关 | 只读 |

### 5.7 错误码

| 码 | 触发 | 处置 |
| --- | --- | --- |
| `RS_SCHEMA_INVALID` | L1 失败 | 拒绝保存草稿 |
| `RS_FLUSH_ANCHOR_MISSING` | L1 失败：RuleSet 声明了插入型牌型（`straight-flush-*`），但 `ladder` 中找不到 `flushRank` 所指张数的炸弹锚点（按 `primitiveId = pattern.bomb` + `params.count` 识别）；**不做降级插入** | 拒绝保存草稿，标出期望的张数与 `ladder` 中现有的炸弹张数 |
| `RS_LADDER_CONFLICT` | L1 失败：`flushRank` 锚点存在，但插入位被多个插入型牌型争夺，或插入后与 `ladder` 相邻项的相对序矛盾 | 拒绝保存草稿，标出冲突的插入位与相邻牌型 id |
| `RS_REF_MISSING` | L2 失败 | 拒绝发布，标出缺失引用 |
| `RS_NO_TERMINATION` | L3 出现无法终局的局 | 拒绝发布 |
| `RS_ILLEGAL_ACTION_RATE` | L3 非法动作率 > 0 | 拒绝发布 |
| `RS_INTERPRETER_TOO_OLD` | `interpreter` 区间不满足 | 拒绝加载，提示升级下限或改 RuleSet |
| `RS_VERSION_LOCKED_BY_ROOMS` | 删除/覆盖仍被房间引用的版本（预留：v1 不提供版本删除/覆盖端点，本码待后续补端点时启用） | 拒绝，列出引用房间数 |
| `RS_CANARY_BLOCKED` | 灰度期间指标超标 | 自动回滚并落审计 |
| `RS_L4_MISMATCH` | L4 黄金用例重放 `stateHash` 不一致（与 `RS_GOLDEN_REGRESSION_FAILED` 分工：本码只管黄金用例哈希不符） | 拒绝发布，输出差异局 id 与首个分歧步 |
| `RS_ZONE_CONSERVATION` | **运行时**守恒失败：Σ 各区当前牌数 ≠ deck 总张数（与 §5.4 运行时断言同一条件） | 判该场流局，按 §6.5 处置 |
| `RS_ZONE_CAPACITY` | **运行时**某区当前牌数 > `capacity`（`capacity = -1` 的区跳过本检查；与 §5.4 运行时断言同一条件） | 判该场流局，按 §6.5 处置 |
| `RS_PHASE_UNIMPLEMENTED` | `lifecycle` 里的有动作 phase 没有任何可执行原语（归入 L2 引用完整性校验） | 拒绝加载 |
| `RS_DECK_COUNT_MISMATCH` | L2 静态牌堆校验：`(ranks − exclude) × suits × copies + jokers × copies` 与 deck 声明总张数不符，或该总张数超过各 `hand` 区 `capacity` 之和 | 拒绝发布，标出张数公式两侧取值与容量上限 |
| `RS_CAPACITY_INVALID` | L2 静态校验：某区 `capacity` 取值非法（既不是 ≥0 的整数，也不是 −1 无上限）；与运行期 `RS_ZONE_CAPACITY` 分工：本码管载入期配置取值，后者管运行期当前牌数超上限 | 拒绝发布，标出非法区号与取值 |
| `RS_TEAM_COVERAGE_INVALID` | L2 校验：`seats.teams` 未覆盖全部座位，或座位在多个队中重复出现 | 拒绝发布，标出漏座/重叠座位号 |
| `RS_QUOTA_EXCEEDED` | L3 dry-run 期单次 `legal_actions` 实测候选枚举峰值超过 4096 条；**不在载入期 L2 设静态闸门，不用于运行期单步动作拒绝** | 判 dry-run 结果无效、拒绝发布，记 `action_quota_exceeded_total`；RuleSet 侧按缺陷处理 |
| `RS_GOLDEN_REGRESSION_FAILED` | L4 回归种子库出现失败（与 `RS_L4_MISMATCH` 分工：本码只管回归种子库） | 拒绝发布，输出首个失败用例 id 与 `matchId`/`roomSeq` |
| `RS_INTERNAL_PANIC` | 解释器在 native 侧 panic 且被 `catch_unwind` 捕获 | 该动作返回失败、房间继续（§8.6），并记 `room_panic_total` |
| `RS_SEAT_TAKEN` | 进房时目标座位已被占；或房间已满、无空座可分配 | 不经 HTTP 409：经 `JoinAck.reason` 返回，并附 `availableSeats[]`；客户端按可用座位重选，或提示房间已满（§5.9） |
| `AUTH_INVALID` | token 缺失、过期或签名不符；短信验证码错误或过期；或请求携带的 `seat` 与 token 绑定座位不一致 | 401，客户端跳登录；座位不一致另记指标（§5.9） |
| `CLIENT_TOO_OLD` | `clientVersion` 低于服务端当前的 `minClientVersion` 下限 | 426，客户端提示升级并停止入座（`minClientVersion` 定义见 §5.16） |
| `RATE_LIMITED` | 单账号动作频率超限 | 429，返回 `retryAfterMs` |
| `STALE_SEQ` | 动作 `seatSeq` 小于该座位当前水位（按座位水位判定，与 `TOO_LATE` 区分） | 动作级：写入 `ActionAck.reason`；客户端重新拉增量（见 §6.2） |
| `TOO_LATE` | 动作与超时同 tick 竞态落败：动作在"超时事件"入队之后到达（`STALE_SEQ` 是序号过期，本码是竞态落败，两者不混用） | 动作级：写入 `ActionAck.reason`；丢弃该动作、不推进 `seatSeq`、不计入该座位超时次数；客户端按座位水位重拉增量，不重发同一动作 |
| `VERSION_CONFLICT` | `If-Match` 的 `rev` 与草稿当前 `rev` 不符 | 409，返回当前 `rev` 与字段级差异 |
| `RS_NO_ACTIVE_VERSION` | 回滚候选集为空：同 `ruleset_id` 下无曾到 `full` 且未 `retired` 的版本 | 409，提示无可回滚版本，不切指针 |

**载体映射（四分）**：牌桌走 WS 帧、不带 HTTP 状态码。①连接级错误（`AUTH_INVALID`、`CLIENT_TOO_OLD`）经 WS Close code 断开连接并附错误码；②请求级错误（`RS_SEAT_TAKEN`）经入座响应帧 `JoinAck.reason` 返回并附 `availableSeats[]`；③动作级错误只写入 `ActionAck.reason`，不断连；④HTTP 状态码（401/409/426/429 等）仅用于后台与玩家侧 REST 接口。

### 5.8 存储结构

**规则发布链**（4 张）：

| 表 | 关键字段 | 说明 |
| --- | --- | --- |
| `ruleset_draft` | `draft_id, ruleset_id, draft_key, rev, body jsonb, updated_by, updated_at` | 草稿可反复覆盖，`rev` 用于 `If-Match`；同一 `baseVersion` 可并存多份草稿，以 `draft_id` 去重；草稿态 `body` 不含 `version` |
| `ruleset` | `ruleset_id, version, body_hash, body jsonb, pinned_interpreter_version, frozen_at` | 发布时从草稿复制，**此后只读**；`body` 与 `status` 分离，`status` 在 release 表；`pinned_interpreter_version` 是发布时把 `interpreter` 区间解析锁定的具体解释器版本（回放头 `interpreterVersion` 同值） |
| `ruleset_release` | 主键 `release_id`（`ruleset_id` 普通索引）；`release_id, ruleset_id, draft_id, rev(普通列，不入主键), version, stage(canary/full/superseded/rolled_back/retired), activated_at, first_activated_at, rolled_back_at, operator` | 状态机唯一真相（§5.6）；**只在进 `canary` 时创建行**；`draft`/`validated` 阶段不留 release 行，其阶段由 `ruleset_validation` 聚合派生，不单独存 `stage` 列。`activated_at` 为空即未生效；`first_activated_at` 是首次进入 `full` 的时刻，**永不可改写**；`activated_at` 为最近一次生效时刻。**active 唯一判定式 = `stage=full` 且 `activated_at` 最大**。`superseded` = 曾到 `full`、已被更新 `full` 取代、仍可回滚；`retired` = 人工确认下线。回滚时：坏版本行写 `rolled_back`，目标版本行 `activated_at` 更新为回滚时刻、`first_activated_at` 不动，`POST rollback` 出参返回目标版本 `release_id`。本表是版本状态唯一真相，Redis 中的 `active_ruleset`/`canary_ruleset` 指针只是缓存（§5.8 Redis 清单、§9.5） |
| `ruleset_validation` | 主键 `(ruleset_id, draft_id, rev, level)`；`task_id, ruleset_id, draft_id, rev, level, passed, findings jsonb, stats jsonb, seed_list jsonb, ran_at` | L1–L4 留档；`seed_list` 保证可复现；草稿/校验阶段无 `version`，一律按 `draft_id + rev` 关联 |

**审计单独一张库**：`audit_log` 不属于上表（它不是规则发布链的一部分），独立 schema `audit` + 只允许 append 的库角色，业务侧账号无删除权（§8.8）。

**业务与账务**（首版必建，即使不接渠道也要有表）：

| 表 | 关键字段 | 说明 |
| --- | --- | --- |
| `player_profile` | `player_id, phone_hash, phone_tail4, nickname, created_at, status` | 手机号只存哈希 + **尾号 4 位**（展示用，§8.4），原文不落库；`phone_hash = HMAC-SHA256(server_salt, 规范化后的 11 位手机号)`；盐为单盐存环境变量，轮换按 §9.7 破坏性变更五步执行，不预留双盐列/双次查找结构 |
| `player_stat` | `player_id, game_id, rating, rating_games, placement_left, rank_level, rounds_total, wins, updated_at` | 段位字段名叫 `rank_level`（**不叫 `level`**，避免与房间级牌混淆）。`rating` 是整数内部评分；`rating_games` 是已计入评分的场次数；`placement_left` 是定级赛剩余场次；`rank_level` 为档位字段。**v1 只保留字段与跨端一致（S9）**：定级分档、Elo 式浮动与 K 衰减算法整体降为 v2 预留（§5.13、§8.7、§10.2 R18/R19）；`rating`/`rank_level` 只在服务端结算事务内用整数运算算出，**不进 `stateHash`、不进双端比对**。`human-casual`/`bot`/`mixed` 不写本表的段位、评分与排行字段；`ai-practice` 是人机房/离线局的模式侧别名，不是 `matchType` 字面值，落库记 `bot`，同不写本表字段（按 §5.15 真值表判定） |
| `round_record` | `match_id, round_no, players jsonb, result jsonb, levels_after jsonb, ruleset_version, engine_version, interpreter_version, started_at, ended_at` | 一局一行。`players` = `[{seat, player_id, rank}]`，`rank` 按 §5.13「局终止与名次」的规则填写（1–4 名，全表唯一口径）；`result`（结构名 `settlement`）= `{winner_team, ranks: [seat...], points}`，`ranks` 按名次 1→4 列 seat；`levels_after` 为按队伍序号升序的 `u8` 数组（每队级牌快照）；`aFailStreak` 不落本表，由动作日志确定性重放重建（§6.5/§7.7），旧存档/旧快照缺该字段时按 0 初始化；三处 jsonb 形状以本行为准，对账、战绩查询与 §9.7 迁移断言按同一结构比对 |
| `match_record` | `match_id, game_id, match_type(human-ranked/human-casual/bot/mixed), winner_team(可空), end_reason(passed-a/safety-cap/aborted), seats_count, started_at, ended_at` | 一场一行；`matchType` 只在这里定义，其它地方引用它。`winner_team` 为胜方队序号，平局时为空；`end_reason` 标记场次终止原因（`passed-a` 过 A、`safety-cap` 达安全局数上限、`aborted` 整场流局或全员离线超时中止，按 §6.3/§6.5 处置）；段位与积分按**场**结算 |
| `settlement_dead_letter` | `settlement_id, match_id, round_no, payload jsonb, failed_at, retries, resolved_at` | 结算三次写库失败后的死信，**只用于结算补写**，人工按 `settlementId` 补写（§6.6）；流局补偿不走本表 |
| `settlement_compensation` | `compensation_id, match_id, reason, issued(bool), created_at` | 流局补偿记录表：补偿记录先落库、`issued=false` 不发放；支付接口启用后才置 `issued=true` 并发放（§6.5/§9.5） |
| `settlement_event` | `settlement_id(主键), match_id, round_no, created_at` | 结算事件计数表：**在结算尝试发起时、进入 `settlementId` 事务之前单独插入**，以 `settlementId` 唯一（§6.6 对账第一条等式）；只追加，不更新、不删除 |
| `invite_code` | `code, created_by, created_at, used_by, used_at, expires_at, status` | 短信降级通道（§6.7）；8 位大写码唯一索引，一码一用，7 天有效，**无批次字段** |
| `order` | `order_id, player_id, sku, amount_cents, currency, status(created/paid/refunded), created_at` | **占位表**（U7）：v1 不产生数据，但表与接口先建 |
| `payment_callback` | `callback_id, order_id, channel, payload jsonb, verified, received_at` | 支付回调占位，验签字段预留 |
| `refund` | `refund_id, order_id, amount_cents, status, reason, created_at` | 退款占位 |
| `external_identity` | `player_id, provider, external_uid, verified_at` | 实名与第三方登录的适配位（v1 为空）；与 §9.8 的预留字段同名 |

Redis 的真实数据清单（原稿"只存两个指针"的表述作废）：

| 键 | 类型 | 用途 | 失效 |
| --- | --- | --- | --- |
| `game:{id}:active_ruleset` | string | 当前生效版本（缓存；唯一真相为 `ruleset_release` 表，服务启动与每次发布/回滚后从表重建，§9.5） | 持久 |
| `game:{id}:canary_ruleset` | string | 灰度版本（缓存；同一真相来源 `ruleset_release` 表，§9.5） | 发布流程结束时清理 |
| `room:{matchId}:snapshot` | string(json) | 房间快照（含区域内容与 `seqWatermark[]`） | 30 秒覆盖写，房间结束即删 |
| `room:{matchId}:actions` | stream | 动作日志（`ActionRecord`） | 房间结束后保留 90 天（与回放同周期） |
| `match:queue:{gameId}` | zset | 匹配队列 | 实时 |
| `ratelimit:{playerId}` | string | 动作频率计数 | 1 秒滑窗 |

### 5.9 牌桌协议（`crates/protocol`）

M0 要交付的就是这张表；服务端与 wasm 客户端共用同一份 proto 生成代码。

| 方向 | 消息 | 关键字段 | 语义 |
| --- | --- | --- | --- |
| C→S | `JoinRoom` | `matchId, seat?, clientVersion` | 入座或续座；`seat` 字段只用于续座（带回原座位）；入座结果一律经 `JoinAck` 返回。`clientVersion` 低于 `minClientVersion` 返回 `CLIENT_TOO_OLD`（定义见 §5.16） |
| S→C | `JoinAck` | `matchId, seat, accept, availableSeats[], reason?` | 入座响应帧：成功与 `RS_SEAT_TAKEN` 都走本帧。`accept=true` 时 `seat` 为服务端分配的座位；`accept=false` 时 `reason` 取 §5.7 错误码（`RS_SEAT_TAKEN`），`availableSeats[]` 为当前可分配座位号空表（房间已满时为空表） |
| C→S | `Reconnect` | `matchId, seat, lastSeatSeq` | `lastSeatSeq` 之后的增量由服务端补齐；该座位 `lastSeatSeq` 对应的 `roomSeq` 早于缓冲起点（换算按动作记录双序号列，见 §6.5）时只发 `RoomSnapshot` 并以快照 `roomSeq` 为新起点（§6.2） |
| C→S | `SubmitAction` | `matchId, seat, requestId, seatSeq, kind, payload` | `requestId` 为幂等键（§6.4）；`seatSeq` 为**每座位**单调递增；**服务端以 token 绑定的座位为准**，请求 `seat` 与绑定座位不一致返回 `AUTH_INVALID` 并记指标；`payload` 结构见下 |
| C→S | `Heartbeat` | `matchId, seat, clientSeq` | 客户端每 15 秒一次；服务端 30 秒内未收到该座位任何消息即判掉线（**唯一判定点**，§6.2 同步） |
| S→C | `RoomSnapshot` | `matchId, seat, rulesetId, rulesetVersion, roomSeq, lastSeatSeq, viewHash, viewSchemaVersion, view, snapshotReset` | 全量视图（按 `view.fields` 裁剪）。`rulesetId`/`rulesetVersion` 为房间创建时锁定的 RuleSet 身份与版本，与 `viewSchemaVersion` 组成客户端白名单缓存键 `(rulesetId, viewSchemaVersion)`（§9.2）。`view` 的类型是 **`bytes`**，内容是 canonical JSON，编码规则见 §5.12；`viewSchemaVersion: u16` 是这份 `view` 用的白名单版本（取自 RuleSet `view.schemaVersion`，房间创建时锁定）；**不下发 `stateHash`**；`snapshotReset: bool`，缺省 false，true 表示客户端必须丢弃本地状态并接受本快照（§6.2） |
| S→C | `RoomDelta` | `matchId, roomSeq, events[]` | 增量事件；断线期间存最近 200 条（§6.2）。`roomSeq` 是房间广播序；本消息是房间广播，**不带 `seatSeq`**，座位水位只在 `Reconnect`/`ActionAck` 按座位传递 |
| S→C | `ActionAck` | `requestId, seatSeq, roomSeq, accepted, reason?` | 回执；`reason` 为 **ASCII 字符串**，取值必须来自 §5.7 错误码**全表**（不在表内即服务端违约，P0 审计项）；`accepted=true` 时该字段不出现（第二十四次变更） |
| S→C | `PhaseChange` | `phase, deadlineMs` | 阶段切换（发牌/进贡/出牌/结算） |
| S→C | `SettleResult` | `roundNo, roomSeq, result, levelsAfter, nextDealInMs, reason?` | 结算广播；`result` 为 **`bytes`（canonical JSON）**，结构与 §7.x `round_record.result`（`settlement`）同构：`{winner_team, ranks, points}`，编码规则见下方「三类载体」；`levelsAfter` 是**每队**级牌（§5.13）；`roomSeq` 是本帧房间广播序；`reason` 为 ASCII 字符串、取自 §5.7 全表，仅在结算异常时出现。本帧不携带 `viewHash`，客户端不重算视图哈希（§5.12/§7.2）（第二十四次变更） |
| S→C | `TrusteeState` | `seat, mode(ai\|human), reason` | 托管状态变化 |

**三类协议载体的取值（第二十四次变更，2026-11-24 用户裁定 A，冻结）**

1. **`SettleResult.result`**：类型 `bytes`，内容为 canonical JSON，编码规则与 §5.12「视图编码」六条完全一致（键序按 `settlement` 结构固定为 `winner_team, ranks, points`；单行、无空白、不写 `null`；`ranks` 为按名次 1→n 的 `seat` 数组、`points` 为整数）。**允许随玩法扩展**：新增结算字段只能追加在 `points` 之后，不得改序、不得改名；proto 侧不设结构体，客户端按 `roundNo` 对应的 `rulesetVersion` 解析。
2. **`ActionAck.reason` / `SettleResult.reason`**：类型 `string`，**ASCII**，取值必须来自 §5.7 错误码全表；不在表内的字符串即服务端违约，按 P0 审计项处理（与「白名单外字段」同一处置口径）。`accepted=true` / 结算正常时该字段不出现（不写空串、不写 `null`）。
3. **`rulesetVersion`**（含 `RoomSnapshot.rulesetVersion`、`SettleResult` 间接引用与快照内同名字段）：类型 `string`，字符集 `[A-Za-z0-9._-]`、长度 **1–32**；只作**身份键**，**禁止按字符串字典序比较大小**（版本大小语义不适用），仅在 `(engineVersion, interpreterVersion, rulesetVersion)` 三元组**相等性**判定里使用（§5.12/§10.4）。

**`SubmitAction.payload` 按 `kind` 分型**（proto 用 `oneof`，每型独立消息）：

| `kind` | 字段 | 说明 |
| --- | --- | --- |
| `play` | `cards: u16[]`、`patternHint: string?` | 只传牌 id；`patternId` **由服务端判定**，`patternHint` 仅用于对不合法提示给理由，不参与判定 |
| `pass` | 无 | 空结构体，禁止带字段 |

`tribute`/`return` 不是玩家提交的动作：贡牌与还贡牌由引擎按策略自动选定，选定结果经 `TributeEvent{from, to, card}` 回传（§5.2/§5.13），贡方与受贡方均不提交牌，`SubmitAction` 不接受这两个 `kind`。

**`RoomDelta.events[]` 分型**：`PlayedEvent{seat, cards, patternId}`、`PassedEvent{seat}`、`TributeEvent{from, to, card}`、`LevelChangedEvent{team, from, to}`、`TurnChangedEvent{seat, deadlineMs}`、`RoundSettledEvent{roundNo}`。新增事件类型只允许加 `oneof` 分支（§9.2）。

约定：`stateHash` 永不下发（§5.12）；`viewHash` 由服务端在每次下发前自算并自证，**客户端不重算、不上报视图哈希**（§7.2 的 P0 只保留服务端重算视图与已下发值不一致；`ViewMismatchReport` 帧不进 v1 协议，调试构建如需本地比对由客户端私有实现，不进线上链路）；`view` 的字段集合由 §5.1 `view.fields` 决定、编码由 §5.12「视图编码」固定为 canonical JSON bytes，服务端不得下发白名单外的字段；`viewSchemaVersion` 与 `view` 同帧下发，客户端不得跳过它自行推断白名单。

**本地存档契约（按模式分流；§2.4 第 16 周 gate 与离线恢复的载体依据）**

两种模式各管一行，不并存两套口径：

- **联机局（含服务端 bot 房）**：机器崩溃、进程被杀一律按网络断线处理，凭服务端快照重连（§6.2 的重连与 `snapshotReset` 口径）；**客户端不写本地存档、不承诺离线续局**。
- **纯本地 bot 局**（wasm 引擎跑在客户端、无服务端判局）：**必须**本地存档，最小契约写清五件事：
  1. 载体：浏览器用 IndexedDB，桌面壳用本地文件；按 `localMatchId` 隔离，只存本地、不上云。
  2. 内容：`{localMatchId, rulesetVersion, engineVersion, interpreterVersion, seed, snapSeq, stateHash 或 viewHash, snapshot 字节, actions 尾段}`。
  3. 写入时机：每 N 个 tick 与每次动作后写；写入用原子替换（先写临时键/临时文件，成功后再改名或提交），避免半写。
  4. 恢复：重开时校验 `rulesetVersion` / `engineVersion` / `interpreterVersion` 三个版本号，并重放 `actions` 尾段重算哈希、与存档内 `stateHash` 或 `viewHash` 比对；三者一致才续局；版本不一致、哈希不一致或无存档，一律判"重开新局"并提示玩家。
  5. 上限与清理：单局存档设容量上限（具体值实现阶段按实测快照体积定，本节不写死），且**只保留最近 1 局**，新局开始时清理旧存档。

纯本地 bot 局的存档不上云、不计战绩（§6.6 三档可信度）。

### 5.10 掼蛋规则取值表（U10）

这是"规则本体"的可执行版本：左列是 RuleSet 字段，右列是取值与来源。本表 15 行取值均已确定，不再有待确认项。**其中 7 项（同花顺位置、进贡配对、还贡、抗贡、升级档、过 A、接风）已于 2026-11-23 由用户逐条确认，随本表冻结进 `guandan-classic` v3；此后任何改动只能走 RuleSet 新版本，不得改已发布版本。**

| 规则项 | 字段 | 取值 | 状态 |
| --- | --- | --- | --- |
| 用牌 | `deck` | 2 副共 108 张（每副 52 张 + 2 王，`jokers: 2`）；合计大王 2、小王 2 | 定 |
| 级牌 | `levelTable.initial` + 房间 `levels` | 首局两队都是 2；级牌在**散牌型**（单张/对/三张/三带二主牌）里牌力高于 A、低于小王，在**组牌型**（顺子/三连对/钢板/同花顺）里只按**自身自然点数位**参与、不额外加权（第二十五次变更）；**每队一个 `levels`**，但**每局只有打级队那一份参与牌力序合成**（`levels[attackingTeam]`，§5.13「决定首出」行、第十六次变更） | 定 |
| 逢人配 | `wildcard.source` | 红桃级牌（`heart-@level`），每副 1 张共 2 张 | 定 |
| 逢人配能否组同花顺 | `wildcard.inStraightFlush` | `true`（C2 争议项；2026-11-24 用户裁定可组，第十五次变更由 `false` 改 `true`） | 定 |
| 炸弹序 | `compare.bombOrder` | `count-then-rank`：4 张 < 5 张 < … < 8 张；同张数比点数 | 定 |
| 同花顺位置 | `compare.flushRank` | `above-bomb-5`（5 张炸之上、6 张炸之下），完整阶梯：4 张炸 < 5 张炸 < 同花顺 < 6 张 < 7 张 < 8 张 < 四王炸；其余三值（`below-bomb-5` / `above-bomb-6` / `above-bomb-8`）作为其他 RuleSet 的可选值保留 | 定 |
| 四王炸 | `pattern.joker-bomb` | 最高，压所有炸弹与同花顺 | 定 |
| 单张/对/三/三带二/顺子/三连对/二连三 | `patterns` | 顺子固定 5 张、`plate-3` 为 3 连对（6 张）、`steel-2` 为 2 组三张连（6 张），**组牌型（顺子/三连对/钢板）一律最长 6 张**，不存在 5 张连对、4 组钢板这类超长写法；**三带二 = 3 张同点 + 1 对（`triple-pair`），大小只看主牌那三张的点数、与所带的对子无关**（2026-11-24 用户裁定，第十八次变更） | 定 |
| 进贡配对 | `tribute.pairs[]` | 单贡：末游→头游；双下（对位进贡）：末游（第 4 名）→头游（第 1 名）、三游（第 3 名）→二游（第 2 名），一对一映射，不交叉 | 定 |
| 还贡 | `tribute.return`（`ceiling`/`count`/`fallback` 三键） | 受贡方还一张不大于 10 的牌；全大于 10 时还其中最小的一张 | 定 |
| 抗贡 | `tribute.resist` | 贡方两家合计持有 2 张大王即可抗贡（不要求在同一人手里），本局不进贡 | 定 |
| 升级档 | `levelTable.upgrade` | 双下 +3、1+3 +2、1+4 +1（按队结算） | 定 |
| 过 A | `level.pass` | 打 A 那一局，本方名次组合为 `1+2` 或 `1+3`（本方拿到头游、且末游是对家）即算过 A；`1+4`（本方头游但队友末游）不算，其余组合（本方未拿头游）也不算；未过 A 不退档，该队 `levels` 保持 A 继续在 A 打下一局，同一队连续 3 局在 A 上未过才退回 `levelTable.initial`（起点 2）并把该队 `aFailStreak` 清零；跨 A 的超额升级数作废 | 定 |
| 接风 | `flow.turn.partnerLead`（顶层 `flow` 键，默认 `true`） | **口径（第二十六次变更订正）**：某座位打出**最后一手牌**后，所有**仍有牌**的座位都选择「过」，牌权不得回到已出完的人，改由其**搭档**直接领出；期间只要有一家出牌压过，接风即不成立，按正常出牌顺序继续。判定主体是**出完牌的本人**，不是「其队友是最后压牌者」；若搭档也已出完，则由沿出牌方向的下一个**仍有牌**的座位领出（防御分支，按「同队全员出完即局终」通常不可达）。`turn.return-to-last` 只管「全过后回到最后出牌者」，不负责接风 | 定 |
| 托管出牌 | `ai.hint` + 托管策略 | 固定"出最小合法牌"，排序键 = 牌力升序、同牌力按牌 id 升序 | 定 |

确认结果的差异：同花顺位置、进贡配对、抗贡三项按拍板结果实改（`above-bomb-5`、对位进贡、贡方两家合计两张大王），过 A 按用户口径放宽（`1+2` 或 `1+3` 即算过 A、`1+4` 不算），还贡、升级档、接风维持原值。以上取值即 `guandan-classic` v3 的生效口径，与 §5.13 的逐条规格一致。

### 5.11 AI 特征与档位

| 档位 | 取值 | 实现层策略 | 特征集合 | 用途 |
| --- | --- | --- | --- | --- |
| 简单 | `easy` | AI-L0（规则评估，无搜索） | `hand.count`, `last.play` | 只做合法性与最小牌，随机扰动 30% |
| 普通 | `normal` | AI-L1（评估 + 有限扫描） | 上列 + `partner.state`, `opponent.count` | 保留大牌、配合队友 |
| 困难 | `hard` | AI-L2（AI-L1 加有限深度搜索） | 上列 + `table.history`（近 20 手）, `bomb.count` | 记牌与炸弹时机 |

`ai.features` 的合法取值就是上表"特征集合"里出现过的字符串，未知取值在 L1 拒绝。

**组牌推荐（`recommend`，第二十次变更，2026-11-24 用户裁定）**

用途：服务端「一键推荐组牌」与「是否有同花顺」提示。用户可自由组牌，推荐只是建议，**不影响判定**。

- **接口**：`recommend(&State, Seat) -> Result<Layout, EngineError>`。纯函数、只读、确定性（同输入同输出），
  **不进 `stateHash`、不改 `legal_actions`、不进双端比对的判定链**；native 与 wasm 必须同解，CI 单列一条比对。
- **`Layout` 形状**：`{groups: [{patternId: String, patternKey: (u32,u32), cards: [u16]}], score: i64, singleCount: u8,
  straightFlushCandidates: [[u16]], hasStraightFlush: bool, disabled: bool}`；`groups` 按 `(patternId 字典序升, patternKey 降, cards 升)` 排序；
  `straightFlushCandidates` 是当前手牌里所有可组同花顺的候选（含百搭方案，百搭代牌规则同 §5.13），按 `cards` 升序去重；
  它**独立于最优拆分**——最优拆分里没走同花顺也要照样列出候选，`hasStraightFlush` 即候选非空。
- **评分公式（全整数，禁止浮点）**：`score = Σ groupScore`，其中
  - 炸弹：`bomb4..bomb8` 各取对应权重；王炸取 `jokerBomb`；
  - 同花顺：`straightFlush`；
  - 顺子：`straightBase + rankBonus(straightRankBonus)`；三带二：`triplePairBase + rankBonus(triplePairRankBonus)`；
    三连对：`plate3Base + rankBonus(plate3RankBonus)`；钢板：`steel2Base + rankBonus(steel2RankBonus)`；
  - 对子：`pair`；单张：每张 `single`（负值即扣分）。
  - `rankBonus(bonus)` = `⌊ bonus × (rankKey − 1) / (N − 1) ⌋`，`N` = 当前合成牌力序里**非王牌点数的个数**（掼蛋 13），
    `rankKey` 为该牌组原语给出的点数权重；轮子顺的 `rankKey = 0`（§5.13）按 0 分处理，即不拿加分。
- **权重刻度（第二十八次变更，2026-11-24 用户裁定 A）**：`weights` 一律按**刻度 10** 书写——即表内数字是"基准分 × 10"，
  目的是让相邻两档之间留出可插入的整数位（1 分一档时 `bomb5=5` 与 `bomb6=6` 之间无整数，无法表达"同花顺介于 5 炸与 6 炸之间"）。
  刻度只决定可表达精度，不改变任何相对次序；`weights` 的每个值仍是**整数 `i16`、禁浮点**，引擎按原值直接比较、不再换算。
- **默认权重（省略 `weights` 时即此表；刻度 10，数值为放大后的原始值）**：
  `bomb4=40, bomb5=50, bomb6=60, bomb7=70, bomb8=80, jokerBomb=90, straightFlush=55,
  straightBase=40, straightRankBonus=20, triplePairBase=30, triplePairRankBonus=20,
  plate3Base=30, plate3RankBonus=10, steel2Base=40, steel2RankBonus=10, pair=0, single=−10`。
  **同花顺严格落在 5 炸与 6 炸之间**（`50 < 55 < 60`），与真实牌力序（`bomb5 < straight-flush-5 < bomb6`）一致，不再与任何牌型同分。
  其余相对关系与第二十次变更的默认表逐项等价（炸弹 4→8 档、王炸最高、顺子/三带二/三连对/钢板的位次加分按 `rankKey` 递增、对子 0、单张扣分）。
  说明：`rankBonus(bonus) = ⌊ bonus × (rankKey − 1) / (N − 1) ⌋` 中的 `bonus` 同样按刻度 10 书写（`20`/`20`/`10`/`10`），
  整除下取整的损失比 1 分一档更小；轮子顺 `rankKey = 0` 仍按 0 分处理（不加分）。
- **tie-break（固定、双端必须同序）**：`score` 高者胜 → `singleCount` 少者胜 → 炸弹组（含王炸、同花顺）总张数多者胜 →
  `groups` 序列按 `(ladder 位次降, patternKey 降, cards 升)` 字典序小者胜。仍相等则取枚举顺序最先者，**不存在随机**。
- **搜索策略**：精确最优是指数级，采用**确定性束搜索**——`beamWidth = 64`（固定常数，写死在引擎、不进 RuleSet），
  候选组按 `(patternId 字典序, patternKey 降, cards 升)` 枚举，逐层贪心扩展 + 剪枝（同 `cards` 集合的不同拆法只保留 tie-break 最优的一条）。
  同输入必同输出；束宽变更视为 `recommend` 口径变更（需重录推荐黄金向量，但**不影响 `stateHash`**）。
- **预算**：`guandan-classic` 满手 27 张单次 ≤ 2ms（P99）；超预算记 `recommend_cpu_overrun_total`（P2），
  **不判错、不影响合法性、不作回滚条件**（与 §6.7 的 CPU 口径同一条原则：机器快慢不改变规则结论）。
- **L1 校验**：`weights` 未知键拒绝、类型非整数拒绝、值域 `i16`；**默认表按刻度 10 书写**，作者自定义权重时必须显式给出全部键（不允许只写一半、其余隐式按默认），`straightFlush` 与炸弹档之间不做强弱校验（权重是启发式排序分，不强制等于牌力序）；`enabled=false` 时 `weights` 可整段省略；
  `ai.recommend` 整段省略 = 默认权重全上。

### 5.12 状态摘要：`stateHash` 与 `viewHash`

三个哈希各管一件事，混用会让"双端一致"这条判据失效：

| 名字 | 输入 | 谁算 | 用途 |
| --- | --- | --- | --- |
| `stateHash` | 全量状态（含所有座位手牌） | 服务端、CI 的 native/wasm 双跑 | 恢复校验、漂移检测、双端一致门禁。**永不发给客户端** |
| `viewHash(seat)` | `view(seat)` 投影 | **只由服务端在每次下发前算并自证**；客户端不重算、不上报 | 视图一致性 P0 指标：服务端重算视图与已下发值不一致即告警（§7.2） |

**纳入 `stateHash` 的字段**（第 2–11 项共 10 段按下列固定顺序拼接，不按 map 迭代顺序；第 1 项是旁挂版本字段，不进拼接）：

1. 版本旁挂字段（**不进哈希拼接**）：`engineVersion`、`interpreterVersion`、`rulesetVersion` 与哈希并列存放；比对时先比版本三元组，版本一致再比哈希（跨版本用 `(版本, 哈希)` 二元组判定，§9.2）
2. `phase`（枚举序号 u16）、`turnSeat`（u8）、`levels`（**每队一个 u8**，按队伍序号升序，存 `levelTable.ranks` 下标；§5.13）
3. `seats`（按座位号升序）：`seat(u8)`；**手牌不在本段重复计入**，手牌只在第 4 项的私有区进哈希一次
4. `zones`（按 `(seat u8, zoneId u16)` 升序）：`seat`、`zoneId`、`cards`（牌 id 升序）。private 区按 `(seat, zoneId)` 实例化；u16 区号在 RuleSet 解析期按声明序分配并冻结；同一批手牌只在本段进哈希一次，`seats[].hand` 只是视图字段、不进哈希
5. `lastPlay`：`seat(u8)`、`patternId(UTF-8)`、`patternKey`（`(countKey, rankKey)` 两个 u32）、`cards`（牌 id 升序）
6. `seqWatermark`（每座位 u64，按座位号升序）
7. `tribute`（`fromSeat`、`toSeat`、`card`，进贡阶段结束后归零；抗贡局与场次首局整段跳过，取占位值 `fromSeat=0`、`toSeat=0`、`card` 为空）
8. `rng`：`counter(u64)`、`seedDigest(前 8 字节)`；`seedDigest` = 对 `seed`（u64 定长小端编码）用 BLAKE3-256 取前 8 字节，快照内同时保存 `seed` 原值，摘要由它派生；`counter` 的递增点与随机字生成算法见 §5.2「发牌随机源的确定性定义」（拒绝采样时被丢弃的那次也计数，双端必须同值）
9. `aFailStreak`（**每队一个 u8**，按队伍序号升序，取值 **0–3**；某局结算时若该值达到 3，则本队 `levels` 退回 `levelTable.initial`（起点 2）并把该值清零；过 A 即该场结束、计数无意义；两端必须同值；§5.13）
10. `finishOrder`（按出完先后升序的 `seat(u8)` 序列；结束时未出完的座位不列入；无出完记录时为空序列；与 §5.13「局终止与名次」同一口径，重放与崩溃恢复据此重建 `rank`）
11. `attackingTeam`（**单个 `u8`**，本局「打级队」的队序号；决定本局牌力序取哪一份 `levels`，须可重建，故进哈希；场次首局由抽牌定、此后为上一局头游所在队；§5.13「决定首出」行、§12.2 第十六次变更）

**`viewHash(seat)` 定义**（服务端自证口径）：

- 纳入字段：`view` 字段的编码字节（即本节「视图编码」的 canonical JSON bytes）+ `seat(u8)` + `roomSeq(u64)`（后两者按 little-endian 定宽追加在字节流尾部）；`levels` 是否在内取决于白名单；`clientPlatform` **必须排除**（纳入则两端值天然不同）。
- 哈希函数与截断：BLAKE3-256 取前 8 字节（16 个十六进制字符），与 `stateHash` 同一函数、同一截断长度。

**视图编码（`RoomSnapshot.view` 与所有下发视图的唯一编码）**：`view` 是 `bytes`，内容为 canonical JSON，规则六条固定——① 顶层是对象，**键序 = `view.fields` 白名单序**（不是字母序、不是 map 迭代序）；② 单行、无空白、无缩进；③ 键名与枚举字面值只用 ASCII，键名不得重复；④ 数组元素定序：牌 id 升序、`seat` 升序；⑤ 字符串 UTF-8、不做 NFC 归一化；⑥ 不写 `null`——字段缺值直接不出现，出现白名单外的键即视为服务端违约（P0，服务端重算视图即可定位）。`viewSchemaVersion` 随每次下发一起给，客户端按它选白名单并逐字段解码，因此**动态字段集不需要 proto 静态 schema**，`crates/protocol` 只声明 `bytes` 与 `u16` 两个字段。

**牌 id 编码**：牌 id 为 u16，在 RuleSet 载入期一次性分配并冻结，运行期不变。编码 = deck 序 × 副本号：先按 `suits` 声明序（`S/H/D/C`）× `ranks` 自然序排基础牌序，同一基础牌的第 `k` 张副本（`k` 从 0 起）占相邻 id；大小王按 `little-joker`/`big-joker` 排在序尾。双端必须按同一载入期结果取 id，不允许运行期按牌面重新编号。

**字段级编码表**（逐白名单键定 JSON 值类型、缺值行为、数组定序；本表内容即 `viewSchemaVersion` 锁定的白名单形状，白名单集合或顺序变化必须 +1，§5.1）：

| 白名单键 | JSON 值类型 | 缺值行为 | 数组 / 元素定序 |
| --- | --- | --- | --- |
| `<zoneId>`（private / shared 区） | 对象 `{"cards": <cardId[]>}`；private 区只含本座位手牌 | 区为空时 `cards: []`，键不省略 | `cards` 按牌 id 升序 |
| `seat.levels` | `u8[]`（元素为 `levelTable.ranks` 的下标，与 `stateHash` 第 2 项同口径、不写牌面键），按队伍序号升序 | 无队伍级牌状态时该键不出现 | 队伍序号升序 |
| `lastPlay` | 对象 `{"seat": u8, "patternId": string, "cards": <cardId[]>}` | 本回合无人出过牌时该键不出现 | `cards` 按牌 id 升序；`seat` 单值不定序 |
| `round.level` | `u8`：本局**局级牌**的档位下标（= `levels[attackingTeam]`，口径见 §5.13「决定首出」行） | `settle.algorithm = score-only` 或无级牌状态时该键不出现 | 单值不定序 |
| `round.attackingTeam` | `u8`：本局打级队的队序号 | 同上 | 单值不定序 |

**定值向量（双端比对锚点）**：覆盖上表全部白名单字段的一条固定视图，canonical bytes 如下（单行、无空白，键序 = 白名单序 `hand, seat.levels, discard, lastPlay, round.level, round.attackingTeam`）：

```
{"hand":{"cards":[0,1,2]},"seat.levels":[2,2],"discard":{"cards":[10,11]},"lastPlay":{"seat":1,"patternId":"single","cards":[5]},"round.level":2,"round.attackingTeam":0}
```

- canonical bytes 长度：169 字节（UTF-8；第二十二次变更追加 `round.level`/`round.attackingTeam` 后由 129 字节增至此）。
- SHA-256 指纹（对上述 canonical bytes 真算，双端逐字节比对锚点）：`36e7bdc6d3ea4bba20ec571f3034c2663ec253909c56b4d649a2713c6aeeab51`（旧值 `9e0b756d…cc72` 随白名单升级作废）。
- 该向量对应的 `viewHash` 仍按本节定义由 BLAKE3-256 从同一 canonical bytes 追加 `seat`/`roomSeq` 定宽尾段推导；改动白名单形状后必须换新向量并重算指纹。

**明确排除**：任何时间戳、连接/在线状态、托管标记（它只影响 UI 与超时策略，不影响合法动作）、日志与审计、客户端版本、昵称头像。

**规范化与序列化**：

- 用 `crates/engine` 里的 `CanonicalState` 固定结构体序列化，**禁止 `serde_json::Value` 与 `HashMap` 参与**（§3.3 约束 2）。
- 字节序固定 little-endian；整数固定宽度（`u8`/`u16`/`u32`/`u64`/`i16`）；字符串 UTF-8 且不做 NFC 归一化（只允许 ASCII 枚举与版本串）。**唯一例外是本节「视图编码」的 canonical JSON**：JSON 里的整数按十进制、无前导零、无正号书写，定宽小端只用于追加在视图字节流尾部的 `seat` 与 `roomSeq`。
- **`State` 与 `CanonicalState` 的关系**：`State` 直接承载本节第 2–11 项这十段字段（与 `CanonicalState` 同一字段集、同一类型），`CanonicalState` 是 `From<&State>` 的**单向只读投影**，只承担序列化与双端比对，不另行持有真源；禁止两处各存一份——转写漂移会让双端 `stateHash` 不一致（§12.2 第十一次变更第 ⑧ 条）。
- **变长序列的长度前缀宽度**（双端必须同一宽度，否则逐字节必红）：段内**元素个数**一律 `u8` 前缀（`levels`、`seats`、`zones`、`seqWatermark`、`finishOrder`，以及 `tribute` 抗贡/首局占位时的空序列）；**牌 id 数组**与**字符串**（`patternId`）一律 `u16` 前缀（小端，先计数后元素），`cards` 即「u16 个数 + 各 u16 牌 id」。宽度属编码定义的一部分，改动即视为 `stateHash` 口径变更（§12.2 第十一次变更第 ⑦ 条）。
- 哈希函数固定 **BLAKE3-256**（32 字节输出）；对外呈现固定取**前 8 字节**，即 16 个十六进制字符。协议字段、日志字段统一是这个 16 字符的定长串，不得出现"16 字节"的表述。
- 计算位置：每次 `apply` 成功后由服务端算一次并留在内存态（写快照与审计）；CI 里 native 与 wasm 各算一次后按 **32 字节全量**逐字节比对（不是比 16 字符的呈现串）。

**`legal_actions` 的排序键**（双端必须同序，否则比对必红）：

`(countKey u32 升, rankKey u32 升, cards 牌 id序列 升, primitiveId 升)`。`countKey` 是该牌型的张数权重、`rankKey` 是点数权重，二者由原语按 `compare.bombOrder` 与载入合成的牌力序（`naturalOrder` + `levelInsert`）给出（`patternKey` 就是这一对 `(countKey, rankKey)`）；四元组全序，重复元素不可能出现。若某原语给不出这一对，L1 判该原语不可用于 `legal_actions`。

**`legal_actions` 只列规范子集（第十九次变更，2026-11-24 用户裁定）**：列的是每种牌型的**规范代表**，不是全部物理副本组合——同一 `patternId` 与同一 `patternKey` 只出现一条。规范代表的选定顺序：①在可达解中取 `rankKey` 最大者（与 §5.13 逢人配一致）；②`rankKey` 相同再取**百搭用量最少**者；③仍相同取**真牌牌 id 升序字典序最小**者。客户端可以用任何在结构上等价的真牌／百搭组合出牌，`apply` 按结构复检、**不受规范子集约束**；因而跨端只保证「每种牌型至少给出一个可直接提交的候选」，不保证「候选覆盖所有物理组合」。全组合枚举的代价（8 张同点数的对子 28 条、四张炸 70 条）由 §5.7 的 4096 峰值闸门与 §10.5 基准共同约束，不做全列。**组牌推荐**（一键推荐整手拆分、提示同花顺）是另一条能力，不与 `legal_actions` 共用口径；其规格待下一条变更落稿。

### 5.13 掼蛋规则规格（按 phase 逐条可判定）

本节是 `guandan-classic` 的规则本体，与 §5.10 的取值表一一对应；两者冲突时以本节为准并把 §5.10 改正。

**deal（发牌）**

| 规则 | 判定写法 |
| --- | --- |
| 牌堆 | 2 副 108 张：`2×4` 花色 × `2..A` 各 2 张 + 4 张王（每副 `jokers: 2` = 1 大王 + 1 小王，两副合计大王 2、小王 2） |
| 发牌 | 每人 27 张，RNG 唯一来源（`rng.counter` 递增），发完剩余 0 张；随机序按 §5.2「发牌随机源的确定性定义」定死——`BLAKE3-256(seed‖stream‖counter)` 取前 8 字节经 `bounded` 拒绝采样，做 **Fisher-Yates 降序**洗 `[0,108)` 牌 id 序，再按座位序号升序、按 `perSeat` 连续切分（座位 0 拿前 27 张） |
| 决定首出 | **每局单一「打级队」口径（第十六次变更，2026-11-24 用户裁定）**：每局只有一份**局级牌**，取 `levels[attackingTeam]`，牌力序按这一份合成（`naturalOrder` + `levelInsert`），全场统一、可传递；`attackingTeam` 在每局 `settle` 结束时置为**头游所在队**（"谁赢打谁的级牌"），`levels` 仍每队各存（用于各自升档与记账），但不再每队各合成一套 ladder。**场次首局**：抽牌**前置**——在 `deal.shuffle` 之前先取一次 `bounded(总张数)` 抽出一张牌（牌不剔出牌堆），洗牌、按座位序连续切分发牌之后，看这张牌落到哪个座位，该座位**先出**、其所属队即 `attackingTeam`（此时两队 `levels` 同为 `levelTable.initial`，故牌力序不存在歧义）；非首局不抽牌、不额外消耗随机字；**此后各局**由上一局头游先出。 |

**tribute（进贡/还贡/抗贡）**

**必填性**：`tribute` 段在掼蛋变体中必填（`enabled: true`），整段缺省即 L1 `RS_SCHEMA_INVALID`；`enabled: false` 时其余子键可整段省略、只写 `enabled: false` 即合法。本节只写规则本体，字段必填性与取值的承载位一律以 §5.1 `tribute` 行为准（2026-11-24 按第十三次变更补录，统一此前本节未重申必填性、实现侧按"整段可省"落地的双口径）。

| 规则 | 判定写法 |
| --- | --- |
| 谁进贡 | 上局末游向头游进贡 1 张；上局双下（三、四名同队）时对位进贡：末游（第 4 名）→头游（第 1 名）、三游（第 3 名）→二游（第 2 名），各 1 张，一对一映射，不交叉 |
| 贡牌选择 | 由引擎按"除红桃级牌外点数最大、同点数取牌 id 最小"**自动选定**，贡方没有选择权（避免"必须最大"与"自选"两规则互斥）；选定结果经 `TributeEvent{from, to, card}` 广播；进贡/还贡不由玩家提交动作（§5.9），贡方与受贡方均无 payload |
| 抗贡 | 贡方两人合计持 2 张大王（单贡时贡方自己持 2 张大王）即可抗贡；抗贡为两家合计一次性判定，双下不存在只免一家的部分抗贡。抗贡成立则**进贡与还贡均不发生**，整段 `tribute` 跳过：不进入进贡动作序列、不发 `TributeEvent`，`stateHash` 第 7 项取占位值（`fromSeat=toSeat=0`、`card` 为空），相位由 `deal` 直接转 `play`（§6.3 转移表） |
| 还贡 | 按 `tribute.return` 三键 `ceiling`/`count`/`fallback` 选定：`ceiling: under-ten` 还 1 张**不大于 10** 的牌；若手牌全大于 10，按 `fallback: smallest` 还其中最小的一张。该兜底分支由 **L3 随机对局的种子用例**覆盖，不在 L1 覆盖（L1 只做结构校验） |
| 顺序 | `tribute` 是独立 phase，与 `play` 分离；进贡完成后 `stateHash` 里 `tribute` 段归零 |

**play（出牌）**

| 规则 | 判定写法 |
| --- | --- |
| 牌型 | `single`/`pair`/`triple`/`triple-pair`（三带二）/`straight-5`/`plate-3`（3 连对）/`steel-2`（2 组三张连）/`bomb-4..8`/`straight-flush-5`/`joker-bomb`，**id 与 §5.1 示例的 `patterns[].id` 完全一致**；同一手牌若同时能判成多个牌型（含百搭多解），按 `compare.ladder` 位次取**最高**的那一种作为识别结果（`plate-3` 与 `steel-2` 是唯一写法，不出现"木板/钢板"别名） |
| 三带二 | 结构 = 3 张同点 + 1 对，共 5 张，id `triple-pair`（**固定形状**，由 `pattern.triple-pair` 承担，不参数化）；**大小只看主牌那三张的点数，与所带的对子无关**：`countKey = 5`、`rankKey` = 主牌点数在载入期合成牌力序（`naturalOrder` + `levelInsert`）中的位置，级牌做主牌时同样按「高于 A、低于小王」；带的对子不进 `rankKey`，带哪一对都不改强弱；带的对子可以是任意同点数两张（双副牌下大小王各 2 张，成对亦可），不得是两张不同点数；与其它非炸弹非插入型牌型**互不可压**（只有炸弹与同花顺按阶梯可压它）；`ladder` 位次在 `triple` 之后、`straight-5` 之前，仅用于识别消歧与展示；百搭可代主牌三张或对子中的任一张，取解仍按逢人配那条（`rankKey` 最大、再真牌字典序最小） |
| 顺子边界 | 顺子取 5 张连续自然点数，允许从 A 起（`A-2-3-4-5`）但不允许绕回（`Q-K-A-2-3` 非法）；相邻判定按 `naturalOrder` 线性比较，唯一环形特例：A 可前接 2（A 在首位组成 `A-2-3-4-5`），A 后接 2 非法，双端按同一顺序判定；级牌在顺子里按自然点数参与，不按 `@level` 的高位（第二十五次变更追认：组牌型里级牌**只有自然点数位一种解释**）；大小王不得入顺；逢人配代牌后仍须整体连续。**轮子特例（第十七次变更，2026-11-24 用户裁定）**：`straight-5`（**非同花**杂顺）的结构为 `A-2-3-4-5` 时——不论这一手是否含级牌、是否由百搭补位而成——其 `rankKey` **恒取保留值 `0`**，在杂顺内永远最小，**不受 `levelInsert` 提升**（否则级牌恰为 5 时轮子会被抬成强顺，与"轮子最小"的直觉冲突）；`0` 为保留值，牌力序里的任何真实点数不得映射到 `0`，其它牌型也不得取 `0`。`straight-flush-5` 的 `A-2-3-4-5` **不适用**本特例：同花顺族内比较仍按 `naturalOrder` + `levelInsert` 的顶牌取 `rankKey`，其与炸弹的相对位次只由 `compare.flushRank` 决定 |
| 级牌 | 当前 `levels[team]` 对应的牌在**单张/对/三张/三带二的主牌**里按"高于 A、低于小王"的牌力参与比较；作为炸弹时仍按自身点数。**组牌型（顺子/三连对/钢板/同花顺）里的级牌只按自身自然点数位参与**（级牌 = 2 时，2 就是自然序的最小位；**不存在「级牌当 A」的第二解**，也不因是级牌而额外加权）；组牌型的张数上限为 6 张（顺子固定 5 张、`plate-3` 与 `steel-2` 固定 6 张），超出即按 `IllegalAction` 拒（第二十五次变更）。牌力序由 `naturalOrder` + `levelInsert` 在 RuleSet 载入期合成：先取 `naturalOrder` 自然序，再按 `levelInsert` 把级牌插到指定位置（`above-A` 即 A 之后、小王之前）；合成结果在任一 `levels` 下唯一，不允许占位符参与运行时排序 |
| 逢人配 | 红桃级牌（`wildcard.source`）可替代除大小王外任意一张牌，也可按自身点数（该队当前级牌）使用；**一手内百搭张数不限**（含整手全为百搭的情形）；含百搭时按「可达解中 `rankKey` 最大者」取解，`rankKey` 相同再由「真牌点数组合按 `naturalOrder` 字典序最小」定唯一解，两端按同一顺序判定；顺子多孔多解同按此条取「最大且唯一」解；`wildcard.inStraightFlush=true` 时可参与同花顺、`false` 时不得参与（本基线取 `true`，见 §5.10 该行与第十五次变更）；**整手全百搭的牌型由此闭合（第二十一次变更，2026-11-24 用户裁定）**：当一手提交的牌**全部是百搭**时，一律按百搭自身的点数（= 当前打级队的级牌）取同一 `rankKey`，**张数直接决定牌型**——1 张 = `single`、2 张 = `pair`、3 张 = `triple`、4–8 张 = 对应的 `bomb-N`；**不得判成顺子、三连对、钢板、三带二或同花顺**（全为同一 `rankKey`，既不可能连续、也不可能有异花色）。该条与 `deck.copies` 无关，多副牌（`copies > 2`）下同样按此判定，不另设上限；张数若超出牌型表已声明的 `pattern.same-rank`/`pattern.bomb` 值域（同一手 > 8 张，或 RuleSet 未声明该张数的炸弹），按 `IllegalAction` 拒绝，不降级、不猜牌型。百搭不得替代大小王，`joker-bomb` 不接受百搭；`triple-pair` 中百搭可代主牌或对子任一张，取解同样按本条 |
| 比较 | 同牌型比 `patternKey = (countKey, rankKey)`（张数优先、再点数）；炸弹按 `bombOrder` 两级比较（本版 `count-then-rank`）：张数多者大；张数相同比点数，点数按载入期合成牌力序（`naturalOrder` + `levelInsert`）比较、级牌位高于 A——故 4 张级牌 > 4 张 A > 4 张 K，但 5 张任何牌 > 4 张级牌（§5.10 炸弹序行、§5.1 `compare` 行）；`straight-flush` 与 `bomb` 的相对位置由 `compare.flushRank` 决定（§5.1）；本版 `flushRank=above-bomb-5`，最终压制阶梯 `bomb-4` < `bomb-5` < `straight-flush-5` < `bomb-6` < `bomb-7` < `bomb-8` < `joker-bomb` |
| 过牌 | 无牌可压或主动不要，`pass` 记入事件；三家连续 `pass` 后由最后出牌者重新领出 |
| 接风 | 含义固定为：某座位打出**最后一手牌**后，所有**仍有牌**的座位都「过」，牌权不得回到已出完的人，改由其**搭档**直接「领出」；期间有一家压过则接风不成立、按正常顺序继续。判定主体是**出完牌的本人**（第二十六次变更订正）。搭档亦已出完时，由沿出牌方向的下一个仍有牌座位领出。`turn.return-to-last` **不负责接风**，接风由 `flow.turn.partnerLead` 实现 |
| 全过后领出 | 「全过之后谁领出」的判定顺序唯一（第二十六次变更）：①先判接风——最后出牌者**已出完**且 `flow.turn.partnerLead = true` → 其搭档领出（搭档亦已出完则沿方向顺延到下一个仍有牌座位）；②接风不成立时按 `flow.turn.afterAllPass`：`last-player`（默认）→ 回最后出牌者，`next-player`（可选玩法）→ 从最后出牌者起沿方向的下一个**仍有牌**座位；③`turn.return-to-last` 与 `afterAllPass: last-player` 同义，两者共存**不视为配置冲突**、L1 不报错，显式取值优先于原语默认 |

**settle（结算与升级）**

掼蛋按**队**升级，所以级牌是"每队一个"（`levels: [u8; teams]`），不是全场单值。

**局终止与名次**：一局在**某一队全部队员均已出完牌**时结束（按 `seats.teams[]` 的该队队员数判定，"两人"是四人两队的基线形态、不是写死的常数）。掼蛋基线的三种收尾都由这一条覆盖——该队两人占据 1、2 名（双下）、两人之间夹进对家一人（该队 1+3）、对家两人先出完而本队头游在前（该队 1+4）。名次按**实际出完先后**排：先出完者为头游（第 1 名），其后依次顺排；**名次空间是 `1..=seats.count`**（四人局即 1–4，六人局即 1–6，编码见 §5.1）。结束时仍**未出完**的座位按**剩余手牌张数升序**依次填入剩余名次（张数相同时按座位号小者在前）——四人局双下收尾时这两人即取 3、4 名（少者三游）。头游所在队即本局胜方队，`round_record.players[].rank` 与 `result.ranks` 的 `seats.count` 个名次一律按此规则填写（`result.ranks` 按名次 `1 → seats.count` 列 seat）。L3 的"必须全部终局"（§5.4）即指此终止条件。

升级档的**基线取值**如下（`upgradeBasis: rank-combo`；`teamRanks` 长度随 `teamSize` 参数化，四人两队下即恰为原 `[1,2]`/`[1,3]`/`[1,4]`，取值逐字未动）：

| 名次组合 | 升级数 | 下局 `levels` |
| --- | --- | --- |
| 1+2（双下） | +3 | 胜方队的 `levels[team]` 往上 3 档 |
| 1+3 | +2 | 胜方队 +2 |
| 1+4 | +1 | 胜方队 +1 |

换 `upgradeBasis: score-diff` 时本表不适用：升级档改由 `rankScores` + `upgradeByDiff` 按两队分值差决定（§5.1 `levelTable` 行），`guandan-classic` 不用该 basis。

`levels` 的存放位置：房间状态字段 `levels`（每队一个 u8，存 `levelTable.ranks` 的数组下标：0 对应 `initial`=2、12 对应 A；序列化、`stateHash` 拼接与双端比较一律按下标，不存牌面键），**不在手牌区、不在 `player_stat`**（后者的 `rank_level` 是段位，字段名不叫 `level`，见 §5.8）；一场（match）内逐局累积，写进 `round_record.levels_after`。队级计数器 `aFailStreak`（`[u8; teams]`，每队一个 u8，取值 **0–3**）与 `levels` 同位置同索引：只有该队 `levels=A` 的局才更新它——过 A 则该队本场随即结束（计数不再使用、不参与任何判定），未过 A 则结算时 `+1` 并写入；某局结算时若该值达到 3，则本队 `levels` 退回 `levelTable.initial`（起点 2）并把该值清零；过 A 即该场结束、计数无意义。该队不在 A 上打的局不改动它（此时它恒为 0）。

**结算的执行顺序**（`settle` 纯函数，四步固定，不得调换）：① 按本节「局终止与名次」定出 1–4 名 → ② 判过 A：该队 `levels=A` 且本方组合为 `1+2`/`1+3` 即过 A，本场结束 → ③ 未过 A 且该队 `levels=A`：`aFailStreak[team] += 1` 并写入，写入后达到 3（阈值读顶层 `level.pass.streakLimit`，基线 3）则 `levels` 退回 `levelTable.initial`（起点 2）、`aFailStreak[team]` 清零 → ④ 该队 `levels≠A`：按 `levelTable.upgradeBasis` 分派取 `step`——`rank-combo` 查 `upgrade[]` 命中胜方队的名次组合（`step: 0` 视为不升档）；`score-diff` 按 `diff = 胜方队各名次分值之和 − 负方队之和` 查 `upgradeByDiff` 中首个 `diff ≥ minDiff` 的 `step`（全不命中取 0）；再按 `step` 推进 `levels`（跨 A 停在 A，超额作废）。

**升级跨 A 的处置**：从 K 打 `1+2`/`1+3`/`1+4` 升级时，先在 A 停留一局（不越级），下一局在 A 上打 `1+2` 或 `1+3` 即算"过 A"；即 `level` 到达 A 后所有超额升级数作废，不累积（`1+4` 到达 A 同此处置）。这样"过 A"永远只由"在 A 的那一局名次组合是否为 `1+2` 或 `1+3`"决定。到达 A 之后，若该局未过 A，该队 `levels` 保持 A（**不退档**），继续在 A 上打下一局；只有连续 3 局在 A 上都未过，才按下面「过 A」段退回起点 2。

**过 A**：`levels[team]=A` 的一局，本方名次组合为 `1+2` 或 `1+3`（本方拿到头游、且末游是对家）即算过 A，本场随即结束；`1+4`（本方头游但队友末游）不算，其余组合（本方未拿头游）也不算。**未过 A 时不退档**：该队 `levels` 保持 A，继续在 A 上打下一局。同一队在 A 上**连续 3 局未过**（`aFailStreak[team]` 满 3）时，该队 `levels` 退回 `levelTable.initial`（起点 2）并把 `aFailStreak[team]` 清零。跨 A 的超额升级数作废（到 A 即停，不累积）。本节写的「3 局」与「`1+2`/`1+3`」是**掼蛋基线取值**，实际判定读顶层 `level.pass`（`streakLimit` 与 `combos`，§5.1），切换后判定逻辑与四条口径不变：`1+2`/`1+3` 算过 A、`1+4` 不算、未过 A 不退档、达 `streakLimit` 才退回起点 2。

**场次终止**：一场（`match`）的终止条件二选一：①某队过 A；②触达安全局数上限（RuleSet `matchCap.rounds`，缺省 **12 局**、取值 1–99，仅防死循环，**不是胜负判据**）。未过 A 达上限时，以各队当前 `levels` 档位较高者为胜方，档位相同记平局（`winner_team` 为空）；该判定只比 `levels` 档位，`aFailStreak` 不参与比较。整场流局或全员离线超时导致中止的，记 `aborted`（按 §6.3/§6.5 处置）。结果写入 `match_record.winner_team` 与 `end_reason`（`passed-a` / `safety-cap` / `aborted`）；段位与积分按**场**结算，"连续 3 局在 A 上未过则退回起点 2"只作用于队级牌 `levels`（见本节「过 A」段），不影响段位与积分。

**段位与评分（v1 口径）**：字段 `rating` / `rating_games` / `placement_left` / `rank_level` 保留（§5.8），跨端一致（S9 仍成立）。定级赛分档、Elo 式浮动、K 衰减与 `rating → rank_level` 阈值映射整体降为 v2 预留（对应 R18/R19，§10.2）；v1 不在本节定义定级分档表、Elo 参数表与阈值表。评分只在服务端结算事务内用整数运算算出，**不进 `stateHash`、不进双端逐步比对**；`human-ranked` 之外的对局按 §5.15 真值表判定，不写本表字段。**v1 的最小口径**（只求可判定、跨端一致，不引入任何阈值表）：`human-ranked` 每场结算，胜方 `rating += 1`、负方 `rating -= 1`（初始 0，整数，允许负值），`rating_games += 1`；`rank_level` 在 v1 不随对局变化（恒为初始档 `0`，「固定分档」在 v1 的落点就是这一档，分档表本身 v2 提供），`placement_left` 不消耗、恒 0；排行榜按 `(rating, rating_games)` 降序取前 N。

### 5.14 生命周期与升级路径

`lifecycle` 取值为 `[deal, tribute, play, settle]` 四段，**v1 只有这四段**；`turn` 是 `play` 内的轮次子阶段，不是 phase。`deal`/`tribute`/`settle` 为零动作 phase，由 `deal.shuffle`、`tribute.*` 原语与结算纯函数自动执行，豁免 `legalFrom` 指向要求；`play` 是唯一有动作的 phase，必须有至少一条 `legalFrom` 指向它。四段的进入条件、退出条件、超时行为在 §6.3；每段允许的动作集合由 §5.1 的 `actions[].legalFrom` 定义（取值是 `patterns`、某个 zone 的 id 或某个 phase 名，不写自由字符串）。`window`/`arbitrate` 不进 v1 的类型与枚举，v2 立项前纸面推演确认后按 engine 次版本加回（R13）。队级状态里跨局保持的只有两项：`levels`（每队一个）与 `aFailStreak`（每队一个，取值 0–3，规则见 §5.13「过 A」段）；两者都在 `settle` 段结束时由结算纯函数更新；新一场（match）首局开局时两者各自初始化（`levels` 取 `levelTable.initial`、`aFailStreak` 置 0），不继承上一场；`lifecycle` 仍是四段，不因此增减。

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
| `GET /api/match/queue/{ticketId}` | — | `{state: waiting\|matched\|timeout, matchId?}` | 轮询结果（v1 不推 WS 匹配事件，降低一条长连接）；matched 后客户端凭 `matchId` 经网关粘性路由接入房间（§4.5），不单发实例地址 |
| `POST /api/room/create` | `{gameId, mode, seatsCount}` | `{matchId, inviteCode?}` | 建房；按 §5.6 `gameId` 与 `rulesetId` 映射取生效 RuleSet；`seatsCount` 取 RuleSet 的 `seats.count`，不由客户端随意指定 |
| `POST /api/room/join` | `{inviteCode}` | `{matchId, seat, accept, availableSeats[], reason?}` | 凭邀请码进房；入座结果与 §5.9 `JoinAck` 同构：成功返回分配座位，`RS_SEAT_TAKEN` 经 `reason` 返回并附 `availableSeats[]`，不经 HTTP 409 |
| `GET /api/history/matches` | `{gameId?, cursor, limit}` | `[{matchId, matchType, endReason, winnerTeam, startedAt, rounds}]` | 按玩家分页返回战绩摘要（`match_record`/`round_record` 聚合） |
| `GET /api/replays/{matchId}` | — | 脱敏回放包或下载地址 | 只对已结束房间返回；玩家读取本人回放不属敏感导出（§8.3） |
| `GET /api/compliance/status` | `{playerId}` | `{verified, minor, dailyMinutesLeft}` | **合规适配层的唯一接口**（§2.1 的"接口占位 + 开关"）：开关关闭时恒返回 `{verified:true, minor:false, dailyMinutesLeft:-1}`，U12 答复为"要接"后由适配器填充真实值 |

**错误码**：以上接口全部复用 §5.7 错误码**全表**，不另立一套。

**`mode → matchType` 映射与分流真值表**（`matchType` 定义在 §5.8 `match_record`，其它地方引用不得另造）：

| 入队 `mode` | `matchType` | 写 `player_stat` | 进排行榜 | 计段位 |
| --- | --- | --- | --- | --- |
| `ranked` | `human-ranked` | ✓ | ✓ | ✓（段位字段落库，v1 计算口径见 §5.13 最小口径：`rating` 按场 ±1、`rank_level` 恒初始档；定级分档与浮动算法 v2 预留） |
| `casual` | `human-casual` | ✗（只写 `round_record`） | ✗ | ✗ |
| `mixed`（含 bot 补位） | `mixed` | ✗ | ✗ | ✗ |
| 纯 bot / 离线局（`ai-practice`） | `bot` | ✗ | ✗ | ✗ |

**「计段位」一列口径**：`human-ranked` 写入 `rating`/`rank_level` 字段并进排行榜；定级分档、Elo 浮动与阈值映射属 v2 预留（§5.13、R18/R19），v1 不在本段判定级落位。`ai-practice`（人机房，模式侧别名，落库 `matchType` 记 `bot`）与自定义房不计段位；其余 `mode` 一律不计段位、不写 `rating`。

`human-ranked` 之外的房间不写段位与排行字段；离线局不上云、不计战绩（§6.6 三档可信度）。

### 5.16 `minClientVersion` 与版本下限

`minClientVersion` 是**独立配置项**（后台可改、落审计），不由 RuleSet 的 `interpreter` 区间推导——两者含义不同：`interpreter` 是引擎侧 semver 区间，`clientVersion` 是客户端构建版本，二者之间没有任何可计算的映射。

- 取值形态：`{platform, minVersion}` 数组（四个平台各一条），`GET /api/games` 按请求方平台取对应值下发。
- 版本格式：`clientVersion` 与 `minVersion` 均为 semver 字符串 `MAJOR.MINOR.PATCH`（v1 无预发布标签）；比较按 semver 优先级分段数值比较（各段按整数比，不按字符串字典序，避免 `1.10` 与 `1.9` 颠倒），`CLIENT_TOO_OLD` 按此判定。
- 平台枚举：`platform` 与协议中的 `clientPlatform` 共用同一闭集 `web`/`windows`/`macos`/`linux`。
- 上调时机：协议 sunset 后（§9.2）、或旧版本出现无法在服务端兜底的问题（如 wasm 语义依赖）。
- 上调后旧客户端行为：`JoinRoom`/`login` 直接返回 `CLIENT_TOO_OLD`，客户端只展示"请升级"页，不再进入大厅。
- 下调不提供（历史上调即不可回退）。
- `viewSchemaVersion` 由 **RuleSet 作者分配**：每 RuleSet 内自增，只增不改序；后台不统一发号、不跨 RuleSet 对齐编号。客户端白名单缓存键为 `(rulesetId, viewSchemaVersion)` 二元组（§9.2）。



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
- **序号作用域**：`seatSeq` 是**每座位单调递增**的，服务端按 `(matchId, seat)` 维护水位；广播顺序用 `roomSeq`（房间内全局递增）。两个序号**在每场（match）内从 0 起算、跨局不归零**（第二十三次变更，2026-11-24 用户裁定）：该座位首条动作 `seatSeq=0`、房间首条广播事件 `roomSeq=0`，进入第 2 局及以后各局时**接着上一局继续递增**，只有新一场（`matchId` 变化）才重新从 0 起算；`snapSeq` 与 `roomSeq` 同一序列、同样跨局连续。**局切换不靠序号归零判断**，由 `SettleResult.roundNo` 与下一局的 `DealEvent` 标识；回放导出按 `roundNo` 切段，客户端只需"序号严格递增"一条校验规则。两者字段名不同（§5.9），不混用。`STALE_SEQ` 只按座位水位判定，避免 4 人并发出牌互相误判过期。
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

**竞态的唯一裁决规则**：动作与超时同 tick 到达时，以"服务端单写者队列的入队顺序"为准——房间所有输入（玩家动作、tick、系统事件）都进同一个串行队列，队列里先执行的先生效；若动作在"超时事件"入队之前已入队，则动作有效且不判超时，反之判超时并丢弃动作，返回 `TOO_LATE`（§5.7：写入 `ActionAck.reason`，不推进 `seatSeq`、不计入该座位超时次数，客户端按座位水位重拉增量）。同 tick 内**不按时间戳比较**，避免时钟粒度造成两端结论不同。

**相位转移条件**（`lifecycle` 四段 `deal → tribute → play → settle`；进/退条件唯一规格在此，§5.14 引用本节；相位机只按本表转移，不存在表外跳转）：

| 转移 | 进入条件 | 退出条件 | `stateHash` 重算 |
| --- | --- | --- | --- |
| `deal → tribute` | 发牌完成（每座位手牌数到位、剩余 0 张） | 进贡/还贡处理完成（抗贡跳过分支见下一行） | 转移后重算一次 |
| `deal → play` | 场次首局（无上局名次）或本局抗贡成立，`tribute` 整段跳过（§5.13） | — | 转移后重算一次 |
| `tribute → play` | 进贡与还贡均已落位（抗贡局不经过本转移） | 领出座位确定 | 转移后重算一次 |
| `play → settle` | 某一队两人均已出完牌（§5.13「局终止与名次」） | 结算四步执行完成（§5.13 结算顺序） | 每步 `apply` 后重算；`settle` 结束后重算一次 |
| `settle → deal` | 下一局开始（过 A 则本场结束，不进入本转移） | 下一局发牌完成 | 新一局初始化后重算 |

各相位的超时行为按本节计时器表；相位转移本身不引入新的计时器。

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
| 动作日志 | 每步动作后追加到 Redis Stream | `ActionRecord = {seatSeq, roomSeq, roundNo, seat, requestId, kind, payload, tsMs}`（`roundNo: u16` 为跨局连续序号下唯一的局归属标识，回放按它切段；`seatSeq`/`roomSeq` 跨局不归零，见 §6.2），与回放包 `actions.jsonl` 同一结构（**只此一份**，不再另立第二个记录类型）；重放水位一律用 `roomSeq` | — |
| 房间快照 | 每 30 秒 + 每阶段结束 | `{seed, engineVersion, interpreterVersion, rulesetVersion, phase, turnSeat, levels, attackingTeam, seats[seat], zones[(seat, zoneId, cards)], lastPlay, seqWatermark[], tribute, finishOrder, rng, aFailStreak, snapSeq}`——对应 §5.12 第 1–11 项全部字段，再加 `seed`/`snapSeq`；`tribute` 段进贡结束后归零但字段位保留，抗贡局/首局取占位值。**快照字段集与 `stateHash` 字段集必须一一对应**，少一个字段就等于少一段可重建状态 | — |
| 重建方式 | — | 载入快照（`snapSeq` 即快照覆盖的最大 `roomSeq`，与 `roomSeq` 同一 u64 序列，故可直接比较步进）→ 从 `snapSeq+1` 重放到 `lastRoomSeq` → 校验 `stateHash` | 重放步数 ≤ 快照间隔内的动作（通常 < 30 步），预期 < 100ms；最坏全量 300 步 < 200ms |

**恢复失败的处置**：若日志与快照对不上（`stateHash` 不一致），不允许"修补"，直接判该场流局；补偿记录先落 `settlement_compensation` 表（`issued=false`，不发放；支付接口启用后才发放，见 §5.8）。理由：宁可判流局，也不能让两个客户端看到不同的牌。`settlement_dead_letter` 只用于结算补写，不承载流局补偿。

### 6.6 结算的原子性与对账

结算涉及三件事：战绩、积分、房间状态。顺序固定为：

1. 计算结算（纯函数，输入 `state`，输出 `Settlement`）；
2. 在进入 `settlementId` 事务**之前**，先以 `settlementId = f"{matchId}:{roundNo}"`（带分隔符，避免 `m1`+`23` 与 `m12`+`3` 撞键）为唯一键单独插入一条 `settlement_event`（结算尝试记账，先于业务事务落库）；随后在同一 `settlementId` 幂等事务内写 `round_record` 与 `player_stat`；
3. 写入成功后才把房间标为 `round-ended`，并广播结算结果；
4. 写库失败则房间停在 `settling`，后台任务按指数退避重试（上限 **3 次**，不是 10 次）；3 次仍失败即**放弃自动重试**：房间转 `settle-failed`，广播"本局结算失败（可重试）"并允许开下一局，失败局写进 `settlement_dead_letter` 表由人工按 `settlementId` 补写，补写成功后才补算战绩与积分——不引入"永久卡在结算中"的状态。

**对账任务**：每日跑两份报表，**只比条数，不比百分比**（"版本号偏差 0.1%"这类说法无法定义分母，原稿作废）：
1. 条数等式：成功写入的 `round_record` 条数 + 未补写的 `settlement_dead_letter` 条数 = `settlement_event` 条数（结算尝试次数）。`settlement_event` 在结算尝试发起时、进入 `settlementId` 事务之前单独插入（§5.8），以 `settlementId` 唯一，不依赖日志导出与对象存储；`settlement_dead_letter` 行尚未计入 `round_record`，故在左侧相加；已人工补写成功的死信不再计入左侧；差异条数 > 0 即告警；
2. `order.status=paid` 金额合计 vs `payment_callback` 验签通过金额合计，差异金额 > 0 即告警（v1 无真实订单，该报表恒为空，**不列入 P0 清单**）。

只告警、不冻结：首版没有值班团队，"冻结当日结算修正"没人解冻，反而会卡住次日结算（见 §7.5）。

**按 `matchType` 分流**：`matchType` 取值 `human-ranked` / `human-casual` / `bot` / `mixed`，定义在 `match_record`，计段位、进排行、写 `player_stat` 的口径以 §5.15 真值表为准：只有 `human-ranked` 写 `player_stat`、进排行、计段位；其余类型只写 `round_record`。匹配两池分开，`mixed` 由"不足 4 人 bot 补位"产生，UI 必须标注"AI 陪练"。

**离线局的三档可信度**（原稿"所有判定都在服务端"与 G3 的离线要求直接冲突，这里分级）：

| 档 | 产生方式 | 判定主体 | `matchType` | 上云 | 计战绩 |
| --- | --- | --- | --- | --- | --- |
| `online-human` | 4 真人联机 | 服务端 | `human-ranked` / `human-casual` | 是 | 计 |
| `online-bot` | 联机且含 bot 补位 | 服务端 | `mixed` | 是 | 不计 |
| `offline-local` | 完全离线与 bot 对打 | 客户端 wasm（不可信） | `bot`（`ai-practice` 为模式侧别名） | 否，只存本地 | 不计 |

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
| `viewhash_mismatch_total` | Counter | `match_id, seat, room_seq` | **>0 即 P0**：服务端下发前重算 `viewHash`，与已下发视图不一致即记一次并告警；客户端不重算、不上报视图哈希（§5.9/§5.12），不设 `client_platform` 维度 |
| `action_illegal_total` | Counter | `ruleset_version, action_kind` | 5 分钟窗口内 非法动作 / 该窗口**总动作数** >0.1% → P1（分母见下） |
| `action_cpu_overrun_total` | Counter | `ruleset_version` | >0.1% → P2；CPU 超预算的处置口径见 §8.6 CPU 炸弹行，本指标只计数 |
| `action_quota_exceeded_total` | Counter | `ruleset_id` | L3 dry-run 期单次 `legal_actions` 实测候选枚举峰值超 4096 即记一次 → P2；不在载入期 L2 设闸门，运行期不按步拒绝（§5.7 `RS_QUOTA_EXCEEDED`） |
| `room_panic_total` | Counter | `ruleset_version` | 解释器 native 侧 panic 被 `catch_unwind` 捕获即记一次 → P2，随 `process_crash_total` 一并观察 |
| `action_ack_p99` | Histogram | `game_id` | >300ms 持续 5 分钟 → P2（体验预警）；100ms 是 §1.4 S6 压测容量基线目标，300ms 是线上体验告警线，二者用途不同 |
| `room_active` | Gauge | `game_id, ruleset_version` | 相对**前 15 分钟滑动均值**下跌 30% → P1 |
| `room_create_fail_rate` | Ratio | `reason` | >5% → P1 |
| `reconnect_success_rate` | Ratio | `client_platform` | <95% → P1 |
| `settlement_pending_age` | Gauge | `match_id` | 单局 >60 秒 → P1（结算卡住） |
| `process_crash_total` | Counter | `instance` | canary 期按次数计：崩溃次数 >0 即自动回滚（§9.4）；`full` 期按比率：崩溃率 = 崩溃次数 / 该实例承接**房间创建次数** >0.1% → P1，同时是 §9.5 回滚判据 |
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
| 两端看到的牌不一样 | 取该局 trace，比对服务端逐步 `state_hash`（用于漂移检测）与服务端自证的 `viewHash` 记录（用于首处分歧定位）；两类哈希先按 §5.12 投影后再相比 | 定位第一个分歧的 `roomSeq` | 导出回放，用离线重放器复现 |
| 结算对不上 | 查 `settlement_pending_age` 与 `round_record` | 核对 `settlement_id` 是否重复写入 | 跑当日对账报表，看条数或金额差异 |
| 发布后新房间异常 | 看 `canary_illegal_rate` 与 `room_active` 按版本的变化 | 确认是否已自动回滚 | 用 L3 dry-run 复跑该 RuleSet，定位具体原语 |

### 7.7 一键导出回放包

一个 `.zip`，内容是：

| 文件 | 内容 |
| --- | --- |
| manifest.json（重放包内） | `matchId`、`seed`、三个版本号、座位与玩家标识（脱敏）；回放头不记 `aFailStreak`，由 `actions.jsonl` 重放重建，旧格式缺字段按 0 初始化 |
| `actions.jsonl` | 逐条动作，与 §6.5 的 `ActionRecord` **同构**：`seatSeq / roomSeq / roundNo / seat / requestId / kind / payload / tsMs`；两列序号都显式保留，重放水位一律用 `roomSeq`，分歧步按 `roomSeq` 对齐 `hash_trace.jsonl` |
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
| 手机号 | 业务表存哈希 + 尾号 4 位；明文仅存在于短信发送链路的内存中；哈希算法与盐策略见 §5.8 `player_profile`（HMAC-SHA256 + 单盐 `server_salt`，轮换按 §9.7 五步） |
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
| CPU 炸弹（超大枚举、无限递归） | 候选枚举 4096 条为 **L3 dry-run 期实测峰值上限**：单次 `legal_actions` 实测峰值超 4096 判 dry-run 无效、拒绝发布，同一码 `RS_QUOTA_EXCEEDED`（不在载入期 L2 设静态闸门，§5.7）；运行时单动作预算 **5ms**，超预算记 `action_cpu_overrun_total`（P2），**不判该动作非法、不按步拒绝、不进非法率、不参与 canary 判定、不作回滚条件、不对房间限流**——合法性只由规则决定，不能由机器快慢决定；本条是 CPU 超预算的唯一口径，其余章节只引用本节 |
| 解释器 panic 拖垮进程 | 服务端 native：动作执行包在 `catch_unwind` 内，panic 转为"该动作失败"并记 `RS_INTERNAL_PANIC`；**wasm 侧没有 unwind**（`panic=abort`），策略是"panic 即整实例重载"。S1 的例外条款写在 §1.4 里（`panic-injection` 用例不参与双端同哈希判定），本表不另立例外 |
| 恶意 RuleSet 拖慢全体 | v1 单实例 + 网关粘性路由，不做房间迁移（§4.5）。CPU 超预算按本节 CPU 炸弹行口径处置：只记 `action_cpu_overrun_total` 并走 P2 告警（§7.2），不另设判定 |
| 越权读取他人手牌 | `view.project` 是唯一出口，引擎不提供全量视图 API |

### 8.7 玩家侧反作弊

| 手段 | 说明 | 边界 |
| --- | --- | --- |
| 服务端权威 | **联机对局**的判定、计时、发牌都在服务端 | 已由架构保证；离线局按 §6.6 降级为本地可信度最低档 |
| 信息裁剪 | 只下发 `view(seat)` | 已由 §5 保证 |
| 异常行为检测 | 出牌耗时分布、连胜异常、同 IP 多账号、`statehash` 不一致计数 | 只做打点与告警，不做自动封禁 |
| 客户端完整性 | 上报客户端版本与包哈希，异常版本进灰度名单 | 不做强校验（会误伤绕开渠道更新的用户） |
| 记牌器 | 属"用公开信息辅助"，不封堵 | 这是合规能力，不是外挂 |
| 评分防刷分 | **v2 预留**：重复对局衰减系数、档位 K 与 K 衰减随段位/Elo 浮动整体降为 v2（R18/R19，§5.13/§10.2），v1 不实现刷分衰减算法。`rating`/`rank_level` 只在服务端结算事务内用整数运算算出，不进 `stateHash`、不进双端逐步比对；人机房（`ai-practice`）与自定义房不写 `rating` | 评分口径改动不影响判定与结算（§5.13、§6.6） |

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
| engine 补丁版本 | 判定结果不变 | 回放先比版本三元组 `(engineVersion, interpreterVersion, rulesetVersion)`，版本一致再比 `stateHash`（版本串是旁挂字段、不进哈希拼接，§5.12）；engine 补丁版本下 `stateHash` 必须一致，CI 强制。跨版本比对一律用 `(版本, 哈希)` 二元组，不跨版本直接比哈希 |
| engine 次版本（新增原语） | 老 RuleSet 行为不变 | 老 RuleSet 不引用新原语即不受影响 |
| interpreter 次版本 | 向后兼容老 RuleSet | RuleSet 的 `interpreter` 区间校验 |
| interpreter 主版本 | **不承诺兼容** | 强制要求老 RuleSet 迁移，或冻结旧解释器版本继续服务（双版本并存上限 3 个月） |
| RuleSet 新版本 | 只影响新房间 | 运行中房间锁版本（§5.5） |
| 协议字段 | 只增不删、不改语义，**但每次增字段同时登记 sunset 条件** | 服务端按 `clientVersion` 做特性开关；**sunset 规则**：某字段上线满 3 个月且旧版本客户端占比 < 1%，即可删除该特性开关与旧字段，`minClientVersion` 同步上调（§5.16），不承诺无限期维护 N 套分支 |
| 视图白名单（`viewSchemaVersion`） | 只增不改序；版本号由 **RuleSet 作者分配**、每 RuleSet 内自增（u16，溢出即视为协议主版本变更），后台不统一发号、不跨 RuleSet 对齐编号 | 服务端按房间锁定的 `viewSchemaVersion` 下发 `view`（§5.9/§5.12）；客户端白名单缓存键为 `(rulesetId, viewSchemaVersion)` 二元组（`rulesetId`/`rulesetVersion` 随 `RoomSnapshot` 下发）。旧客户端遇到未知 `viewSchemaVersion` 只渲染已知字段、不做视图哈希比对（避免假阳性），并在下次登录按 `minClientVersion` 拉起更新；删字段的唯一合法路径是先把它移出白名单并抬版本号，不允许原地改序 |
| DB 表结构 | 只前滚迁移 | 破坏性变更走"影子表 → 双写 → 切换 → 观察 → 删旧"五步 |

**最贵的一条**：`interpreter` 主版本变更会让所有历史 RuleSet 失效。因此规则语言的语法要一次设计到位（`A14` 的风险点），而**解释器版本存档**是兜底：保留旧解释器二进制，老 RuleSet 按 `ruleset.pinned_interpreter_version` 选择对应的旧解释器；`interpreter` 区间内多版本并存时，一律以发布时锁定的 `pinned_interpreter_version` 为准。

**在役解释器版本登记**：在役清单由 `ruleset_release.pinned_interpreter_version` 聚合得出（不另建登记表），每条记录含解释器版本、进入时间、计划退场时间；引用计数 = 引用该版本的未 `retired` release 行数 + 当前在房数。到期（双版本并存满 3 个月）按三步退场：①停发布——新 RuleSet 不再 pin 该版本；②停新建——新房间不再选该版本解释器；③仅保留回放只读——旧局回放按 §9.3 镜像窗口处置。

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
| `canary` | 新建房间的 10% | 30 分钟 | 该版本新建房间内 非法率 ≤0.05%、`statehash` 不一致 0、服务端复核 `viewHash` 不一致 0、`process_crash_total` 次数 0（次数 >0 即回滚，不按比率）；CPU 超预算按 §8.6 处置，不计入本判定 | 自动回滚 |
| `full` | 100% | 持续 | 指标与 canary 同口径不恶化；`action_ack_p99` ≤ 发布前 7 日均值 ×1.1。首次发布无 7 日基线时，只用绝对值上限 `action_ack_p99 < 300ms`（§7.2 P2 线）；自第二个版本起才比对 7 日均值 | 回滚到候选集最近一个 `active`（口径见 §5.6 rollback 行） |

三处枚举对齐：本节阶段名、§5.6 接口的 `stage`、`ruleset_release.stage` 用同一套字符串；release 行阶段集合为 `canary/full/superseded/rolled_back/retired`，`draft`/`validated` 由 `ruleset_validation` 聚合派生、不落 release 行（§5.8）。**取消 `ramp5`/`ramp25`**：内测规模下多两级只增加人工推进成本，不增加安全性（§8 的 YAGNI 剪枝）。

**内测初期的样本量问题**：canary 覆盖 10% 新建房间，30 分钟仍可能只出个位数房间。因此增加准入条件：**该阶段样本动作数 < 200 时，不按比率判定，改为"零 P0 事件"判定**，并延长观察窗口至 2 小时；满 2 小时仍 < 200 的，由人工 TOTP 决定放行（`promote` 到 `full`）或回滚，决定与理由落审计（出口条件见 §5.6）。

阶段切换的授权口径（与 §8.1 一致）：`canary` 达标后由人工经 `promote` 接口带 TOTP 确认转 `full`；`canary` 失败时**自动回滚**，不需要人确认，回滚后 10 分钟内人要补一条理由落审计。

### 9.5 回滚

| 项 | 规定 |
| --- | --- |
| 触发条件 | P0 状态漂移；canary 内非法率 >0.05%；canary 期 `process_crash_total` 次数 >0；`full` 期 `process_crash_total / 房间创建次数 > 0.1%`；当日对账报表差异非零 |
| 决策人 | `canary` 阶段系统自动；`full` 阶段由管理员在 5 分钟内决定 |
| 时间预算 | 从决定到新房间不再使用坏版本 ≤5 分钟。两步顺序：先改 `ruleset_release` 表目标行（唯一真相），再重建对应 Redis 指针；表改成功而指针重建失败时，新房间回源查表判定版本，失败落告警并重试，不以 Redis 指针为真相 |
| 已开局房间 | **不重放、不回滚**，按锁定版本打完；若该版本被判定有致命缺陷，强制流局并按 §6.5 处置：补偿记录落 `settlement_compensation` 表（`issued=false` 不发放）；`settlement_dead_letter` 仅用于结算补写 |
| 回滚后的 RuleSet | `stage` 记为 `rolled_back`（**不是 `retired`**）；该版本经重新校验通过后可合法转移回 `canary`/`full`，否则只读、要改就发新版本。`superseded` 表示曾到 `full`、已被更新 `full` 取代、仍可回滚；`retired` 只表示人工确认下线（与"被取代"无关），不再被任何房间引用 |

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
| R1 | 规则判定写错 + DSL 表达力不够（两条风险共用一个编号，名册见 §11.1 同名行） | ①黄金用例覆盖牌型全表、进贡/还贡/升级档位残局（§10.3 手工构造 40 条）；②M0 末纸面推演族内掼蛋变体 RuleSet（不实现）；掼蛋基线 RuleSet 走 L1–L4 | ①与 §5.13 规格逐点一致、逐步 `stateHash` 一致；②纸面完整规则不需新增原语；基线校验全过，否则触发降级（`A14`） |
| R2 | 40～44 周基线不成立 | 第 16 周 gate 验收：Web + 本地 bot 打完一场（含再来一局、杀进程后恢复） | 验收记录入台账；未过即按 R2 缩范围 |
| R3 | 解释器性能不达标 | §10.5 基准 + M3 压测；按 A4 峰值 ×3 倍余量空转 | P99 达预算；实测上限写回 §4.8；房间密度不低于实测上限 60% |
| R4 | 规则语法返工 | 表达力矩阵：掼蛋 + 族内变体（纸面）+ 麻将（纸面）对照原语清单 | 语法冻结评审通过；发布后只做加法 |
| R5 | 商店渠道缺位 | Web 分享链路走查：从链接到打完一局 | ≤3 步，且无需安装 |
| R6 | 2 人上下文过载 | 每两周一次"近两周改动 crate 分布"审查记录；owner 分工表（一人引擎 + DSL，一人平台 + 客户端） | 同一周内无人切换三个以上模块；crate 边界即职责边界 |
| R7 | 状态漂移 | §10.4 双端一致性；断线恢复故障注入（含 `snapshotReset` 分支） | 每步 `stateHash` 一致；恢复后哈希与断线前一致 |
| R8 | bot 体验差 | bot 对 bot 对子胜率：固定种子、固定名单文件 `golden/bot_matchup.txt`（bot 对子胜率专用，与 §10.4 一致性快速集 `fast20.txt` 分开，§10.3 注册）；三档各跑固定集。**生效时点：三条胜率断言挂到 M2 之后执行；M0 gate 与 M1 gate 不受本断言约束** | 对子胜率硬断言：`easy` 对 `normal` ≤30%、`normal` 对 `easy` ≥60%、`hard` 对 `normal` ≥60%；胜率区间随版本调整，测试只断言对子胜率，不冻结算法与区间 |
| R9 | 四端打包与签名卡住 | 四端产物构建与安装冒烟（§10.7） | 四产物均可安装并进房 |
| R10 | 素材授权问题 | `../../assets/license.md` 全表可查 + 抽样核对许可原文 | 0 条缺原文 |
| R11 | 人机混合房引发不满 | 集成用例：`mixed` 房结算后查 `player_stat`；AI 座位标注走查 | 该表 diff 为空；AI 座位标注可见 |
| R12 | 短信通道受阻 | 集成：mock 短信通道 + 通道超时/失败注入；测试号白名单与备服务商切换 | 重试/降级链路可用，不阻塞登录续座；失败落日志可告警 |
| R13 | 表达力上限未被验证 | v1 不启用 `window`/`arbitrate`；M0 末纸面推演"麻将最小对局"（不实现） | 输出结构缺口清单（标明是否需新原语）；v2 立项前纸面推演确认后按 engine 次版本加回 |
| R14 | 死锁 / 无法终局 | L3 随机对局 200 局（每晚） | 100% 终局，非法动作率 0 |
| R7 | 断线恢复出错 | 故障注入：随机断网 5s、随机杀房间进程、**断线超过 200 条缓冲（触发 `snapshotReset`）** | 恢复后 `stateHash` 与断线前一致；`snapshotReset` 分支必须有一条用例断言"客户端丢弃本地状态并接受新快照" |
| R15 | 结算账不平 | 集成测试跑"结算 → 落库 → 对账"，并模拟写库失败 | 幂等键不重复；对账报表差异 0 |
| R16 | 发布链被绕过 | 每月演练：注入非法 RuleSet 走发布链 | 被 L2/L3 拦下；人为绕过时 canary 自动回滚 |
| R3 | 性能回退 | 基准固定基线，超阈值阻断合并 | `legal_actions` P99 退化 >10% 即阻断 |
| R17 | 客户端兼容断裂 | 旧版本客户端进房冒烟 | 旧版能看到玩法列表与 `minClientVersion` 提示 |
| R18 | 定级赛归属 | **v2 预留**：定级赛归属与 `rank_level` 落位算法随段位体系整体降 v2（§5.13/§8.7）；v1 只保留 `rating`/`rank_level` 字段落库，不跑定级落位断言 | v1 不判定级落位；v2 立项后按本表补集成用例 |
| R19 | 同对手重复对局 K 衰减 | **v2 预留**：重复对局衰减系数与 K 衰减随 Elo 浮动整体降 v2（§5.13/§8.7）；v1 不实现刷分衰减 | v1 不跑 `Δrating` 衰减断言；v2 立项后按本表补集成用例 |

### 10.3 黄金用例怎么来

| 来源 | 数量目标 | 说明 |
| --- | --- | --- |
| 手工构造边界（M0–M1 用） | 40 条 | 逢人配组顺子、四王炸对 8 炸、双下升级、抗贡、过 A 未过（连续 3 局退回起点 2）、进贡/还贡；**S10 与 §10.2 在 M0–M1 的用例来源就是这一批**，此时还没有线上对局 |
| 真实对局录制（内测后累积） | 20 条完整局 | 内测开始后从线上异常局与人工导出的回放包累积，用于 L4 回归 |
| 规则变异 | 12 条 | 同一局在四种争议配置下各跑一遍，验证配置真的生效（S8） |
| 回归种子库 | 100 条 | 每条只覆盖一个原语或一条规则，失败时能直接指到原语 |

规则改动、原语改动、解释器改动的合并门槛统一为：**黄金用例 + 回归种子库全绿，双端一致性零差异**。

**用例编号与目录约定**：用例 id 为 `case-<rulesetId>-<NN>`；每条用例目录固定为 `crates/engine/tests/golden/<caseId>/`，内含 `manifest.json`（规则与三版本号、seed）、`actions.jsonl`（与回放包同构，§7.7）、`expected_hashes.jsonl`（逐步 `stateHash`/`viewHash` 期望值）。每日提交跑的快速集为固定名单文件 `golden/fast20.txt`（用例 id 列表，在本节注册，不按动态抽样）：从手工构造 40 条与规则变异 12 条中选取覆盖全部牌型、进贡/还贡/抗贡、升级档、过 A、顺子边界分支的 20 条；新分支用例未入列时优先补入 fast20，20 个具体 id 在 M1 用例建档时于本节注册。全量集 = 手工构造 40 + 录制 20 + 规则变异 12 + 回归种子库 100 = **172** 条（回归种子库只计一次，不重复加）。

### 10.4 双端一致性测试的具体做法

1. 用例来源：`action log` 集合（`manifest + actions.jsonl`），与回放包同格式。
2. 执行：`native` 直接跑引擎；`wasm` 用 `wasmtime` 加载同一份 `.wasm`。
3. 比对：逐步比对 `stateHash`、`legal_actions` 的排序结果（排序也必须一致，否则客户端提示顺序会不同）。
4. 门禁：任何一步不一致即失败，并输出第一处分歧的 `roomSeq` 与两边状态摘要。
5. 频率：每次提交跑固定名单 `golden/fast20.txt`（§10.3）；每晚跑全量（含所有黄金用例）。
6. 段位与评分的两条用例——**定级赛 10 场归属**、**同对手重复对局 `K` 衰减**（§10.2 R18/R19）——随段位体系整体降 v2 预留，v1 全量集不跑；v2 立项后补入。`rating`/`rank_level` 终值不参与逐步 `stateHash` 比对。

### 10.5 性能基准

| 基准 | 指标 | 实测基线（2026-11-24，`m0-minimal` 夹具） | 门禁 |
| --- | --- | --- | --- |
| `legal_actions`（领出态，27 候选） | P50 / P99 | native 2.700 / 3.100 µs；wasm 2.097 / 2.364 µs | 相对本行基线退化 >10% 阻断 |
| `legal_actions`（跟牌态，1 候选） | P50 / P99 | native P50 0.400 µs；wasm 0.464 / 0.721 µs | 同上 |
| `apply` | P50 / P99 | native 0.800 / 3.500 µs；wasm 2.149 / 2.495 µs | 同上 |
| 单核容量推算 | 可承载房间数 | native ≈**26,900**、wasm ≈**23,300**（按 tick 10Hz、每房间每秒 1 次 `legal_actions` + 1 次 `apply` 共 20 动作/秒、门槛 ≤33.33 µs/动作折算） | ≥1500 房间（§3.2 的 3 倍余量）；当前余量 15–18 倍 |
| 1500 房间空转（A4 峰值 ×3 倍余量） | 常驻内存、CPU | **已实测（2026-11-24）**：1500 房间份（按 A4 峰值 ×3 倍余量，1000 只作占位）常驻状态堆内存合计 **1,065,000 B（≈1,016 KiB）**，折算 **710 B/房间**（构成：`State` 248 + 包装 16、`zones` 5×32、4 手手牌 4×54、`seats` 4、`seq_watermark` 4×16、`a_fail_streak` 2）；区间峰值 1,065,216 B（高出部分为发牌期临时牌堆瞬态，不计常驻）。空转 tick CPU **0.803 ns/(房间·tick)**（5 轮 1500×100 tick，中位 0.803 / 最小 0.802 / 最大 0.823），折算 1500 房间 10Hz 空转 ≈ **12.0 µs/s** | 内存 24 小时增长 >5% 视为泄漏；口径限定：统计型 `GlobalAlloc` 差值，不是 RSS、不含共享 RuleSet 与进程运行时；tick 只做房间级记账（计数器、一次到期比较、读 `phase`/`turnSeat`），不调 `legal_actions`/`apply`，不含网络 IO 与计时器内核开销，故为**地板值** |
| wasm 包体 | gzip 后的 wasm 单文件 | **已实测（2026-11-24，HEAD `061ae60`）**：原始 **678,604 B（662.7 KiB）**、gzip 后 **189,972 B（185.5 KiB）**，压缩比 27.99%；两口径均 ≤1.5MB 目标线，未进 1.5–2MB 区间 | **阻断门禁 2MB**；目标 1.5MB；1.5～2MB 之间不阻断，但发布记录必须写明体积变化与原因；>2MB 阻断合并。测量条件：cargo **默认 release profile**（`opt-level=3`、`lto=false`、`strip=false`、`codegen-units=16`），未为凑体积改 strip/lto；gzip 用 .NET `GZipStream` `SmallestSize`，只压单个 `.wasm`、无 tar/zip 包装；零 import 由"无 import 段（id=2）"与 Node 空 import 实例化成功双重确认 |

测量条件（复核时必须照此复现）：release 构建，native 与 wasm 各跑 **10 万次**；机器 i7-13700KF（16 物理核 / 24 逻辑核 / 3.4GHz / 31.8GB），**非独占**（采样期间系统负载 8–17%，native 侧 max 离群 67–164 µs 由此产生，故本表用 P50/P99 而非 max 作判据）；Windows 上 `Instant::now()` 自身开销约 0.1 µs，已含在 P50 内。

**这组数是地板，不是上限**：`m0-minimal` 只声明了 `single` 一个牌型，候选枚举极轻；`guandan-classic` 有 13 个牌型（含 5/6/7/8 张炸、同花顺、四王炸、钢板、连对），`legal_actions` 明显更重。**M1 签核前必须在 `guandan-classic` 上重跑同一基准并把新数字写回本表**，否则上面的容量推算会被误当成"已验证的成品能力"。空转内存、空转 CPU 与 wasm 包体三项已于 2026-11-24 实测（见第 5、6 行），但空转 CPU 不含 `legal_actions`/`apply` 与网络 IO、包体是默认 release 未 strip 的口径，两者都是**地板**，不得当成成品能力的上限。

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
| R1 | **规则判定写错 + DSL 表达力不够**（R1 = 规则判定写错 + DSL 表达力不够，两条风险共用一个编号）：①牌型、进贡、升级档位等规则本体实现与 §5.13 规格不符；②麻将的鸣牌仲裁、掼蛋的进贡链这类结构写不出来 | 高 | 高 | ①黄金用例出现合法被拒 / 非法被接受，或残局结果与规格不符；②M0 末纸面推演族内掼蛋变体时，"必须加新原语才能表达"出现超过 3 次 | ①黄金用例覆盖牌型全表与进贡/升级残局，并与 §5.13 逐点对照；②降级为"族内模板 + 参数"，工期回落 6～8 周，麻将族推迟 | ①§10.2 R1 的判定用例全绿；②**M0 末（第 6 周）gate**：族内变体能否只靠 RuleSet（纸面）写出完整规则；M3 接入时再实测 |
| R2 | **40～44 周基线不成立**：2 人同时做引擎、DSL、平台、四端、后台 | 高 | 高 | 第 16 周 gate 没达成（Web + 本地 bot 打不完一整场）；或第 20 周时 M2 剩余工作量 > 计划一半 | 三档缩范围：先砍桌面壳（只留 Web）→ 再砍回放式观战 → 最后砍族内变体配置能力 | 16 周 gate 硬验收（§2.4），未过即触发缩范围 |
| R3 | **解释器性能不达标**：单动作超预算，单实例房间密度掉到实测上限的 60% 以下 | 中 | 中 | M0 基准测试 P99 超预算；M3 压测房间密度 < 上限 60% | RuleSet 预编译为字节码；原语内枚举加缓存；兜底手段为单实例改多进程 | M0 基准：`legal_actions` / `apply` P99（§10.5）；M3 压测给出单实例上限 |
| R4 | **规则语法返工**：主版本不兼容，历史 RuleSet 全部失效 | 中 | 高 | 表达第二款族内变体时需要改已有原语语义 | 语法冻结前用"掼蛋 + 族内变体（纸面）+ 麻将（纸面）"做表达力矩阵；发布后只做加法 | 语法冻结评审（M0 末），都走纸面，不做可运行对局 |
| R5 | **商店渠道缺位**：暂不申报版号，移动端只能侧载/TestFlight，拿不到应用商店的自然流量与内购通道 | 高 | 中 | 上线 3 个月后新增用户主要来自 Web 分享、留存依赖熟人传播 | 首发 Web/H5 + 桌面自主分发；用内测数据验证留存与付费意愿，再决定是否为商店渠道把版号排回关键路径 | 每季度看一次渠道机会成本（`A9` 修订版） |
| R6 | **2 人上下文过载**：引擎、DSL、平台、四端、后台五线并行 | 高 | 中 | 同一周内两人都在切换三个以上模块 | 固定 owner（一人引擎 + DSL，一人平台 + 客户端）；crate 边界即职责边界 | 每两周看一次"近两周改动的 crate 分布" |
| R7 | **状态漂移**：双端或恢复后哈希不一致 | 中 | 高 | `statehash_mismatch_total` > 0 | 确定性纪律 + 双端一致性测试 + 重放恢复 | §10.4 每次提交 |
| R8 | **bot 体验差**：AI-L0/AI-L1 太弱，人机房没人玩 | 中 | 中 | 内测反馈"bot 太傻"占比 >30% | 先做配合规则（不压搭档、保炸弹），再谈搜索 | 内测第 2 周做一次小样本问卷 |
| R9 | **四端打包与签名卡住**：macOS 公证、Linux AppImage 依赖 | 中 | 低 | M2 结束时三端桌面产物仍装不上 | CI 在 M2 第一周就接签名与打包；移动端打包随移动壳押后 | 四端冒烟清单（§10.7） |
| R10 | **素材授权问题**：素材不允许商用，或署名/许可文本要求未满足 | 低 | 中 | 审核或法务提出异议 | 优先 CC0；允许商用但有署名要求的（如 OFL 字体）可收，但必须随分发保留许可文本；清单 `../../assets/license.md`（即仓库内 docs/assets/license.md） | M1 前全表可查 |
| R11 | **人机混合房引发不满**：玩家觉得"bot 假装真人" | 中 | 中 | 投诉集中在"对手是机器人" | UI 明确标注 AI 座位；提供"仅真人房"开关 | 内测问卷单项 |
| R12 | **短信通道受阻**：模板报备失败或限额不够 | 低 | 中 | M2 账号链路阻塞 | 测试号白名单兜底；备一家服务商 | M2 前完成报备 |
| R13 | **表达力上限未被验证**：v1 只做出牌族的两款玩法，`window` / 仲裁原语从未被真实玩法压过 | 中 | 高 | v2 接麻将时才发现结构性缺口，需要改原语语义（连带 R4） | v1 不把 `window.open` / `window.arbitrate` 纳入类型与枚举；v2 启动前先做一次"麻将最小对局"纸面推演，确认需要后按 engine 次版本加回并补测 | v2 立项时用纸面规则逐条对照原语清单 |
| R14 | **死锁 / 无法终局**：相位机或规则配置导致某局无法出完 | 中 | 高 | L3 随机对局出现不终局局，报 `RS_NO_TERMINATION` | 终局条件写死在 §5.13「局终止与名次」；L3 跑 200 局闸门，失败留最小动作序列 | §10.2 R14：100% 终局、非法动作率 0 |
| R15 | **结算账不平**：结算事务计数与战绩记录对不上 | 中 | 高 | 每日对账报表条数差异非零 | `settlement_event` 在结算尝试发起时、进入 `settlementId` 事务之前单独插入；死信人工补写（§6.6） | §10.2 R15：幂等键不重复、对账差异 0 |
| R16 | **发布链被绕过**：非法 RuleSet 绕过 L1–L4 闸门进入 canary 或全量 | 中 | 高 | 演练注入非法 RuleSet 未被拦；线上出现闸门外版本 | 发布只走 §5.6 接口，闸门结果落 `ruleset_validation` 留档，审计留痕 | §10.2 R16：被 L2/L3 拦下，人为绕过时 canary 自动回滚 |
| R17 | **客户端兼容断裂**：旧版本客户端进房或列表异常 | 中 | 中 | 旧版冒烟看不到玩法列表或 `minClientVersion` 提示 | 协议只增不删，`minClientVersion` 特性开关（§5.16/§9.2） | §10.2 R17：旧版能看到玩法列表与升级提示 |
| R18 | **定级赛归属错误**：非 `human-ranked` 场次被计入定级或段位（**v2 预留**：定级归属与落位算法随段位体系降 v2，§5.13/§10.2 R18） | 低 | 中 | v1 不跑定级落位判定；v2 立项后按 §5.15 真值表核对 | v1 只保留 `rating`/`rank_level` 字段落库；定级只认 `human-ranked`，`ai-practice` 与自定义房不计 | v2 立项后补 §10.2 R18 集成用例 |
| R19 | **同对手刷分**：重复对局刷高 `rating`（**v2 预留**：重复对局衰减与 K 衰减随 Elo 浮动降 v2，§8.7/§10.2 R19） | 中 | 中 | v1 不实现刷分衰减；v2 立项后观察 `rating` 增量是否集中于少数固定对手对 | v1 不写衰减系数；v2 按 24 小时重复对局 K 衰减处置 | v2 立项后补 §10.2 R19 集成用例 |

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
| U9 | 工期基线怎么定 | 决定能否对外报交付时间 | **已定（2026-11-23，round-05 修订）**：拆到 M0–M4 五段并承诺 40～44 周（因保留 172 条全量测试与八层全量回归，由 34～38 周上调 +6 周），阶段延期必须重算总和 | settled |
| U10 | 掼蛋规则本体谁定 | 决定 M1 能否开工 | **已定（2026-11-23）**：由设计者按淮安主流打法起草《规则取值表》（§5.10），用户逐条确认后冻结 | settled |
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
| D6 | 引擎形态从"每玩法一个 crate"改为"原语库 + RuleSet 解释器" | 运营要零代码上新玩法，规则必须是数据 | 解释执行开销；规则语法一旦发布难改；相对单玩法基线约 +16 周（已并入 §2.4 的 40～44 周）。**生效判据**：v1 上线后 3 个月内存在第 2 款玩法的上线计划，且至少 1 名非研发能独立写出 RuleSet；判据不成立即回落"每玩法一个 crate" | C7、C8、`A14`、`A16` |
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
| D20 | 工期基线 = M0–M4 五段、合计 40～44 周，按阶段承诺（round-05 修订：因保留 172 条全量测试与八层测试全量回归，由 34～38 周上调 +6 周，用户 2026-11-23 拍板） | 单一总数字掩盖了阶段间的依赖；分阶段才能逐段验收 | 阶段间要重算总和，任一段延期都要对外改口径 | U9（2026-11-23，round-05 修订）、§2.4 |
| D21 | 掼蛋规则取值由设计者起草《规则取值表》，用户逐条确认后冻结 | 规则口径（同花顺位次、进贡触发、升级档、过 A 条件）逐条都是可争议的，模糊就写不出引擎 | 冻结前 M1 不能开工；确认过程占用用户时间 | U10（2026-11-23）、§5.10 |

### 12.2 需求变更留痕（本设计在设计期与实现期共被改过十三次，基线因此变了两次，工期口径变了三次）

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
| 第八次变更（规则取值确认） | 2026-11-23，用户逐条拍板 §5.10 七项待确认取值，随表冻结进 `guandan-classic` v3：其中 3 项与原取值不同而实改（同花顺位置改为 `above-bomb-5`——5 张炸之上、6 张炸之下；进贡配对改为对位进贡——末游→头游、三游→二游；抗贡改为贡方两家合计两张大王即可抗贡）；1 项按用户口径放宽并定稿（过 A 由"必须双下"放宽为 1+2 或 1+3 即算过 A，1+4 不算；未过 A 不再退档、继续留在 A 打，同一队连续 3 局在 A 上未过才退回起点 2，为此新增队级状态 `aFailStreak`）；3 项维持原值（还贡、升级档、接风） | §5.1（`compare.flushRank` 新增 `above-bomb-5` 取值、`tribute.pairs[]` 对位配对、`resist` 口径与示例 YAML）、§5.10（7 行状态由待确认改为 `定`、全表冻结）、§5.13（进贡配对、抗贡、比较阶梯、升级跨 A 与过 A）、§5.12/§5.14/§6.5（`aFailStreak` 进 `stateHash` 第 9 项、跨局更新与快照字段集） |
| 第九次变更（round-04 评审修订） | round-04 评审裁决修订（95 条：Blocker 4 / Major 31 / Minor 44 / Nit 16，用户全部 PROPOSE FIX）：`aFailStreak` 统一为「每队一个 u8、取值 0–3，某一局结算时达 3 才退回 `levelTable.initial`（起点 2）并清零」，§5.12/§5.13/§5.14 同步，§5.10 过 A 行的「未过」处置子句随之对齐用户第八次拍板（1+2/1+3 判据与其余取值一字未动）；新增「局终止与名次」——一局在某队两人均已出完时结束，双下收尾按剩余手牌张数（同数按座位号）定 3、4 名，`finishOrder` 即此口径；新增 §5.2「原语参数结构」表（逐原语列字段/类型/取值/默认值）与 §5.3 ADT 变体、L1 参数键校验；`view` 定为 canonical JSON bytes（§5.12 新增「视图编码」六条）并增 `RoomSnapshot.viewSchemaVersion`，删 `view.mode`/`hide` 死分支；`stateHash` 由 9 项改 10 项（新增第 9 项 `aFailStreak`、第 10 项 `finishOrder`）；场次局数上限字段化为 `matchCap.rounds`；定级赛按「场」计 10 场、K 衰减叠加、`ai-practice` 为模式侧别名而落库记 `bot`；发布链 `publish` 只从 `validated` 推进、新增 `RS_NO_ACTIVE_VERSION` 与 `ruleset_release.first_activated_at`、在役解释器由 release 表聚合、canary 崩溃按次数判（0.1% 仅 full 期）；错误码载体三分（连接级走 WS Close code、动作级写 `ActionAck.reason`、HTTP 码仅用于 REST）；`wildcard.source` 改结构化 `{suit, rank}`、`deck.exclude` 纳入总张数公式、`teams` 每座恰属一队、字段必填性图例（✓/○/—）与条件式；`tribute`/`return` 定为引擎自动执行（`SubmitAction` 不再接受、`tribute` 仍为零动作 phase）；§6.3 新增相位转移表、`seatSeq`/`roomSeq` 均从 0 起算、本地存档 `snapshotSeq` 更名 `snapSeq`；AI 实现层改名 AI-L0/AI-L1/AI-L2；§10.2 表尾补 R14–R19 并复用 R7/R3、§11.1 补登、§10.3 全量集去重为 172 条并补齐 `fast20`/`bot_matchup` 选取规则；13 条 DECLINE（段位体系、里程碑容量、发布链简化、`levelInsert`、`flushRank` 两值、`deck.exclude`、schema 表单、`ViewMismatchReport`、补偿表、L4 两码合并、`RS_PHASE_UNIMPLEMENTED`、`timing`/`ai` 移出、`protocol_field` 登记表）不改文档，风险与复查时点记台账 | §1.2/§1.4/§1.6/§2.1/§2.2/§2.4/§4.5；§5.1–§5.16（新增 §5.2 参数结构、§5.3 ADT、§5.12 视图编码、§5.14 零动作 phase 豁免、§5.16 semver 与四平台闭集）；§6.2/§6.3/§6.5/§6.6；§7.2/§7.7；§8.4/§8.6/§8.7；§9.2/§9.4/§9.5；§10.2/§10.3；§11.1/§11.2；§12.2 |

**第七次变更的收尾拍板（同日）**：round-03 修订者留下的 8 条待拍板项由用户逐条拍板——4 项维持现状（①`legalFrom` 检查与 `RS_PHASE_UNIMPLEMENTED` 留在 L2；②安全局数上限达顶时比 `levels` 档位、高者胜、同档平局；③`end_reason` 保留 `aborted`；④台账收敛记录里 round-03 的备注保留改后版本）；4 项改为：段位算法改为「定级赛按胜率分档 + 后续 Elo 式浮动」（§5.8、§5.13、§5.15、§8.7、§10.2、§10.4）；本地存档按模式分流（§2.1、§2.4、§5.9 末行）；原单列的「规则判定写错」并回 R1、不新增编号（§10.2、§11.1）；`publish` 语义写死为「只推进不新建」（§5.6）。

| 时点 | 变更 | 影响 |
| --- | --- | --- |
| 第十次变更（round-05 裁决修订） | round-05 评审裁决修订（21 条全部落实）：新增顶层 `flow` 段（`deal`/`turn`/`score`/`rng`/`assert` 五子对象，整段缺省全取默认值）；§5.12 增 view 字段级编码表、牌 id 载入期固定编码（deck 序 + 副本号）、覆盖全部白名单字段的定值向量（canonical bytes 真算 SHA-256 指纹）并绑 `viewSchemaVersion`；`kind` 闭集收为 `play`/`pass`，`claim`/`declare` 与 `action.claim`/`action.declare` 移出 v1 标 v2 预留，**`pattern.joker-bomb`（四王炸）保留在 v1**；斗地主押后 v2，M3/M4 与 S12/G6 验收对象改为掼蛋变体配置，`seats.teams` 空阵营、`pattern.with-attachment`、`pattern.all-same-suit` 标 v2 预留（`Settle::ScoreOnly` 经用户后续追认保留 v1，见本行补记）；客户端不重算 `viewHash`、删 `ViewMismatchReport` 帧，原 P1 线索并入 P0 服务端自证，`SettleResult` 不携 view；`viewSchemaVersion` 缓存键改 `(rulesetId, viewSchemaVersion)` 二元组、版本号由 RuleSet 作者分配，`RoomSnapshot` 增 `rulesetId`/`rulesetVersion`；发布状态机重写——publish 四步事务（复制 body → 分配 `version` → 锁 `pinned_interpreter_version` → 推进 stage）、新增 `superseded` 状态、active 唯一判定式 `stage=full` 且 `activated_at` 最大、rollback 入参 `badReleaseId` 且候选集排除当前 active 与 `rolled_back` 旧行、补 `rolled_back → canary/full` 合法转移、`ruleset_release` 进 canary 才建行、主键去 `rev`、`draft`/`validated` 由 `ruleset_validation` 聚合派生；结算对账拆两本账，`settlement_event` 在进入 `settlementId` 事务之前单独插入、按 `settlementId` 唯一；段位降固定分档——删 §5.13 定级/Elo/阈值三张表与 §8.7 K 衰减，R18/R19 降 v2，评分只在服务端结算事务整数运算、不进 `stateHash`、不进双端比对；示例 YAML 点数改带引号字符串；原语重复键 `order`/`mode`/`tableRef`/`algorithm` 收归顶层，`level.upgrade` 无参，还贡三键收归 `tribute.return`，L2 增 `levelTable` 唯一引用校验；增 `JoinAck` 帧，错误码载体由三分改四分，`RS_SEAT_TAKEN` 改经 `JoinAck.reason` 并附 `availableSeats[]`；新增 `RS_DECK_COUNT_MISMATCH`/`RS_CAPACITY_INVALID`/`RS_TEAM_COVERAGE_INVALID`；4096 候选峰值闸门挪到 L3 dry-run；`stateHash` 移出版本串，版本三元组旁挂存放、先比版本再比哈希；钉 `m0-minimal` 最小夹具（路径与版本号写入 §2.4，M0 gate 与 A16 改引用它）；写死 `Validation = {ranLevels, findings: Vec<Finding>}`、`validate` 接受 `levels` 入参与 `EngineError` 变体清单；`ladder` 只列非插入型牌型，`straight-flush-*` 位次由 `flushRank` 载入期合成，增两条 L1 规则；`levelTable.upgrade[]` 元素定为 `{teamRanks: [u8;2], step: u8}`，与 `level.pass.combos` 共用编码；AI 三条胜率断言挂到 M2 之后，M0/M1 gate 不受约束；172 条全量集与每晚全量保留，总工期由 34～38 周上调为 **40～44 周**（+6 周，用户 2026-11-23 拍板；M0 第 1–6 周、M1 第 7–16 周不动，M2 第 17–26 周、M3 第 27–36 周、M4 第 37–44 周，M4 收尾落第 44 周）。**补记（2026-11-23 用户裁决，并入本行不留新变更号）**：`Settle::ScoreOnly` 恢复为 **v1 在役变体**——它不只服务斗地主，是无阵营玩法与 `m0-minimal` 夹具唯一的最小结算变体，`m0-minimal` 因此改走真实 DSL 结算路径；斗地主其余部分仍押 v2。同时追认：§5.10 冻结表的**字段列**允许实效化改名（`tribute.returnPolicy` → `tribute.return` 三键），取值与状态逐字不动、不视为违反冻结。第 4 项所列 11 条相邻 Minor/Nit 经全文 grep 实证无残渣（无旧值域、无指向已删字段的断裂引用、无同一规格两套说法），维持 `DECLINE` + 复查时点 `round-06-lite`。**round-06-lite 补记（2026-11-23，仍并入本行、不开新变更号）**：轻量轮（仅 Step9 就绪度单步）11 条 = 0 Blocker / 3 Major / 6 Minor / 2 Nit；3 条 Major 均属**补缺类**——F9-01 `§5.1` `tribute` 类型列补 `return`（同段补 `pay`/`firstRound` 承载位、并写明 `enabled=true` 时 `return` 三键必填）、F9-02 `flow.deal` 补 `zone` 承载位（`perSeat`/`reveal` 默认规则写清，`deal.draw`/`deal.reveal` 标 v1 未启用并写明理由）、F9-03 顶层补 `level` 段（`pass.combos`/`streakLimit`/`resetOnFail` 与 `current.scope`，`levelTable` 不动）；三条只把已裁决内容补到字段级承载、**不引入任何新的设计决策**。用户裁决「补缺类 Major 修完即视为通过」，故本轮 3 条 `Major` 按 `settled` 结、8 条 Minor/Nit 按 `DECLINE` 结（复查时点 M0 末第 6 周）；`clean` 仍按报告口径判 `否`，不因本补记改为 `是`。顺带收敛的两处口径：`streakLimit` 取值收敛为 1–3（上界与 `aFailStreak` 冻结值域 0–3 同源）、§5.13 过 A 判定显式声明读顶层 `level.pass`（四条口径原文不变）。同批随附落地的 8 条 Minor/Nit 修正（`flow` 键与原语字段的映射表、接风承载位统一为 `flow.turn.partnerLead`、L2 两条检查的码口径、v1 段位最小口径、`Finding`/`ErrorCode`/`Severity` 具名类型与 `EngineError` 两个变体、`stateHash` 标题改为「9 段拼接 + 1 项旁挂」、`seat.levels` 明确为下标、§5.2 与 §12.2 两处断表归位）**不改变任何裁决与取值口径**，按用户口径这 8 条仍记 `DECLINE`、不再回改；本条不新开变更号，故**当时**不设「第十一次变更」行（该号后由 2026-11-24 的 M0 实现期口径对齐占用，见本表末行） | §1.2/§1.4/§1.5/§2.1/§2.4/§3.3/§4.3/§4.4；§5.1/§5.2/§5.4/§5.6/§5.7/§5.8/§5.9/§5.10/§5.12/§5.13/§5.15/§5.16；§6.5/§6.6；§7.2/§7.6；§8.6/§8.7；§9.2/§9.4/§9.5；§10.2/§10.4；§11.1/§11.2；§12.1/§12.2 |
| 第十一次变更（M0 实现期口径对齐） | 2026-11-24，M0 第一批（契约骨架 + L1 校验 + `m0-minimal` 夹具 + `stateHash` 定值向量）落码后，实现端暴露 8 处文档内部矛盾或缺口。用户回复「继续」、**未逐条明示表态**，故先按主会话逐条建议暂定为执行口径，本行如实记录，不含任何未发生的表态；**追认（2026-11-24）：用户随后就该 8 条逐条表态，8 条全部「确认」（8/8），故口径由「暂定」转为「已追认、生效」**；若日后任一条被推翻，按本行倒序回退。① `legal_actions` 返回 `Result<Vec<Action>, EngineError>`（§2.4 原写裸 `Vec`，与 §5.4 把 `QuotaExceeded` 列为它的错误出口直接冲突）；② `level.pass.streakLimit` 值域统一 **1–3**（§5.4 原写 1–9 属笔误，§5.1/§5.2 与 `aFailStreak` 冻结值域 0–3 均为 1–3）；③ `flushRank` 插入位改**惰性校验**（RuleSet 无插入型牌型时不校验）；**锚点口径已于 2026-11-24 由用户定稿**：炸弹大小由张数决定（同点数张数越多越大），同花顺位次为「大于 5 张炸、小于 6 张炸」（即 `count-then-rank` + `above-bomb-5`，均为 §5.10 冻结现值，取值未动），故锚点按 `primitiveId = pattern.bomb` + `params.count` **语义识别**、不看 `id` 名字；锚点缺失 → 拒绝加载并报新码 `RS_FLUSH_ANCHOR_MISSING`，位次冲突 → 拒绝加载并报新码 `RS_LADDER_CONFLICT`，**两码分开报、均拒绝、不降级**（用户 2026-11-24 选择）；原 §5.1/§5.4 无条件要求冲突即拒、且与 `m0-minimal` 无锚点自相矛盾的问题随之关闭；**同批补充定稿（2026-11-24 用户裁决）**：`bombOrder` 的两级比较写成可判定口径——先比张数（多者大）、张数相同比点数，且**点数按载入期合成牌力序**（`naturalOrder` + `levelInsert`，级牌高于 A）比较，`rank-then-count` 为对照取值；§5.10 炸弹序行的取值与状态逐字未动；④ `score-only` 局的 `seat.levels` 取空序列且该键不出现（§5.12 字段级编码表原已如此，此处追认为**双端同解**口径，禁止一侧填占位值）；⑤ `ai.hint` 定为 `bool`（默认 `false`），是提示开关而非难度档位（原 §5.1 只给字段名、未给类型）；⑥ `tribute.enabled=false` 时其余子键可整段省略（原仅写「整段不运行」，未定键集）；⑦ §5.12 固化**变长序列长度前缀宽度**——元素个数 `u8`、牌数组与字符串 `u16`（原未规定，两端各写一套必然对不上）；⑧ `State` 直接承载 §5.12 九段字段，`CanonicalState` 为 `From<&State>` 单向只读投影（原只定义 `CanonicalState`、`State` 为占位，M1 的 gate 无法接上）。**这 8 条全部是实现期暴露的文档内部矛盾或缺口，不引入任何新的设计方向与取值**；`§5.10` 冻结表的取值与状态逐字未动 | §2.4（四接口签名）；§5.1（`compare`/`tribute`/`ai` 三行）；§5.4（L1 行）；§5.12（九段拼接、规范化与序列化） |
| 第十二次变更（M0 实现期：纸面推演缺口补齐与随机源固化） | 2026-11-24，M0 gate 第 2 项纸面推演（`docs/assets/m0-paper-walkthrough.md`，444 行，已入仓）列出 **14 条 schema 缺口 + 4 条校验/执行层欠债**，其中 2 条判定**阻断 M1**；另有 PRNG／洗牌／发牌顺序三项设计**从未规定**（N7/N8/N9），而它们会被 §10.3 黄金向量锁死。主会话就三件事逐条提选项、用户当日逐条选定，本行按原话记录：①随机源 →「**BLAKE3(seed‖counter) 派生**」；②两条阻断 M1 的缺口 →「**两条都现在补**」；③交互草图形态 →「**文本 + ASCII 线框**」（草图产物见 `docs/assets/m0-editor-interaction-sketch.md`，属 §2.4 gate 5 后半）。据此落三组改动，**§5.10 冻结表的取值与状态逐字未动**：**(a) 名次类型参数化**——`teamRanks` 由 `[u8;2]` 定长、名次闭集 1–4，改为 `[u8; teamSize]`、名次空间 `1..=seats.count`（长度必须等于该队队员数、元素互不相同且升序），`level.pass.combos` 与 `result.ranks`/`finishOrder`/`round_record.players[].rank` 同一口径；`tribute.pairs[].payer`/`receiver` 由 `first`~`fourth` 枚举改数值 `1..=seats.count`；§5.13 局终止由"某一队两人均出完"改为"该队**全员**出完"，剩余名次按剩余手牌张数升序依次填入。**(b) 升级判定扩展**——新增 `levelTable.upgradeBasis: rank-combo \| score-diff`；`rank-combo` 保持现值（`upgrade[]` 的 `teamRanks` 参数化，`step` 值域恢复为 **0–3**，`0` 表该组合不升档，允许枚举满全部组合）；`score-diff` 新增 `rankScores: [u16]`（长度 = `seats.count`）与 `upgradeByDiff: [{minDiff: u16, step: u8}]`（按 `minDiff` 降序、必须含 `minDiff: 0` 兜底、要求 `flow.score.dimension = team`），两 basis 的键集互斥、同时出现即 L1 拒绝；分值表作为 `levelTable` 的静态子键恢复，**不是把已移出 v1 的 `score.table` 原语拉回**。**(c) 随机源固化**——§5.2 新增「发牌随机源的确定性定义」：随机字 = `BLAKE3-256(seed_le64 ‖ stream_le32 ‖ counter_le64)` 的前 8 字节，`bounded(n)` 用 `⌊2^64/n⌋*n` 拒绝采样，洗牌为 Fisher-Yates 降序，按座位序号连续切分；`counter` 含被拒绝的那一次、禁止批量/并行预取；`deal.shuffle` 与 `rng.pick` 共用同一 `(seed, stream, counter)` 序列。**(d) 顺带把 M0 gate 4 实测数字写回 §10.5**（附"`m0-minimal` 是地板、`guandan-classic` 须在 M1 签核前重跑"的限制说明）。**未补的两条**（推演另列的 `flow.turn.firstLead` 与 `settle.termination` 键）按此前处置**押后、M0 不加键**；局终止判据仍按 §5.13 正文 | §5.1（`levelTable`/`level`/`tribute` 三行与示例 YAML）；§5.2（`level.pass` 参数行、新增随机源定义节）；§5.4（L1 行）；§5.12（第 8 项 `counter`/`seedDigest` 与新定义对接）；§5.13（发牌、局终止与名次、升级档表、结算执行顺序 ④）；§10.5（基准表回填）；§12.2 |

| 时点 | 变更 | 影响 |
| --- | --- | --- |
| 第十三次变更（M0 收尾：实测数字补录 + 必填性口径对齐） | 2026-11-24，M0 gate 第 3、4 项收尾。**本条不含任何新的设计决策，只有①实测数字回填与②已裁决口径的对齐**：①§10.5 第 5、6 行由「待实测（M0 末第 6 周补）」改为实测值——1500 房间（按 A4 峰值 ×3 倍余量，行标题由「1000 房间空转」改为「1500 房间空转」）常驻状态堆内存 1,065,000 B / 710 B 每房间、空转 tick CPU 0.803 ns/(房间·tick)（1500 房间 10Hz ≈12.0 µs/s）；wasm（HEAD `061ae60`、默认 release profile、未改 strip/lto）原始 678,604 B、gzip 后 189,972 B（27.99%），两口径均 ≤1.5MB 目标线；两行均补写测量口径并以"地板值"标注（空转 CPU 不含 `legal_actions`/`apply` 与网络 IO，内存是分配器差值而非 RSS）；②§5.13 「tribute」段补一段必填性声明，把此前本节未重申必填性、实现侧按"整段可省"落地的双口径统一到 §5.1 的写法（`enabled: true` 时 `return` 必填、整段缺省即 L1 拒绝；`enabled: false` 时只写一个键即合法），与第十一次变更⑥同源、未新增契约。同批实现侧据此落 L1 值域闸门（`pattern.bomb` 4–8〔`count: 12` 由静默通过改为拒绝〕、六类牌型参数值域、六条原语参数值域、`tribute.pairs` 名次数值域与 `tribute` 必填性），`m0-minimal` 锚点 `e09079f1…bedd` / steps 150 / finishOrder `[1,3,2,0]` 逐字未变。**§5.10 冻结表的取值与状态逐字未动** | §5.13（`tribute` 段必填性声明）；§10.5（第 5、6 行实测值与测量口径、表后"地板"说明）；§12.2 |
| 第十四次变更（M0 收尾：`perSeat` 两侧值域对齐） | 2026-11-24，M0 收尾补 `flow.deal.perSeat` 顶层 1–108 值域闸门时，实现侧报出同一量的两侧上界不一致：§5.1 顶层写 1–108，§5.2 `deal.shuffle.perSeat` 写 1–27，且 §5.1 的键名映射表漏了 `flow.deal.perSeat`↔`deal.shuffle.perSeat` 这一行；实现侧未自行裁决、交回主会话。用户当日选定**方案 A**：「把 §5.2 原语侧放宽到 108（27 作为四座真值仍在域内，对将来 3/5 座扩展更稳）」。据此落两处改动：①§5.2 `deal.shuffle.perSeat` 值域 1–27 → **1–108**，并写明它与顶层是同一个量（"每座位分得张数"，不是"每轮每人张数"）、四座真值 27 仍落域内、上界放宽是为三座／五座变体免改语法；②§5.1 键名映射表补 `flow.deal.perSeat`↔`deal.shuffle.perSeat`，顶层值域行补一句「两侧同域、守恒属 L2」。**未动**：默认值公式（`deck` 总张数 ÷ `seats.count` 的整除值）、不整除必须显式声明的规则、`m0-minimal` 与 `guandan-classic` 两份夹具的取值、§5.10 冻结表的取值与状态。实现侧已落的顶层 1–108 闸门（含 0／109 反证、1／27／108 正例）与本变更方向一致，无需回改；原语侧闸门由 1–27 改为 1–108 属本次的落码动作 | §5.1（`flow.deal.perSeat` 行、键名映射）；§5.2（`deal.shuffle` 参数行）；§12.2 |
| 第十五次变更（M1 开工：逢人配口径与同花顺可用） | 2026-11-24，M1 第一条（牌型全表）落地时，实现侧报告 §5.13「逢人配」行只写了「可替代除大小王外任意一张牌 + `inStraightFlush=false` 不得用于同花顺」，而 `guandan-classic` 首局在第 136 步（`turnSeat=3`、领出态、剩 1 张红桃级牌）因百搭被整体剔除而出现空候选死锁，五个判定空白必须定死才能跑完一局。用户当日裁决两句：①RNG 契约「**改实现，对齐文档**」（§5.2 的 20 字节 `seed_le64‖stream_le32‖counter_le64` + `bounded()` 拒绝采样 + Fisher-Yates 降序为准，实现侧现用的 16 字节 + 取模须重写，锚点随之重算——属实现对齐、不改文档）；②逢人配套餐「**多张可用 + 最大解 + 同花顺可用**」。据此落三处：**(a)** §5.13 逢人配行改写为可判定口径——百搭张数不限（含整手全百搭，此时按自身级牌点数使用）、含百搭时取「可达解中 `rankKey` 最大者」、`rankKey` 相同再按「真牌点数组合字典序最小」定唯一解（顺子多孔多解同此），百搭不得替代大小王、`joker-bomb` 不接受百搭；**(b)** §5.10「逢人配能否组同花顺」取值由 `false` 改 **`true`**（§5.10 唯一一处取值变更，用户明确裁定）；**(c)** §5.1 示例 YAML 的 `inStraightFlush` 同步为 `true`。**未动**：`tribute`/`level`/`compare` 各项冻结取值、`pattern.joker-bomb` 无参、炸弹上界 8（G9/G10 仍属六人变体 backlog）。同批实现侧另修一处跨型压制 bug（`beats` 曾把 `compare.ladder` 当跨型合法性表，致对子可压单张），收敛为「同型比 key / 炸弹族内按 ladder / 炸弹压非炸弹 / 其余不可压」四分支，属实现缺陷、非设计变更 | §5.1（示例 YAML `wildcard` 行）；§5.10（逢人配能否组同花顺行）；§5.13（逢人配行）；§12.2 |
| 第十六次变更（打级队口径：B3 裁决） | 2026-11-24，M1 实现期 B3（"两队级牌不同时牌力序按谁的"）一直 hold。用户原话："首局由系统随机抽一张牌分到谁就由这对先出并且打这对的级牌，后续就是谁赢就打谁的级牌，依次升级直到过 A"。据此把「每队各一套 ladder」改为「每局单一局级牌」的口径：①每局只有一份局级牌，取 `levels[attackingTeam]`，牌力序按这一份合成（`naturalOrder` + `levelInsert`），全场统一、可传递，不必再在比较时判定方向；②`attackingTeam` = 上一局头游所在队（"谁赢打谁的级牌"），场次首局无上局名次，改为**抽牌前置**：`deal.shuffle` **之前**先取一次 `bounded(总张数)` 抽出一张牌（牌不剔出牌堆），洗牌与连续切分发完之后看这张牌落在哪个座位，该座位先出、其所属队即 `attackingTeam`；非首局不抽牌；③`levels` 仍每队各存（各自升档与记账），`aFailStreak` 不动；④新增状态字段 `attackingTeam` 并**进 `stateHash` 第 11 项**（段数 9→10），快照字段集同步；⑤删除原文「v1 固定座位 0 先出」。**抽牌口径（用户 2026-11-24 当日追认，本行内补记、不另开变更号）**：用户选 A（抽牌定持有者）并要求**抽牌前置**——先抽一张牌、再洗牌发牌、最后「落到谁就是谁」。落码定为：`deal.shuffle` 之前先 `bounded(总张数)` 取牌 id 下标，洗牌与切分之后按牌 id 定位持牌座位；该牌不剔出牌堆、正常参与本局；非首局不抽牌、不消耗随机字。由此 `counter` 的消耗序整体前移一个字，锚点按最终口径一次性重算。**未动**：§5.10 其余取值、升级档表、过 A 判定（1+2 或 1+3）与 `aFailStreak` 阈值 | §5.10（级牌行）；§5.12（哈希段标题、第 11 项、快照字段集）；§5.13（决定首出行）；§12.2 |
| 第十七次变更（轮子顺恒最小：B5 裁决） | 2026-11-24，M1 实现期 B5（`A-2-3-4-5` 的 `rankKey` 取 5 还是 A）一直 hold。用户裁决两句：①「选 a」——沿用「`rankKey` = 该顺子按 `naturalOrder`（含级牌插入）的最高一张」，不做另一套原始序；②「12345 非同花不管包不包含级牌都是最小牌不用特殊处理」——即 `straight-5` 的结构为 `A-2-3-4-5`（轮子）时，无论是否含级牌、是否由百搭补位，都恒为杂顺最小。据此落：**`straight-5` 的轮子 `rankKey` 恒取保留值 `0`**（小于任何其它顺子、不受 `levelInsert` 提升；`0` 为保留值，真实点数不得映射到 `0`，其它牌型不得取 `0`）；**`straight-flush-5` 的 `A-2-3-4-5` 不适用**，同花顺族内仍按 `naturalOrder` + `levelInsert` 顶牌取 `rankKey`，与炸弹的位次只由 `flushRank` 决定。**未动**：顺子结构规则（A 可前接 2、不可绕回、王不入顺）、`levelInsert` 合成算法、`flushRank` 与阶梯、`bombOrder`、逢人配取解规则 | §5.13（顺子边界行）；§12.2 |
| 第十八次变更（三带二进 v1：B2 裁决） | 2026-11-24，M1 实现期 B2 一直是**文档内部打架**：§5.2 把带牌类能力（`pattern.with-attachment`）标为 v2 预留、RuleSet 一引用即 L1 拒，而 `docs/开发计划.md` 的 M1 行又列着三带二。用户裁定："三带二进进 v1，掼蛋有三带二，大小判断为 3 张的那张大小同单张一样，跟带的二是什么没关系"。据此：①**三带二进 v1**，新增固定形状原语 `pattern.triple-pair`（无参，只能为空对象 `{}`，固定 3 张同点 + 1 对共 5 张）；`pattern.with-attachment` **仍留 v2**，只服务"带牌数与主牌数都可配"的玩法（三带一、四带二）；②牌型 id 取 **`triple-pair`**，加入 §5.13 牌型行与 §5.1 示例 YAML 的 `patterns`、`ladder`（位次在 `triple` 之后、`straight-5` 之前）；③比较口径：`countKey = 5`、`rankKey` = **主牌三张的点数**（按 `naturalOrder` + `levelInsert`，级牌做主牌时同样高于 A），**所带的对子不进 `rankKey`**、带哪一对都不改强弱；与其它非炸弹非插入型牌型互不可压，只有炸弹与同花顺按阶梯可压它；④带的对子允许任意同点数两张（双副牌下王成对亦可），不得是两张不同点数；⑤百搭可代主牌或对子任一张，取解沿用逢人配的"`rankKey` 最大、再真牌字典序最小"；⑥新增识别消歧通用规则：**同一手牌同时可判成多个牌型（含百搭多解）时，取 `compare.ladder` 位次最高者**；⑦§5.10 冻结表相应行由「单张/对/三/顺子/三连对/二连三」扩为含三带二，**其余 15 行取值逐字未动**。**未动**：顺子结构、`flushRank` 与阶梯、`bombOrder`、逢人配张数与取解、`countKey`/`rankKey` 的既有定义 | §5.1（示例 YAML 的 `patterns` 与 `compare.ladder`）；§5.2（分类表、v2 预留说明、参数表新增行）；§5.10（牌型行）；§5.13（牌型行、新增三带二行、逢人配行）；§12.2 |
| 第十九次变更（`legal_actions` 规范子集：B6 裁决） | 2026-11-24，M1 实现期 B6 一直 hold：`legal_actions` 现在每个点数只发一条规范子集，但"只发一条"这件事本身没有写进设计稿，实现侧无法判定合法与否。用户选 **B（只列规范子集）**，并要求另配一套「组牌规范」做整手推荐。本条只落前者：**同一 `patternId` 与同一 `patternKey` 只列一条**，规范代表的选定序为①可达解中 `rankKey` 最大 → ②百搭用量最少 → ③真牌牌 id 升序字典序最小；客户端可用任意结构等价的真牌／百搭组合出牌，`apply` 按结构复检、不受规范子集约束；跨端只保证每种牌型有可提交候选、不保证覆盖全部组合。**组牌推荐（整手拆分 + 同花顺提示）**：用户当日给出评分取向——炸弹越大分越高（4→4…8→8、王炸 9）、顺子与三带二越大分越高、对子不得分、单张扣分，按此求"最优推荐"、允许用户自由组牌；具体目标函数形式与常数尚未拍板，**本条不落其规格**，待下一条变更。**未动**：排序键四元组、4096 峰值闸门、基准表 | §5.12（规范子集段、`State` 段数表述）；§12.2 |
| 第二十次变更（组牌推荐进 DSL：B6 后半） | 2026-11-24，B6 裁决「只列规范子集」之后用户补一条需求："最好定义一套组排规范，让服务端一键推荐组牌、提示是否有同花顺等……按照这个逻辑组一套最有牌推荐，用户不同意可以自己自由组牌"，并选定 **C（权重写进 DSL）**、"但 DSL 里默认用 A 的规则"（即默认权重 = 用户给定的评分取向：炸弹 4→4…8→8、王炸 9、顺子与三带二越大分越高、对子 0 分、单张扣分）。据此落：①§5.1 `ai` 段扩为 `{features[], hint, recommend}`，`recommend = {enabled, weights}`，`weights` 全整数（禁浮点）；②新增只读接口 `recommend(&State, Seat) -> Result<Layout, EngineError>`（§2.4/§5.11），纯函数、不进 `stateHash`、不改 `legal_actions`；③`Layout` 形状与 `straightFlushCandidates`（含百搭方案、独立于最优拆分、`hasStraightFlush` 即候选非空）；④评分公式全整数：炸弹按张数权重、王炸 9、同花顺 6、顺子/三带二/三连对/钢板 = 基础分 + `⌊bonus ×(rankKey−1)/(N−1)⌋` 位次加分、对子 0、单张 −1；⑤默认权重表逐项写入（用户取向原样），已知 `straightFlush=6` 与 `bomb6=6` 同分、由 tie-break 决定先后；⑥tie-break 固定五级（分数 → 散张少 → 炸弹组总张数多 → ladder 位次降的字典序 → 枚举序最先），无随机；⑦搜索用确定性束搜索 `beamWidth = 64`（写死引擎、不进 RuleSet），束宽变更只影响推荐向量、不影响 `stateHash`；⑧预算 ≤2ms P99，超预算只记 P2 指标、不判错。**未动**：`stateHash` 段集与段数、`legal_actions` 排序键与规范子集、§5.10 冻结表全部取值 | §2.4（接口清单）；§5.1（`ai` 行与示例 YAML 可省略）；§5.11（新增「组牌推荐」段）；§12.2 |
| 第二十一次变更（整手全百搭边界闭合） | 2026-11-24，M1 实现期最后一处设计空白：B1 已定"含整手全百搭按自身级牌点数使用"，但未说"张数如何映射到牌型"，而本副牌百搭只有 2 张、">2 张全百搭"物理不可达，实现侧无从判定。用户裁决 **A（现在就闭合）**。据此定：整手全百搭一律按自身点数取同一 `rankKey`，**张数直接决定牌型**（1 `single`、2 `pair`、3 `triple`、4–8 `bomb-N`），不得判成顺子/三连对/钢板/三带二/同花顺（同 `rankKey` 既无连续也无异花色）；该条与 `deck.copies` 无关，多副牌同样适用、不另设上限；张数超出牌型表已声明值域（>8 张或未声明该张数炸弹）按 `IllegalAction` 拒，不降级。**未动**：逢人配张数不限、`rankKey` 取解序、`joker-bomb` 不收百搭、`wildcard.inStraightFlush=true`、其余 §5.10 取值 | §5.13（逢人配行）；§12.2 |
| 第二十二次变更（视图补全：局级牌与打级队进视图） | 2026-11-24，B3 落地后暴露的连带缺口：白名单只有 `seat.levels`（两队档位），而 B3 之后**只有打级队那一份参与牌力**，客户端要渲染"级牌是哪张"必须知道是哪一队、哪一档；若让客户端自行推理，断线重连与中途加入的座位推不出 `attackingTeam`，渲染错牌而服务端 `viewHash` 自证查不出来。用户选 **B（直接下发）**。据此：①`view.fields` 允许的写法由三类扩到五类，新增 `round.level`（本局局级牌档位下标 = `levels[attackingTeam]`）与 `round.attackingTeam`（本局打级队序号）；②两者都是桌面公开信息（谁先出、谁打级人人可见），不构成信息泄露；③示例 YAML 的白名单按"只增不改序"追加两键、`view.schemaVersion` 由 1 升 **2**；④§5.12 字段级编码表新增两行（`u8` 单值、无级牌状态与 `score-only` 局该键不出现）；⑤定值向量随白名单序重算为 169 字节 canonical JSON、SHA-256 `36e7bdc6…ab51`（旧向量 `9e0b756d…cc72`、129 字节作废）；⑥`viewHash` 全部既有值随之作废，需随实现一并重算。**未动**：`stateHash` 段集与段数、`RoomSnapshot` 字段集、`lastPlay` 与 zone 编码、`seat.levels` 口径 | §5.1（`view` 行、示例 YAML）；§5.12（字段级编码表、定值向量）；§12.2 |
| 第二十三次变更（跨局序号连续口径） | 2026-11-24，M1 的"再来一局"硬 gate 会撞上这条：一局结束进入下一局时 `roomSeq`/`seatSeq` 是否归零，设计稿只写了"均从 0 起算"，没说跨局。用户选 **A（连续、不归零）**。据此：①两个序号**在每场（match）内跨局连续递增**，只有新一场（`matchId` 变化）才从 0 起算；`snapSeq` 与 `roomSeq` 同一序列、同样连续；②**局切换不靠序号归零判断**，由 `SettleResult.roundNo` 与下一局 `DealEvent` 标识，客户端只需"严格递增"一条规则；③`ActionRecord` 与 `actions.jsonl` 增 `roundNo: u16`，回放按它切段；④旧口径"每个序号每局从 0 起算"作废。**未动**：`STALE_SEQ` 只按座位水位判定、`roomSeq`/`seatSeq` 字段名分工、重放水位一律用 `roomSeq`、`snapSeq` 与 `roomSeq` 同源 | §6.2（序号作用域）；§6.5（动作记录）；§9.4（回放包 `actions.jsonl`）；§12.2 |
| 第二十四次变更（三类协议载体冻结） | 2026-11-24，M0 backlog 里挂着的三个待冻结载体（`SettleResult.result` 编码、`reason` 类型、`rulesetVersion` 载体）一直是实现侧的 TODO 项。用户裁定 **A（一次性全冻结）**：①`SettleResult.result` 定为 `bytes` + canonical JSON，与 §5.12 视图编码同一六条规则，结构同 `round_record.result`（`{winner_team, ranks, points}`），新增字段只能追加在 `points` 之后、不改序不改名；②`ActionAck.reason`/`SettleResult.reason` 定为 ASCII 字符串、取值必须来自 §5.7 全表，表外即服务端违约（P0）；正常时字段不出现；③`rulesetVersion` 定为 `string`，字符集 `[A-Za-z0-9._-]`、长度 1–32，只作身份键、**禁止按字符串比大小**，仅用于版本三元组相等判定；④`SettleResult` 帧增可选 `reason?`。**未动**：帧集与其余字段、`stateHash`、视图编码、错误码表取值 | §6.5（协议帧表与「三类载体」块）；§12.2 |
| 第二十五次变更（级牌入组牌型：只按自身点数位） | 2026-11-24，实现侧报告 §5.13 存在**文档内部冲突**：「顺子边界」行写「级牌在顺子里按自然点数参与」，而「级牌」行写「在单张/对/三张/**顺子**里按高于 A、低于小王的牌力参与」，两行互斥；按后者实现会让含级牌的顺子/连对/钢板在合成序上连续性断裂（级牌跳去顶位），牌型实际不可达。用户裁决：**级牌参与组牌型时只有自身点数位，不存在 A 位**（原话「级牌非红配牌组顺子的时候就只有自身点数位的情况不存在 A 位」），并同时确认张数口径：**顺子永远 5 张、三连对只能 3 对（6 张）、组牌型最多 6 张**。据此落：①级牌在**散牌型**（单张/对/三张/三带二主牌）仍按合成牌力序「高于 A、低于小王」；②级牌在**组牌型**（顺子/三连对/钢板/同花顺）只按**自身自然点数位**参与，既不存在「当 A」的第二解，也不因是级牌而额外加权——故同一张级牌在两种语境下位次不同，**但两者只在各自 `patternId` 内比较**，跨型只按 `compare.ladder`，不做数值比较；③`straight-5` 的轮子 `rankKey = 0` 特例不变（B5），`straight-flush-5` 的轮子仍不适用；④张数上限：顺子固定 5、`plate-3`/`steel-2` 固定 6，**组牌型最长 6 张**，超出按 `IllegalAction` 拒；炸弹 4–8 与 `joker-bomb`、`triple-pair`（5 张）不受此条约束；⑤删除 §5.13 级牌行原「顺子」二字（改为「三带二的主牌」）。**未动**：`naturalOrder`/`levelInsert` 合成算法、炸弹 `bombOrder` 与 `flushRank` 阶梯、逢人配取解规则、`§5.10` 其余行取值、`stateHash` 段集。**代价**：含级牌的组牌型由不可达变为可达，`guandan-classic` 锚点与基准须在落码后重算一次 | §5.10（级牌行、牌型行）；§5.13（级牌行、顺子边界行）；§12.2 |
| 第二十六次变更（接风口径订正 + 双下名次改按座位方向） | 2026-11-24，实现侧报告两处待追认：①§5.10/§5.13「接风」行写成「若其**队友**是最后压牌者则该队友领出」，与通行规则相反（应为**出完牌的本人**是最后压牌者、无人能压→**其搭档**领出）；②§5.13「局终止与名次」原用「剩余手牌张数升序」排双下时的 3、4 名，用户改按座位方向。用户当日描述：「如果是队友出完牌了而且对方都不要的情况下，接风给另一个队友，如果有对方接牌则按照出牌顺序继续行牌；如果上游二游走完，则判断头游的下一位对手是末游（4游），其他情况就是正常的出完顺序」，并就「是否保留 `afterAllPass: next-player`」明确「**保留 next-player 作为可选玩法**」、就双下名次选 **A（座位方向）**。据此落：①两处接风行订正为「出完最后一手牌的本人是最后压牌者、所有仍有牌座位都过 → 其搭档领出」，并写明「有人压过则接风不成立、按正常顺序继续」与防御分支「搭档亦已出完 → 沿方向的下一个仍有牌座位领出」；②新增「全过后领出」判定顺序行：接风优先，接风不成立才读 `flow.turn.afterAllPass`（`last-player` 默认 / `next-player` 可选玩法，均从最后出牌者起算、跳过已出完座位）；③`turn.return-to-last` 与 `afterAllPass: last-player` 判为**同义共存**，不再视为冲突、L1 不报错，显式取值优先于原语默认（原「两键对同一事件给相反答案」的歧义随之关闭，取消上一轮拟定的 L1 互斥校验）；④双下（未出完座位全属同一队）名次改按座位方向：从头游起沿 `flow.turn.direction` 第一个遇到的未出完座位记**末游**、第二个记 `seats.count−1` 名、依次倒排；四人不变量即「头游的下一位对手 = 末游、另一对手 = 三游」；⑤明确**不得再用剩余手牌张数**排名次（张数不进判定链、不进视图白名单），并写明名次分配不影响 `step`（`rankCombo=[1,2]` 恒按升级档取），只影响 `round_record`、下一局进贡角色与战绩。**未动**：`flow.turn.direction`/`skipFinished`、`partnerLead` 默认值、`afterAllPass` 默认值、升级档表、过 A 判定、`stateHash` 段集。**代价**：接风与双下名次的判定变了，`guandan-classic` 若有双下或接风轨迹则锚点随之漂移，须重算；`finishOrder` 口径不变（仍只列已出完座位、按出完先后） | §5.2（`flow.turn.afterAllPass` 行）；§5.13（接风行、新增「全过后领出」行、局终止与名次段）；§12.2 |
| 第二十七次变更（`consume-cards` 的 `count > 0` 标 v2 预留） | 2026-11-24，实现侧报 §5.2 参数表只写了 `count: u8` 取 0–8、默认 0（0 = 按本次出牌张数），但 `count > 0` 时**消耗哪几张牌**从未规定，而 v1 所有现存 RuleSet 都没引用 `action.consume-cards`（`guandan-classic` 走的是 `action.require-legal` 里的 `consume: hand`，即 `count = 0` 路径）。用户裁定 **A（v1 锁定 0）**。据此落：①`count` 在 v1 **只允许 `0`**，非 `0` 一律 L1 拒绝、报 `RS_SCHEMA_INVALID`；②`count > 0` 的「固定消耗 N 张」整段语义**标 v2 预留**，留待真有「出牌付代价」类玩法时再定——届时必须一并定「哪几张」（引擎自动选 vs 客户端 payload 指定 vs 混合）与 `legal_actions` 的候选形态，现在不猜；③§5.2 分类表的行动行补注这一限定。**未动**：`zone` 必填性、`action.require-legal` 的 `force`/`consume` 默认值、`action.pass`、`stateHash` 段集、v2 预留清单其余项 | §5.2（行动分类表、参数表 `action.consume-cards` 行）；§12.2 |
| 第二十八次变更（`recommend` 权重刻度放大到 10） | 2026-11-24，用户提出「同花顺大于 5 炸小于 6 炸怎么设定」——第二十次变更定的默认表是 1 分一档（`bomb5=5`、`bomb6=6`），整数刻度下两档之间**没有可用整数**，所以同花顺只能取 6 与 `bomb6` 同分。用户裁定 **A（全表 ×10）**。据此：①§5.11 默认权重表整体改为`bomb4..8 = 40/50/60/70/80`、`jokerBomb = 90`、**`straightFlush = 55`**、`straightBase = 40`、`straightRankBonus = 20`、`triplePairBase = 30`、`triplePairRankBonus = 20`、`plate3Base = 30`、`plate3RankBonus = 10`、`steel2Base = 40`、`steel2RankBonus = 10`、`pair = 0`、`single = −10`；②新增「权重刻度 = 10」的书面约定（表内数字是「基准分 × 10」，引擎按原值直接比较、不再换算；`weights` 仍是整数 `i16`、禁浮点），刻度只改可表达精度、不改任何相对次序；③同花顺由此**严格落在 5 炸与 6 炸之间**（`50 < 55 < 60`），与真实牌力序一致，与任何牌型都不再同分；④`rankBonus` 的 `bonus` 同按刻度 10，求和与向下取整仍全整数，取整损失小于 1 分一档；⑤L1 补一条：作者自定义权重必须**显式给全所有键**（不允许只写一半），且不校验 `straightFlush` 与炸弹档的强弱关系（权重是启发式排序分、不是牌力强度序）。**未动**：`Layout` 形状、tie-break 五级序、`beamWidth = 64`、≤2ms P99 预算、`recommend` 纯函数与不进 `stateHash` 的定位、评分公式结构。**代价**：`Layout.score` 数值量级 ×10；实现侧默认权重表与相关测试的分数期望要同步改；不进 `stateHash`，锚点不受影响 | §5.11（权重刻度段、默认权重表、L1 校验行）；§12.2 |

**留痕的意义**：评审时若发现"为什么一个棋牌项目要 40～44 周"，答案在 12.2 而不是在某一节的数字里；若未来要缩范围，也按 12.2 的变更顺序倒着砍。


