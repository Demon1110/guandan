# Step 6 · 理解性评审（round-03）

| 项 | 内容 |
| --- | --- |
| 评审者 | egm-comprehension |
| 步骤 | 6 |
| 范围 | 白名单两文件全文（design_draft.md 全文、docs/assets/license.md 全文） |
| 对象类型 | technical |
| 只读 | 是 |
| 隔离模式 | fresh-subagent-query-leak |
| 状态 | complete |

## 覆盖声明

- 已读：`docs/designs/2026-11-23-guandan-multiplatform/design_draft.md`（全文，1–1319 行）
- 已读：`docs/assets/license.md`（全文）
- 未读：评审包第 7 节列出的 7 个 URL（未获人批准，未抓取）
- 未读：清单外文件（无）

## Findings

| Finding | 严重度 | 位置 | 问题 | 依据 | 建议 |
| --- | --- | --- | --- | --- | --- |
| F6-01 | Major | §6.5「房间快照」行 / §5.12 字段清单 | 我按 §6.5 的括注「即 stateHash 的字段集减去版本串再加 seed/snapSeq」逐项对照，发现快照枚举里没有 §5.12 的第 8 项 `tribute`（fromSeat、toSeat、card），却多出 `zoneDigest`；同时快照保留了 `rulesetVersion`、只去掉 engine 与 interpreter 两个版本串。读者无法判断「一一对应」以哪边为准、tribute 是否属于必须可重建的状态。成因：括注描述与紧邻的字段枚举不一致，且未说明 zoneDigest 是否计入该对应关系。 | §6.5：`{seed, rulesetVersion, phase, turnSeat, levels, seats, zones[], lastPlay, seqWatermark[], rng, snapSeq, zoneDigest}`，并写「快照字段集与 stateHash 字段集必须一一对应，少一个字段就等于少一段可重建状态」；§5.12 第 8 项为 `tribute`（`fromSeat`/`toSeat`/`card`，进贡阶段结束后归零）。 | 请回答：快照是否必须包含 tribute 段？zoneDigest 属于「对应字段」还是额外的写入前校验字段？若两集合确实不等，§6.5 的「一一对应」句应按哪个集合改写？ |
| F6-02 | Major | §6.5「动作日志」行 / §7.7 `actions.jsonl` 行 | 我在 §6.5 读到 ActionRecord 与回放包 actions.jsonl「同一结构（只此一份）」，又读到 §7.7 说 actions.jsonl「与 ActionRecord 同构」「重放器不做任何字段转换」，但两处字段对不上：ActionRecord 是 seatSeq 加 roomSeq，actions.jsonl 只有一个 seq。读者无法判断回放里的 seq 对应哪个序号，也无法判断「不做转换」是否成立。成因：两处各自罗列字段，未说明 seq 是 roomSeq 或 seatSeq 的别名。 | §6.5：`ActionRecord = {seatSeq, roomSeq, seat, requestId, kind, payload, tsMs}`；§7.7：actions.jsonl 与 ActionRecord「同构：`seq / seat / requestId / kind / payload / tsMs`」。 | 请回答：actions.jsonl 的 seq 等于 ActionRecord 的 seatSeq 还是 roomSeq？若两个序号都需保留，另一个从何而来？ |
| F6-03 | Major | §5.1 `deck` 行 / §5.1 规则示例 yaml | 我先按 §5.1 的字段清单（含 jokers）读示例，见 deck 只有 copies、ranks、suits，而同一示例的 rankOrder 又出现 little-joker 与 big-joker。按 §5.1 的总张数公式算得 104，与同节的 108、以及 zones 中 4×27=108 的手牌容量对不上。读者无法判断示例漏写王，还是 jokers 有缺省值。成因：字段清单把 jokers 列为结构之一，示例省略且未给缺省；§5.1 又声明该示例是「校验基准」，必须跑通 L1–L4。 | §5.1 `deck` 约束：总张数 = `ranks × suits × copies + jokers × copies` = 13×4×2 + 2×2 = 108；示例：`deck: { copies: 2, ranks: [2,3,…,A], suits: [S,H,D,C] }`；示例 zones：`hand capacity: 27`；§5.1：示例过不了自己定的校验即视为设计缺陷。 | 请回答：示例是否漏写 `jokers: 2`？若 jokers 允许缺省，缺省值写在哪条约束里、缺省时总张数按什么算？ |
| F6-04 | Major | §5.4 L2 行 / §5.1 `zones` 约束 / §5.7 `RS_ZONE_CAPACITY` 行 | 我把三处「牌数守恒」当成同一件事读，却得到三个算式：§5.4 是 deck×copies 等于各区容量和；§5.1 是牌堆总张数等于各区当前牌数之和；§5.7 是当前牌数不等于牌堆总数或某区超上限。且示例中 discard 的 capacity 为 -1（无上限），按字面把容量求和得 27×4−1=107，永不等于 108，L2 无法通过；§5.1 又明确「容量只做上限校验，两者不得混用」。读者无法判断 L2 究竟断言什么。 | §5.4 L2：`牌数守恒（deck × copies = 各区容量和）`；§5.1 zones：`capacity` 是上限、`-1` 表无上限、运行时断言 `deck 总张数 = Σ 所有区当前牌数`；同格：容量只做上限校验，两者不得混用；§5.7 `RS_ZONE_CAPACITY`：`当前牌数 ≠ 牌堆总数，或某区超上限`；示例 `discard capacity: -1`。 | 请回答：L2 校验的是「容量和」还是「当前牌数和」？若为容量和，capacity 取 -1 的区如何参与求和？示例通过 L2 时应满足的算式请写出。 |
| F6-05 | Major | §5.1 `actions` 行 / §5.1 示例 yaml 的 pass 行 / §5.14 | 我按 §5.1 的三种 legalFrom 取值解析示例：play 用 patterns、tribute 与 return 用 tribute 阶段名都能落地，但 pass 写的是 `turn`——它不在 lifecycle 的四个 phase（deal、tribute、play、settle）里，也不是 zones 中的区（示例只有 hand、discard）。§1.6 又写「轮（turn）」不进 ID。读者无法判断 `turn` 是第四个阶段、一个区，还是笔误。成因：§5.1 把 `turn` 举为「phase 名」的例子，但同节的 lifecycle 与 zones 都没有它。 | §5.1 actions：`legalFrom 只允许三种取值：patterns、某个 zone 的 id、某个 phase 名（如 tribute/turn）`；§5.1 lifecycle：`[deal, tribute, play, settle]`；示例：`{ kind: pass, primitive: action.pass, legalFrom: turn }`；§1.6：轮「不进 ID，只用于轮转描述」。 | 请回答：`turn` 在索引里对应哪一类对象？若既不是 phase 也不是 zone，pass 的 legalFrom 应改成哪个合法值？ |
| F6-06 | Major | §5.13 settle「过 A」段 / §1.6 场定义 | 我读到「任一队过 A 即该场结束（match 终止，按局数上限 3 局封顶）」，无法判断 3 局是整场上限还是未过 A 时的上限。按字面读成整场 3 局封顶时与过 A 冲突：从首局级牌 2 打到 A，即使每局双下加 3 档，也需 2→5→8→J→A 共 4 局，3 局内不可能过 A；而 §1.6 说一场是「从入座到所有玩家离桌」。读者不知道一场到底何时结束。 | §5.13：`任一队过 A 即该场结束（match 终止，按局数上限 3 局封顶）`；§5.13 settle 表：双下 +3；§1.6：场「从入座到所有玩家离桌；一场含若干局」。 | 请回答：「3 局封顶」约束整场还是未过 A 的连续局数？若整场封顶为 3 局，过 A 在该上限内是否可达？场的终点在「过 A」与「玩家离桌」之间如何取舍？ |
| F6-07 | Major | §5.6 发布语义段 / §5.6 状态机 canary 行 / §9.4 授权口径段 | 我在 §5.6 同一处读到「canary 是唯一允许自动推进的阶段（指标达标后自动转 full）」与「full 必须人工 TOTP 确认」，状态机写「full（自动达标 + 人工确认）」，§9.4 又写「canary 达标后自动转 full，但转全量时必须人按 TOTP 确认」。读者无法判断 full 由谁触发：系统自动置位后等人确认，还是人调用 publish 并带 TOTP；也不知道 TOTP 是审批凭据还是触发动作。接口 `POST /admin/rulesets/{id}/publish` 又要求调用方显式传 stage。 | §5.6：`canary 是唯一允许自动推进的阶段（指标达标后自动转 full）；full 必须人工 TOTP 确认`；§5.6 状态机：full 的进入条件写 `自动达标 + 人工确认`；§9.4：`canary 达标后自动转 full，但转全量时必须人按 TOTP 确认`。 | 请回答：canary 到 full 的推进由系统调用 publish 还是人工调用？TOTP 校验发生在哪一步、校验通过后由谁写入状态？ |
| F6-08 | Major | §5.6 发布语义段 / §7.2 `action_cpu_overrun_total` 行 / §8.6 CPU 防护行 | 我把「CPU 超预算是否触发 canary 回滚」对照三处，得到相反答案：§5.6 的自动回退条件明说「不含 CPU 超预算」，§7.2 说该指标「不参与 canary 判定」，§8.6 却说「连续 3 分钟超阈值则把该 RuleSet 版本标 canary 失败并回滚」。读者无法判断 CPU 超预算到底会不会触发 canary 回滚。 | §5.6：`窗口内出现 stateHash/viewHash 不匹配、action_illegal 超标（不含 CPU 超预算，见 §8.6）或崩溃率超标，自动回退`；§7.2：`action_cpu_overrun_total` 柱「不进非法率、不参与 canary 判定」；§8.6：`连续 3 分钟超阈值则把该 RuleSet 版本标 canary 失败并回滚`。 | 请回答：CPU 超预算是否属于 canary 自动回滚的触发条件？「标 canary 失败并回滚」与「不参与 canary 判定」如何在同一版本上并存？ |
| F6-09 | Major | §8.6「CPU 炸弹」行 / §5.7 `RS_QUOTA_EXCEEDED` 行 | 我读到「一次最多生成 4096 个候选，超限即拒绝加载」，又读到错误码表里同一 4096 上限的处置是「拒绝该步动作并记 action_quota_exceeded_total；RuleSet 侧按缺陷处理」。读者无法判断超限发生在发布期（RuleSet 被拒）还是运行期（单步被拒）：两者的产物、告警与回滚路径不同。成因：同一常量在两节被赋予两种时机。 | §8.6：`原语内部枚举设硬上限（一次最多生成 4096 个候选，超限即拒绝加载）`；§5.7 `RS_QUOTA_EXCEEDED` 触发为 `legal_actions 候选枚举超过 4096 条的常量上限`，处置为 `拒绝该步动作并记 action_quota_exceeded_total；RuleSet 侧按缺陷处理`。 | 请回答：4096 超限发生在发布校验还是运行时长路径？发布期如何预知一次枚举的规模？运行期拒绝该步之后，房间与该 RuleSet 版本各处于什么状态？ |
| F6-10 | Major | §6.3 竞态裁决段 / §5.7 错误码表 | 我读到超时与动作同 tick 竞争时「返回 TOO_LATE」，但到 §5.7 的唯一错误码表里找不到 `TOO_LATE`（表内只有 STALE_SEQ、RATE_LIMITED 等）；§5.15 又说所有接口「全部复用 §5.7 的错误码表，不另立一套」。读者不知道 `TOO_LATE` 是否算一次超时、客户端收到后应重拉增量还是直接重发，也不确定它是否属于对外错误码。 | §6.3：`反之判超时并丢弃动作（返回 TOO_LATE）`；§5.7 错误码表无 `TOO_LATE`；§5.15：`以上接口全部复用 §5.7 的错误码表，不另立一套`。 | 请回答：`TOO_LATE` 是否纳入 §5.7 错误码表？它的触发条件、客户端处置与是否计入超时次数分别是什么？ |
| F6-11 | Minor | §5.12「纳入 stateHash 的字段」清单 | 我按顺序读这份「固定顺序拼接」的清单，编号从 6（seqWatermark）直接跳到 8（tribute）、再到 9（rng），第 7 项不存在。读者无法判断是删过一个字段（那就要确认它是否仍影响状态），还是仅编号笔误；S10 要求唯一算法定义，序号断档会让「按清单顺序实现」失去确定所指。 | §5.12：`6. seqWatermark（每座位 u64，按座位号升序）` 之后直接是 `8. tribute`，再接 `9. rng`。 | 请回答：第 7 项是被删除的字段还是编号笔误？若删除，被删字段原名与删除理由是什么？ |
| F6-12 | Minor | §7.1 表「出问题了吗」行 / §7.2 P0 清单段 | 我在 §7.1 读到「4 个 P0 信号必须能在 1 分钟内触发」，紧接着 §7.2 标注「5 个 P0 信号的清单」并列出五项。读者无法判断 P0 是 4 个还是 5 个，以及 1 分钟内触发的时限是否也适用于第 5 项。成因：§7.1 未随 §7.2 的补全同步。 | §7.1：`4 个 P0 信号必须能在 1 分钟内触发`；§7.2：`**5 个 P0 信号的清单**` 后列五项。 | 请回答：P0 清单以 4 项还是 5 项为准？五项是否都要求 1 分钟内触发？ |
| F6-13 | Minor | §6.6「对账任务」段 | 我读到「每日跑两份报表，只比条数」与「round_record 条数 vs settlement_dead_letter 条数之和 vs 房间结算事件条数，差异条数 > 0 即告警」，无法还原等式：是三组数两两相等，还是 round_record 条数加 dead_letter 条数等于结算事件条数；「条数之和」是两项相加还是各取条数。读者照此实现会写出不同报表。 | §6.6：`1. round_record 条数 vs settlement_dead_letter 条数之和 vs 房间结算事件条数`，`差异条数 > 0 即告警`。 | 请回答：这三个量的等式关系是什么？dead_letter 的行是否已计入 round_record（即相加还是取其一）？ |
| F6-14 | Minor | §1.4 成功判据 S1–S12 / §4.2 候选表 S1–S7 | 我先把 §1.4 的 S1 到 S12 读成成功判据编号，到 §4.2 又看到同一套字母下 S1 自建到 S7 Pomelo 是候选方案编号，§4.4 还写「主方案 S1」。读者读到「S1 的例外条款写在 §1.4」与「主方案 S1（自建）」时无法区分指代，也无法确定 §12.1 依据列的 S10、S5 指判据还是方案。成因：两套编号共用 S 加数字。 | §1.4：S1–S12 为判据；§4.2：`**S1 自建**`、`**S2 Nakama**` 至 `**S7 Pomelo**`；§4.4：`结论：主方案 S1（自建 Rust 平台）`；§12.1 D1 依据：`C3、S10、A11`。 | 请回答：方案候选与成功判据是否改用互不重叠的编号前缀？正文里的「见 S1」应按哪一套解读？ |
| F6-15 | Minor | §1.6 术语表 / §1.4 S2 / §2.4 gate 口径 | 我先在 §1.6 读到场是「入座到离桌、含若干局」、局是「发牌到结算」，再读到 S2「完成一局：建桌 → 发牌 → 出牌 → 结算」与 §2.4「Web 端 + 本地 bot 可完整打完一场」。读者无法判断验收要求是一局还是一整场：建桌属场级、结算属局级，却写进「一局」；「完整打完一场」又与 gate 里的「再来一局」并列。成因：术语表定死两词后，后文仍互换使用。 | §1.6：场「从入座到所有玩家离桌；一场含若干局」；§1.4 S2：`完成一局：建桌 → 发牌 → 出牌 → 结算`；§2.4：`Web 端 + 本地 bot 可完整打完一场`、gate：`再来一局`。 | 请回答：S2 与第 16 周 gate 验收的粒度是「一局」还是「一场」？建桌归到场还是局？ |
| F6-16 | Minor | §5.13 deal 表「决定首出」行 | 我按字面读「首局由 level 最低座位先出」，理解为按级牌高低选座位；但 §5.12 与 §5.13 都写级牌是每队一个，首局两队都是 2，无法决出「最低座位」；括号同时给出「v1 固定座位 0」，两者是不同选择依据。读者不知道该实现哪条，也不知道括号里的 v1 是否表示前半句永不启用。 | §5.13：`首局由 level 最低座位先出（v1 固定座位 0）；之后由上局头游先出`；§5.12：`levels` 为每队一个 u8。 | 请回答：句中 level 指什么？v1 是否只实现固定座位 0、前半句是否仅描述未来规则？ |
| F6-17 | Minor | §6.3 托管表「连续 2 次超时」行 | 我读到「单步超时 1 次自动过、连续 2 次超时进入托管」，但全文没有定义「连续」的边界：同一座位相邻两次行动，还是整场累计；中间正常出牌一次是否清零；跨局是否清零。读者实现时三种口径会得到不同托管触发点。 | §6.3：`连续 2 次超时` 的处置为 `进入托管，由 AI 代打（只用 view(seat)）`；上一行为 `单步超时 1 次` 的处置。 | 请回答：「连续」以什么事件为分界、什么事件会清零？跨局是否延续？ |
| F6-18 | Minor | §5.10 规则取值表「接风」行 | 我先按表头读「左列是 RuleSet 字段」，到接风行看到左列是 `turn.skip-finished` 加队友判定——这是 §5.2 的原语名加一个未命名的判定，不是 §5.1 顶层结构里的任何字段；而该行状态为问号，要求确认后去掉问号并冻结进 guandan-classic v3。读者不知道确认结果写进哪个字段（是否需要新增 turn 配置字段），也不知道接风语义是否只能随 engine 版本变化而不进 RuleSet 版本。 | §5.10：`左列是 RuleSet 字段`、标问号的行逐条确认后冻结；接风行取值为 `turn.skip-finished + 队友判定`；§5.1 顶层字段表中没有承载 turn 参数的字段；§5.13：接风「由 turn.skip-finished + 队友判定实现」。 | 请回答：接风的确认结果落在哪个 RuleSet 字段？若它只由引擎原语决定，该行为什么列在需要冻结的 RuleSet 取值表里？ |

## 未覆盖范围

- 未评价方案取舍好坏、备选方案与优先级（属 Step 5 与 Step 7 的职责）
- 未核对代码、路径与签名（read_list 内无源码；§3.1 自述仓库无源码）
- 未做实现就绪度判定（属 Step 9）
- 未对 license.md 的素材条目内容作判定（登记表为空属 §3.4 所述 M1 前归档项，非本轮问题）
