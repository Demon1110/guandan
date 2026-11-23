# round-04 · step6 理解性评审报告（egm-comprehension）

## 元信息

| 项 | 内容 |
| --- | --- |
| 评审者 | EGM comprehension 理解性评审者（round-04 step6，fresh-subagent） |
| 步骤 | round-04 / step 6 |
| 范围 | `design_draft.md`（全文通读，非抽样）、`docs/assets/license.md`、`manifest.json`；`review_packet.md` 为执行依据 |
| 对象类型 | technical |
| 只读 | 是（未修改任何既有文件，仅新建本报告） |
| 隔离模式 | fresh-subagent-query-leak（取自 `manifest.json` isolation；`zero_context=false`，已知 L2 原始请求透传，结论已据此降权） |
| 状态 | complete |

## 评审范围与依据

- 白名单内三份正文已全部读取：设计草案 1421 行逐节通读；`docs/assets/license.md` 为素材授权台账（登记表与字体表均为"待补"，变更记录 2026-11-23 建档）；`manifest.json` 确认 `read_list` 三项与 7 个未获人批准的来源 URL。
- 未读取白名单外任何文件；未读取 `reviews/` 下任何历史报告或目录；未抓取任何 URL，未用搜索代替抓取。选型表（§4.2）中关于 Nakama / Colyseus / Skynet / Photon / Agones 的第三方事实本轮无第三方复核，按"该结论文中无出处，属未验证"处理，不作为本报告 finding 的依据。
- 本报告只评文档自身完整性：一个无前置背景的新人，仅凭本文本能否把规则判定步骤与 M0 第一步动作落地。

## 已核无误项（三类聚焦中核对一致、不进 Findings 的部分）

- **`seatSeq` / `roomSeq` 指称一致**：`seatSeq` 为每座位单调递增水位，`roomSeq` 为房间广播全局序；§5.9 协议表、§6.2 序号作用域、§5.7 `STALE_SEQ`（序号过期）与 `TOO_LATE`（同 tick 竞态落败）的分工、§6.5 `ActionRecord` 双序号列四处口径一致，未发现混用。
- **`stateHash` / `viewHash` 分工一致**：`stateHash` 含全量手牌、永不发客户端；`viewHash(seat)` 按 `view(seat)` 投影计算并排除 `clientPlatform`；§5.12 字段清单、§5.9 消息表、§7.2 指标口径三处对齐。
- **`levels` 与 `rank_level` 指称一致**：队级牌 `levels` 是房间状态字段（每队一个），段位 `rank_level` 存于 `player_stat`；§5.8、§5.13、§5.12 第 2 项均明确二者不混名。
- **`zoneId` 实例化与排序一致**：私有区按 `(seat, zoneId)` 实例化，`u16` 区号在解析期按声明序分配冻结，哈希按 `(seat u8, zoneId u16)` 升序；§5.1 `zones` 行、§5.12 第 4 项、§6.5 快照字段集一致。
- **`stage` 发布状态机枚举一致**：`draft / validated / canary / full / rolled_back / retired` 六态在 §5.6 接口表、§5.8 `ruleset_release` 表、§9.4 灰度步骤三处使用同一套字符串。
- **`stateHash` 九项与快照字段集一一对应**：§5.12 第 1–9 项与 §6.5 快照字段清单（版本串除外，另加 `seed`/`snapSeq`）逐项可对上。
- **过 A 的 `1+2` 与 `1+3` 口径在 §5.10 与 §5.13 之间未发现相反结论**：两者在过 A 判定上等价（均算过 A），区别仅在升级档（`1+2` +3、`1+3` +2）；`1+4` 不算过 A 两处一致。

## Findings

| Finding | 严重度 | 位置 | 问题 | 依据 | 建议 |
| --- | --- | --- | --- | --- | --- |
| F6-01 | Blocker | §5.1 `levelTable` 行；§5.12 第 9 项；§5.13 settle 段与「过 A」段；§5.14；§5.10 过 A 行 | `aFailStreak` 的取值上界（0–2）与触发退回的阈值（满 3 / 达到 3）自相矛盾，实现者无法判定第三次未过后计数器到底是 3 还是直接回退清零，规则不可执行。 | §5.1："每队一个 u8、取值 0–2，记录该队'在 A 上连续未过 A'的局数，达到 3 即该队 `levels` 退回 `initial` 并清零"；§5.12 第 9 项："取值 0–2"；§5.13："未过 A 则 `+1`；`+1` 后达到 3 时该队 `levels` 退回……并把 `aFailStreak[team]` 清零"；§5.13「过 A」："连续 3 局未过（`aFailStreak[team]` 满 3）"；§5.14："0–2"。取值 0–2 的字段永远到不了 3。 | 统一上界与阈值：要么字段取值改为 0–3、第三次未过置 3 后触发回退并清零；要么上界保持 0–2、触发条件改为"计数达 2（即第二次未过后）"；并写清每局 `+1`、判定退回、清零三者的先后顺序。 |
| F6-02 | Major | §5.13 表一（定级分档表）、表二（Elo 参数表）；§5.8 `player_stat` 行；§5.15 末段 | 定级赛期间的 `rating` 更新口径缺失：用什么 K、什么公式累计未写；定级 10 场是否计入 `rating_games` 未定义，导致 `K=40/32` 档位的切换时点无法判定；第 10 场同时落档与 Elo 更新的先后顺序未写。 | §5.13 表二："`K`：定级赛结束后前 20 场 `K = 40`，之后 `K = 32`；档位按 `rating_games` 判"；§5.8："`rating_games` 是已计入评分的场次数"；表一："新账号前 10 场为定级赛，只累计 `rating`、不改 `rank_level`"；§5.13 末段："`rating_games` 用于判定 `K` 档"。定级赛 10 场是否递增 `rating_games` 决定"前 20 场"从第 1 场还是第 11 场起算，文档未给边界值。 | 补写：定级赛每场 `rating` 如何更新（固定分或 Elo + 指定 K）；定级赛场次是否递增 `rating_games`；K 档按 `rating_games` 的具体边界（如 1–20 为 40、≥21 为 32）；第 10 场结算时落档与评分更新的执行顺序。 |
| F6-03 | Major | §8.7 评分防刷分行；§5.13 表二 | 同对手重复对局的 K 衰减与 40/32 K 档的叠加关系未定义；`K=0` 不写 `rating` 的场次是否递增 `rating_games` 未写，会反过来影响 F6-02 的 K 档判定。 | §8.7："同一对对手 24 小时内重复对局：第 3 场起 `K` 减半、第 5 场起 `K = 0`（该场不写 `rating`）"；表二 K=40/32。减半基数是 40 还是 32、衰减与档位切换孰先孰后未写；`K=0` 场是否计入 `rating_games` 未写。 | 写清叠加公式（最终 K 与档位 K、衰减系数的关系），并明确 `K=0` 场是否递增 `rating_games`、是否计入定级场次。 |
| F6-04 | Major | §5.13 tribute 段「抗贡」行；§5.1 `tribute.resist` 行；§5.12 第 7 项 | 抗贡命中后整段 `tribute` 的跳过范围未讲死：还贡是否随进贡一并跳过、双下时两家能否部分抗贡、`tribute` phase 是否进入、事件与哈希字段如何处理均未定义。 | §5.13："贡方两人合计持 2 张大王（单贡时贡方自己持 2 张大王）即可抗贡，本局整段 `tribute` 跳过"；§5.1："`two-big-jokers` 按贡方两家合计持两张大王判定，不要求在同一人手里"；§5.12 第 7 项："`tribute`（`fromSeat`、`toSeat`、`card`，进贡阶段结束后归零）"。未写：抗贡局还贡是否发生；双下两家是否允许只免一家；抗贡时 `PhaseChange`/`TributeEvent` 是否广播；stateHash 第 7 项在抗贡局的初始占位值。 | 明确：抗贡为进贡+还贡整段全跳还是部分跳；双下抗贡是两家合计一次性判定（不存在部分抗贡）；抗贡局 `tribute` phase 的进入/退出口径、事件是否发出；哈希第 7 项在抗贡局的初始值。 |
| F6-05 | Major | §4.5 匹配行；§5.8 `player_stat` 行；§5.13 表一；§10.2 定级赛归属用例；§1.6 术语表 | 定级单位"局 / 场"指称冲突：§4.5 写"前 10 局走定级局"，其余各处按"场"结算，而定级赛分母到底是 10 个 match 还是 10 个 round 文档内部不一致。 | §4.5："内测首日没有段位可填，前 10 局走定级局"；§5.8："`rating` 与 `rank_level` 按场结算"；§5.13："新账号前 10 场为定级赛"；§10.2："新账号连续 10 场 `human-ranked` 结算"；§1.6："一场含若干局"。 | 统一术语：定级赛按场（match）计，将 §4.5 的"前 10 局"改为"前 10 场"，并在 §1.6 术语表注明定级/段位一律按场结算。 |
| F6-06 | Major | §5.1 `rulesetId` 行；§5.6 `GET /admin/games` 行；§5.15 `POST /api/match/queue`、`POST /api/room/create` 行；§5.8 Redis 键清单 | `gameId` 与 `rulesetId` 的映射关系未定义：建房/匹配传 `gameId`，RuleSet 按 `rulesetId` 注册，二者基数关系与建房时选取 active/canary 版本的判定路径缺失，新人无法实现匹配与建房的规则加载。 | §5.1："`rulesetId` 全局唯一，kebab-case"；§5.6："`GET /admin/games` → `[{gameId, activeVersion, stage, roomCount, canaryVersion}]`"；§5.15：队列与建房入参均为 `gameId`；§5.8：Redis 键 `game:{id}:active_ruleset`、`game:{id}:canary_ruleset`。文档未说明一个 `gameId` 对应一份 RuleSet 还是一个玩法族下多版本。 | 补一张 game ↔ ruleset 映射口径：`gameId` 的定义、与 `rulesetId` 的基数关系、建房时按哪个键选取 active / canary RuleSet 版本。 |
| F6-07 | Major | §2.4 M0 行；§2.1 `crates/protocol`、`crates/table-runtime` 行；§5.9 首行；§1.5 A1、A15、A16 | 仅凭本文档，新人列不出 M0 第一个可执行动作：缺 M0 任务分解、workspace/crate 目录骨架、engine 公共接口签名、proto 文件位置与包名；且 M0"断线恢复原型"与 M1–M2 交付物 `crates/table-runtime` 的落位边界未划清。 | §2.4 M0："引擎契约与原型、`crates/protocol`、确定性纪律、双产物 CI、空转压测与断线恢复两个原型"；§2.1：`crates/table-runtime` 阶段 M1–M2；§5.9："M0 要交付的就是这张表"；A15："M0 交付'空转压测'与'断线恢复'两个原型"；A16："M0 基准：同一 RuleSet 在双产物下各跑 10 万次 `legal_actions` 与 `apply`"。文档未给首个 crate/文件路径、接口签名或第一步验收口径。 | 在 M0 段补第一步清单：Cargo workspace 与 crate 骨架（含 `crates/protocol` 的 proto 文件路径/包名）、engine 最小对外接口签名、两个原型各自落位的 crate 与对应 A1/A15/A16 的可测验收指标。 |
| F6-08 | Major | §5.1 `tribute` 行；§5.1 YAML 示例 | `tribute.trigger` 字段的取值枚举与语义未定义，L1 结构校验无法闭环；该字段与 `pairs[].when` 的分工也未说明。 | §5.1："`tribute` | `{enabled, trigger, pairs[], returnPolicy, resist}`"；示例 YAML："`trigger: last-round-rank`"。全文未给 `trigger` 的取值集合，`last-round-rank` 在类型表中无定义；`when` 已有 `single-loss`/`double-loss` 闭集，二者关系未写。 | 补 `trigger` 枚举闭集与语义，或在判定完全由 `pairs[].when` 驱动时删除该字段；在 §5.1 字段表写清取值与必填性。 |
| F6-09 | Minor | §11.2 U10 行；§5.10 首段；§12.2 第八次变更行 | U10 状态残留"七行待确认"旧口径，与规则取值已冻结的正文冲突。 | §11.2："U10 …… settled（七行待确认）"；§5.10："本表 15 行取值均已确定，不再有待确认项"；§12.2 第八次变更："7 行状态由待确认改为 `定`、全表冻结"。 | 将 U10 状态改为 settled（已冻结），去掉"七行待确认"残留。 |
| F6-10 | Minor | §5.13 表一（定级分档表）；表二 S 行 | 定级胜率临界值（30% / 50% / 70%）归属哪一档未定义区间开闭；平局按 0.5 折算后是否进入胜率分母未写。 | 表一："30%–50% → 2、50%–70% → 3"；表二："`S`：1（胜）/ 0.5（平）/ 0（负）"。临界 30%/50%/70% 落上档还是下档未写；平局计入分子折算还是剔除出分母未写。 | 写清区间开闭（如左闭右开）与胜率公式（含平局折算口径）。 |
| F6-11 | Minor | §5.13 settle 段；§5.14 | `levels` / `aFailStreak` 跨 match 的重置路径未定义：一场因过 A、safety-cap 或 aborted 结束后，下一场开局时是否回到 `initial`、计数器是否清零、由哪一步执行，文档只写了"一场内逐局累积"。 | §5.13："一场（match）内逐局累积"；§5.14："队级状态里跨局保持的只有两项：`levels`（每队一个）与 `aFailStreak`（每队一个、0–2）"。新 match 首局的初始化口径未写。 | 补 match 开局初始化口径：新 match 首局 `levels=initial`、`aFailStreak=0` 的执行时点与位置。 |
| F6-12 | Minor | §5.12 第 2 项；§5.13 `levels` 存放位置段；§5.1 `levelTable` 行 | `levels` 的存储形式未定义：存 `ranks[]` 数组下标还是牌面键，stateHash 拼接与双端比较都依赖此口径。 | §5.12 第 2 项："`levels`（每队一个 u8，按队伍序号升序）"；§5.13："取值 2..A"；§5.1：`levelTable.ranks: [2,3,...,A]`。u8 存 0–12 下标还是 2–14 牌面值未写。 | 明确 `levels` 存 ranks 数组下标（或牌面键枚举），并在 §5.12 序列化字段说明中写死。 |
| F6-13 | Minor | §5.8 `player_stat` 行；§5.13 表一注；§5.15 末段；§10.2 定级赛归属用例 | 定级期内 `rank_level` 到底"不改"还是"定格为定级分档结果"两处口径冲突，定级过程中是否随当前胜率临时更新档位未写。 | §5.8："定级期只累计 `rating`、不改 `rank_level`"；§5.13 表一注："定级期内的 `rank_level` 显示定级分档结果"；§5.15："`rank_level` 定格为定级分档结果"；§10.2 用例断言："定级期内 `rank_level` 不改"。 | 明确定级期 `rank_level` 的更新规则：开局置默认值、10 场结束才落档，还是按当前胜率临时显示档位；三处口径统一。 |
| F6-14 | Minor | §5.13 升级跨 A 处置段；§5.13 名次组合升级表 | `1+4`（+1）从 K 升到 A 的跨 A 处置未写：文档只提 `1+2`/`1+3` 停留一局，未说明 +1 到达 A 是否同样停留、超额是否作废。 | §5.13："从 K 打 `1+2`/`1+3` 升级时，先在 A 停留一局（不越级）"；升级表："`1+4` +1"。K 按 +1 到 A 的处置口径缺失。 | 补 `1+4` 到达 A 的处置，与 `1+2`/`1+3` 对齐说明停留与超额作废规则。 |
| F6-15 | Minor | §5.13 play 段「顺子边界」行 | A-2-3-4-5 的相邻判定与线性 `naturalOrder` 的关系未写实现口径：A 与 2 的相邻属环形特例，两端实现同序判定需要明确规则。 | §5.13："顺子取 5 张连续自然点数，允许从 A 起（`A-2-3-4-5`）但不允许绕回（`Q-K-A-2-3` 非法）"；`naturalOrder` 为 2..A 线性表，A 前接 2 的特例判定步骤未写。 | 写清顺子判定的环形特例（A 可前接 2、不可后接 2），便于双端实现同一判定顺序。 |
| F6-16 | Nit | §5.15 `GET /api/match/queue/{ticketId}` 行 | 出参 `roomAddr` 的结构与用途全文未定义。 | 轮询出参："`{state: waiting|matched|timeout, matchId?, roomAddr?}`"；§4.5 仅写"按 `matchId` 哈希把同一房间的连接固定到同一实例"，未解释 `roomAddr` 字段内容。 | 定义 `roomAddr` 的字段结构与用途，或在出参中删除该字段。 |
| F6-17 | Nit | §5.9 `Reconnect`/`SubmitAction`/`RoomDelta` 行；§6.2 序号作用域 | `seatSeq` / `roomSeq` 的起始值未定义，水位比较与 `STALE_SEQ` 判定依赖序号起点。 | §6.2："`seatSeq` 是每座位单调递增的"；"`roomSeq`（房间内全局递增）"；未写从 0 还是 1 起算。 | 在 §6.2 补两个序号的初始值与首条动作/事件的序号。 |
| F6-18 | Nit | §6.6 三档可信度表；§5.15 真值表 | `bot` matchType 与三档可信度 `online-bot` / `offline-local` 的映射未在表中对齐。 | §6.6：`online-human` / `online-bot` / `offline-local` 三档；§5.15 真值表：`bot` = 纯 bot/离线局，`mixed` = 含 bot 补位。联机含 bot 补位档对应 `mixed` 还是 `bot` 未显式写明。 | 在 §6.6 三档表补一列 `matchType`，与 §5.15 真值表逐项对齐。 |
| F6-19 | Nit | §5.6 `POST /admin/rulesets/{id}/publish` 行；`promote` 行 | publish 入参 `stage` 是否参与校验、跨级调用返回什么处置未写。 | §5.6 publish 入参："`{draftId, stage: canary}`"；"首档固定 `canary`"；promote："跨级返回 409"。publish 传非 `canary` 值或跨级的错误码/处置未写。 | 明确 publish 的 `stage` 入参仅作声明还是校验项，非法值/跨级返回的错误码。 |
| F6-20 | Nit | §5.7 `RS_QUOTA_EXCEEDED` 行；§8.6 CPU 炸弹行 | 4096 候选枚举上限的触发时点两处表述不一。 | §5.7："dry-run / 发布校验时 `legal_actions` 候选枚举超过 4096 条静态上限；不用于运行期单步动作拒绝"；§8.6："候选枚举 4096 条为加载期/发布期静态上限：超限拒绝加载或拒绝发布"。"拒绝加载"与 §5.7 的"拒绝发布 / dry-run 无效"口径未对齐。 | 统一触发阶段（加载期 / 发布期 / dry-run）与对应处置错误码。 |

## 未验证与限制说明

- §4.2 选型表中各候选框架的形态、许可、能力描述来自 7 个未获人批准的外部 URL，本轮未抓取、未复核；本报告未将这些第三方事实作为任何 finding 的依据。
- `docs/assets/license.md` 当前登记表与字体表均为"待补"，无具体素材条目可供核对授权合规性；该项属 M1 填充事项，本轮不构成设计缺陷。
- 文档中引用的 `C1–C9`、`U1–U12`、`S1–S12`、`R1–R13`、`A1–A17` 编号在本草案内均可自洽对上；`concepts_brief.md` 等白名单外文件未读取，编号与外部概念简报的一致性本轮无法核对。

## 自检结果

- 元信息表「状态」单元格内容恰为 `complete`，无括号备注、无留空。
- `## Findings` 段内仅含一张六列表格，段内无任何以 `#` 开头的小标题，表格前后无其他段落文字。
- 严重度列仅出现 `Blocker` / `Major` / `Minor` / `Nit` 四个字面值，无组合值或中文备注。
- 编号 `F6-01` 至 `F6-20` 两段式连续编号，无跳号、无重号。
- 每条 finding 均给出章节位置与可检索的原文摘录；未读取白名单外文件，未抓取任何 URL，未修改既有文件。
