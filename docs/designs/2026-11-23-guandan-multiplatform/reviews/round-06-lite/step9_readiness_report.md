# Step 9 就绪度报告（round-06-lite 轻量验证轮）

| 项 | 值 |
| --- | --- |
| 评审者 | egm-readiness（readiness + codebase_details + style） |
| 步骤 | 9 |
| 范围 | DetailedRange（全文件，重点 §5.1–§5.16、§6.6、§9.4–§9.5、§10.2–§10.3、§12.2） |
| 对象类型 | technical |
| 只读 | 是（`design_draft.md`、`docs/assets/license.md`、`manifest.json`） |
| 隔离模式 | fresh-subagent-query-leak |
| 状态 | complete |

核对说明：本轮只跑第 9 步，只读上述三个文件，未读取 `reviews/` 下任何历史报告；`manifest.json` 的 `code_check=false` 且仓库无源码，故本报告不含任何代码层结论，仅评文档自身的实现就绪度。`manifest.json` 的 `missing` 项 `crates/protocol/proto/gf/v1.proto` 是待建路径，不计缺陷。定值向量的字节数与指纹由本评审按 §5.12 表独立重算，结果写在下方第 2 项。

## 22 项落地核对

| # | 验证项 | 结论 | 关键证据（行号） |
| --- | --- | --- | --- |
| 1 | §5.1 `flow` 五子对象逐键类型/取值/默认值；每个原语是否有落点 | 部分 | 401–413 三列齐全；落点缺口见 F9-02、F9-03、F9-04 |
| 2 | §5.12 字段级编码表与定值向量 | 通过 | 810–814 三键的类型/缺值/定序齐全；按表重算 canonical bytes = **129 字节**、SHA-256 = `9e0b756d600f8eb6b74b098f804735c752a2232f0ca3772673a3a4da51afcc72`，与 822–823 完全一致；牌 id 编码（806）以 deck 序 × 副本号分配、载入期冻结，双端唯一；`seat.levels` 的数值语义见 F9-11 |
| 3 | `kind` 收为 `play`/`pass`，无 v1 断裂引用 | 通过 | 391；`claim`/`declare` 只在 493、544 标 v2 预留，正文无 v1 用法 |
| 4 | `Settle::ScoreOnly` v1 在役；`m0-minimal` 走真实 DSL | 通过 | 187（路径 `crates/engine/tests/fixtures/m0-minimal/ruleset.yaml`、版本 `m0-minimal@1.0.0`、`settle: {algorithm: score-only}`）、392 |
| 5 | `viewHash` 服务端自证；`ViewMismatchReport` 移出；`SettleResult` 不依赖 view | 通过 | 784、727、713、1094 |
| 6 | `RoomSnapshot` 带版本字段；缓存键二元组 | 通过 | 709；394、946、1291 同口径 |
| 7 | 发布状态机（四步事务/`superseded`/active 判定式/rollback/表主键） | 通过 | 596、609；619–623；665（主键 `release_id`，`rev` 为普通列，仅进 canary 建行） |
| 8 | 结算对账等式；`settlement_event` 移出结算事务 | 通过 | 1039；1034、680 |
| 9 | 段位降档、Elo/K 衰减已删、只在服务端整数、不进 `stateHash` | 部分 | 895、1251；但 v1 落库值的计算口径仍无定义，见 F9-08 |
| 10 | §5.1 示例点数带引号且能原样过 L1（484 行自证条款保留） | 未通过 | 425–438、471–475 点数均带引号，484 条款在；但 464 的 `tribute.return` 不在 389 行的类型列，按 §5.2/§5.3 会被 L1 判未知键，见 F9-01 |
| 11 | 四个重复键已删、`level.upgrade` 无参、还贡三键、L2 唯一引用 | 通过 | 415、532、389+464、565 |
| 12 | `JoinAck` 字段齐、`RS_SEAT_TAKEN` 走该帧、载体四分 | 通过 | 705、646、655 |
| 13 | 三个新码入表、L2 每检查项映射唯一码 | 部分 | 640–642 入表；565 的六个映射成立，但两项无码，见 F9-07 |
| 14 | `RS_QUOTA_EXCEEDED` 为 L3 dry-run 语义、载入期闸门已删 | 通过 | 566、643、1097、1237 |
| 15 | `stateHash` 第 1 项移出版本串、`(版本, 哈希)` 口径 | 通过 | 788、1285；标称计数见 F9-10 |
| 16 | `Validation` 与 `EngineError` 可编译 | 部分 | 573–574 与 185 签名一致；`Finding.code` 类型未定，见 F9-09 |
| 17 | `ladder` 只列非插入型、`straight-flush-*` 由 `flushRank` 合成、两条 L1 | 通过 | 388、564 |
| 18 | `levelTable.upgrade[]` 结构与示例、L1 约束 | 通过 | 383、433–435、564 |
| 19 | AI 三档胜率断言挂 M2 之后 | 通过 | 1378「生效时点」；M0/M1 gate（190–196、163）未被该断言约束 |
| 20 | 工期 40～44 周与五段周次、第 16 周 gate | 通过 | 41、159–166、170；`34～38` 仅出现在 170 与 §12.2 的历史留痕里，无在役承诺 |
| 21 | §5.10 冻结表 15 行与四王炸 | 通过 | 749–763 共 15 行；755 四王炸为「定」，869 与 454 显示其在 v1 压制阶梯末位 |
| 22 | 表格列数/编号/错误码闭合 | 部分 | §1–§12 小节编号连续、无占位未填；25 个错误码在 §5.6 的 12+13 两档分类中全覆盖；表格断裂两处，见 F9-06 |

## Findings

| Finding | 严重度 | 位置 | 问题 | 依据 | 建议 |
| --- | --- | --- | --- | --- | --- |
| F9-01 | Major | `§5.1` `tribute` 行（389 行）与示例（464 行） | `tribute` 的类型列写 `{enabled, trigger, pairs[], resist}`，不含 `return`；但同段正文要求还贡三键「收归 `tribute.return`」、示例也把 `return: {ceiling, count, fallback}` 写在 `tribute` 下。按类型列反序列化，`return` 是未知键，示例会被 L1 拒绝，与 484 行「示例过不了自己定的校验即视为设计缺陷」直接矛盾，而 M0 的「非法结构不可构造」（196 行）正要以该示例为基准 | 389 行类型列；464 行示例；556 行 `deny_unknown_fields`；502 行「未知键……一律 `RS_SCHEMA_INVALID`」；484 行自证条款 | 类型列改为 `{enabled, trigger, pairs[], resist, return}`，并在约束里写明 `return` 三键（`ceiling`/`count`/`fallback`）在 `enabled=true` 时必填 |
| F9-02 | Major | `§5.1` `flow` 表 403–404 行；`§5.2` 506–508 行；`§2.4` 187 行 | `deal.shuffle` 的 `zone` 是必填键，但 `flow.deal` 只给了 `perSeat`/`reveal`，RuleSet 里没有任何位置能声明「发牌落到哪个区」；`deal.draw`（`zone` 必填、`count`）与 `deal.reveal`（`zone`、`toSeats`）更是完全没有承载位。`m0-minimal` 夹具只含「发牌」一件事，实现者要么自造键（被 L1 未知键拒绝），要么把 `hand` 硬编码进引擎（文档未授权，且多私有区变体无法表达） | 403–404 `flow.deal` 仅两键；506–508 三个发牌原语均以 `zone` 必填；187 行夹具说明；502 行「缺失必填键拒绝、未知键拒绝」 | `flow.deal` 增 `zone: ZoneId`（必填，或写死「取唯一 private 区」这一默认规则），并明确 `deal.draw`/`deal.reveal` 在 v1 是否启用；启用则同样给承载位 |
| F9-03 | Major | `§5.2` 级牌行（496）与参数行（533）；`§1.4` S8（68 行）、S11（71 行） | `level.pass` 定义了 `combos`/`streakLimit`/`resetOnFail` 三键（`level.current` 还有 `scope`），但 §5.1 顶层字段没有 `level` 段，`levelTable` 只承载 `initial`/`ranks`/`upgrade`，规则文件里无处写这三个键。结果：S8 要求的「过 A 条件 `level.pass` 可配置切换」在 v1 无法配置，S11 要求的「每条配置 ≥3 黄金用例」也就没有可切换的两个口径；实现者只能在引擎里写死 `[[1,2],[1,3]]`/3/true | 496 行把 `level.pass` 列为 v1 原语；533 行给出三键与默认值；68 行 S8 把「过 A 条件」列为可配置项；71 行 S11；§5.1 顶层 377–397 无 `level` 段 | 顶层增 `level: {pass: {combos[], streakLimit, resetOnFail}}`（`levelTable` 不动），或把 S8/S11 里的「过 A」一条移出可配置清单并写进 §2.2 的非目标；两者只能选一 |
| F9-04 | Minor | `§5.2` 527、528、539、541 行；`§5.1` 411–413 行 | 同族缺承载位（不阻断 M0，但变体配置表达不了）：`tribute.determine.firstRound`、`tribute.pay` 的 `count`/`selection`、`rng.range` 的 `min`/`max`、`assert.card-count` 的 `expected` 都无处声明，只能吃默认值；且 `flow.assert` 的键名与形状与原语字段不一致（`cardCount`↔`expected`、`zoneCapacity: [ZoneId]`↔`zone: ZoneId`） | 527–528、539、541 的必填/可选键；411–413 `flow.assert` 三键；顶层 `tribute`（389）无 `pay` 子键；`flow.rng`（410）只有 `stream` | 要么在顶层 `tribute`/`flow` 补对应子键并对齐键名，要么在 §5.2 把未启用原语标注「v1 不启用、参数仅供 v2」，二者择一 |
| F9-05 | Minor | `§5.1` 407 行、`§5.2` 525 行、`§5.10` 762 行 | 接风开关归属三处三种说法：§5.1 把 `partnerLead` 作为顶层 `flow.turn` 键（472 行示例也这么写），§5.10 却说它「不进顶层 RuleSet 字段」；且 407、525 行把承载位置写成 `flow.turn.skip-finished`，这个路径名在 §5.1 中不存在（`flow.turn` 下只有四个键） | 407、472、525、762 四处原文 | 统一为一种：`flow.turn.partnerLead`（顶层）或 `turn.skip-finished` 的 `params`（原语级），并删掉不存在的 `flow.turn.skip-finished` 写法 |
| F9-06 | Minor | `§5.2` 参数表 536–542 行；`§12.2` 1551 行 | 两处表格被空行截断而失去表头：`§5.2` 在 `score.level-up` 行后空一行，`view.project` 至 `assert.zone-capacity` 六行成了无表头的孤立行；`§12.2` 的「第十次变更」行在 1549 行散文之后单列，同样没有表头 | 535 行后空行、537–542 六行；1549 行散文、1551 行单行 | 删掉 536 行的空行把六行并回 `§5.2` 参数表；把 1551 行并回 1536 起的变更表（或给它配一行表头） |
| F9-07 | Minor | `§5.4` L2（565 行）；`§5.7` 640–641 行 | L2 有两个检查项没有可落地的错误码：①`seats.count` 不在 2–8 内（L1 的「取值范围校验」只列了 `timing` 与 `matchCap.rounds`）；②「总张数 ≤ 各 `hand` 区容量之和」不成立——`RS_DECK_COUNT_MISMATCH` 只定义「公式与 deck 声明总张数不符」，`RS_CAPACITY_INVALID` 只定义「`capacity` 取值非法」，容量不足时闸门结果没有码可报，跨端也只能各判一套 | 565 行两项检查与前后的失败表现列；640、641 两码的触发定义；564 行 L1 取值范围清单 | 给这两项各指定一个已有码（如容量不足归 `RS_CAPACITY_INVALID` 并在该码触发里加一句），或新增一码并入 §5.6 的 Blocker 档 |
| F9-08 | Minor | `§5.13` 895 行；`§5.15` 927 行；`§1.4` S9（69 行） | 段位算法整体降 v2 后，v1 仍要求 `human-ranked` 写 `rating`/`rank_level`，但 v1 自己没有任何计算口径（只有一句「服务端整数运算算出」），落库值无从产生；S9 的「段位跨端一致」因此缺少可判定内容。此项不影响 M0（段位在 M3），但 M3 开工前必须补 | 895 行「v1 不在本节定义定级分档表、Elo 参数表与阈值表」；927 行 ranked 仍写两字段；69 行 S9 | 明确 v1 的落库口径（例如「`rating` 只记场次与胜负计数、`rank_level` 恒为初始档」），或把两个字段的写入整体推到 v2 并同步 S9 的表述 |
| F9-09 | Minor | `§5.4` 573 行；`§5.6` 605 行 | `Validation.findings: Vec<Finding>` 中的 `Finding.code` 类型写成了章节引用「`code: §5.7 错误码`」而不是具体类型（字符串还是枚举未定），`severity` 的 `Blocker|Major|Minor` 是内联枚举、没有命名；`EngineError` 也没有承接 `RS_ZONE_CONSERVATION`/`RS_ZONE_CAPACITY`（569 行说这两条由 `apply` 之后的断言报出）的变体，实现时只能塞进 `Internal` | 573 行结构；605 行 `Finding{...}` 与 `code: §5.7 错误码`；569 行「每步 `apply` 后由引擎断言」；574 行变体清单 | 把 `code` 定为枚举（或 `&'static str`）、给 `severity` 起个类型名，并在 `EngineError` 里补 `ZoneConservation`/`ZoneCapacity` 两个变体 |
| F9-10 | Nit | `§5.12` 786–788 行 | 标题写「纳入 `stateHash` 的字段（共 10 项，按下列固定顺序拼接）」，但第 1 项自述「不进哈希拼接」，实际参与拼接的是 9 项；按字面读会以为要拼 10 段 | 786 行「共 10 项」；788 行第 1 项「不进哈希拼接」 | 标题改为「9 段拼接 + 1 项旁挂版本字段（不进拼接）」，或把第 1 项移出编号 |
| F9-11 | Nit | `§5.12` 813 行；`§5.13` 885 行 | 视图编码表把 `seat.levels` 定为 `u8[]`，未说明元素是 `levelTable.ranks` 的下标还是点数字面值；§5.13 只说「序列化……一律按下标」，定值向量里的 `[2,2]` 因此读不出是「级牌 4」还是「级牌 2」，客户端渲染与向量断言各可能取一种 | 813 行「`u8[]`，按队伍序号升序」；885 行「存 `levelTable.ranks` 的数组下标」；819 行向量 `[2,2]` | 在 813 行补一句「元素为 `levelTable.ranks` 下标，与 `stateHash` 第 2 项同口径」 |

## 结论与修复顺序

本轮验证结论：**暂不可开工**——3 条 Major 必须先修（F9-01 `tribute` 类型列、F9-02 发牌区承载位、F9-03 `level.pass` 承载位），它们分别打掉 M0 的 L1 基准示例、M0 夹具的发牌声明、S8/S11 的过 A 口径切换。三条都是「补字段/改一行类型列」，不动既有取值口径，修完不需要重跑全轮评审，第 9 步复验即可。

其余 19 项已落地且自洽：定值向量可被第三方复算（129 字节、指纹一致）、`m0-minimal` 夹具走真实 `score-only` DSL、发布状态机与结算对账等式闭合、四王炸在 v1 且在压制阶梯末位、工期 40～44 周与第 16 周 gate 均未被改动；Minor/Nit 六项（F9-04 至 F9-11 中的轻量项）不阻断 M0，可在 M0 末尾随第一批实现回填。
