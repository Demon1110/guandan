# M0 纸面推演：两款掼蛋变体的 RuleSet 表达力实测

> 任务：`design_draft.md` §2.4 M0 gate 表第 2 项——「掼蛋变体/麻将纸面推演：只用 RuleSet（不写代码）写出两款最小对局，结构缺口逐条列出」。
> 产物性质：**分析文档**，不是可运行规则实现。全篇没有一行引擎代码、没有改动任何设计文档。
> 引用锚点：`docs/designs/2026-11-23-guandan-multiplatform/design_draft.md`，SHA256 = `d6a36f3e52f3170051af75ac7a0c987d94c4278434ccefef2b46cca396dc16aa`（215,828 字节，mtime 2026-11-24 08:30:10）。本文所有 `L<行号>` 均指该版本，可直接按行号复核。

---

## 1. 声明

### 1.1 本次做了什么、没做什么

| 项 | 内容 |
| --- | --- |
| 做了什么 | 用现行 RuleSet DSL（`§5.1` 顶层键表 + `§5.2` 原语参数表）逐句试写两款掼蛋变体的最小对局；用现成 L1 校验器只读试跑，把拒绝码与静默通过都当证据；把「现行键集里没有承载位」的结构逐条登记成缺口 |
| 没做什么 | ① 没写任何引擎代码；② 没改 `docs/designs/**`、`crates/**`、`.gitattributes`；③ 没执行任何 git 写操作；④ 没有为让 YAML 通过而修改校验器或设计文档 |
| 硬规矩 | RuleSet 里**只出现** `§5.1`/`§5.2` 已定义的键。表达不了的规则**不许发明键名塞进去假装表达成功**：YAML 里干脆省略 → 缺口表登记 → 另附键形提案（提案明确标注「不属于现行 schema」） |
| 判定门槛 | 「缺」= 现行键集与结构体字段清单里确无承载位；「绕法」= 能用现有键组合近似且偏差已注明；「近似」必须写明行为偏差，**近似不等于表达成功** |

### 1.2 为什么选这两款（压力互补，且都是真实可上线的玩法）

| 变体 | 是什么 | 真实出处 | 它压的是哪一类结构 |
| --- | --- | --- | --- |
| **A** | 四人两队、双副 108 张、每人 27 张的经典掼蛋，把 §5.13 的进贡链与轮转链**写满** | `§5.10` 冻结取值表（`L750–772`）就是这款的口径，是 v1 在役基线 | **流程链深度**：单贡/双下对位进贡/抗贡/还贡/首局跳过/接风/全过回末家/跳过已出完/首出来源——考原语串联与跨局状态入口 |
| **B** | 六人两队三人、三副牌 162 张、每人 27 张（隔一人落座 `[0,2,4]` vs `[1,3,5]`） | 边锋游戏《六人掼蛋游戏怎么玩》<http://www.gameabc.com/subjectinfo/5_24_2.html>；抖音教程《六人规则》<https://www.iesdouyin.com/share/video/7707161116656274728>（2026-11-12） | **维度参数化**：座位数 6、队人数 3、名次空间 1–6、按积分差分档升级、六王炸、9–12 头炸——考 schema 的定长数组、闭集枚举、取值上界 |

两款互补的理由：A 在**同座位数**下把流程链压到最深，暴露「引擎硬编码、RuleSet 无键」的那类缺口；B 把**座位数/名次空间/计分维度**整体顶到六人，暴露「类型层写死四人两人队假设」的那类缺口。两类缺口性质不同，一款压不出另一款的问题，因此两条都要写。

> 口径修正说明：派单时按「六人三队」起步，实现期核对边锋原文为**两队各三人**（隔一人落座、按两队积分总和之差升级），变体 B 按查证后的真实口径落笔。抖音教程给出的「三张大王/三张小王本身算一炸」「剩 ≤7 张须报牌」按**可选项**处理，未全部落进 YAML。

### 1.3 校验工具与分流口径（结论的读法）

- 试跑入口：把 `RuleSetBody` 交给 `crates/engine` 的 `from_yaml`（草稿态构造）+ `validate(body, [L1])`，只读，不写仓库。用的是**预编译探针**，未在仓库内 `cargo build`。
- 现行代码状态：**L1 结构校验是已实装的**（`crates/engine/src/ruleset/validate.rs` 全文无 `todo!()`，且模型全字段 `deny_unknown_fields`），**L2–L4 未实装**（`validate.rs` 文件头自述「本轮只实现 L1」）。`contract.rs` 的 `todo!()`（L268/L274/L280）属 **M1 原语执行层**（`legal_actions`/`apply`/`view`），**不是** L1。
- 由此得到两条读法，全篇严格区分：
  - `L1-CONSTRUCT: REJECT … unknown field/unknown variant` = **schema 缺口的硬证据**（键集里确实没有这个位，连构造都过不去）；
  - `L1-CONSTRUCT: OK` + `L1-FINDINGS: none` **不证明语义可表达**，只证明现行 ADT 吃得下这份外壳。语义级判定要到 L2（引用完整性/静态牌堆）与 M1 执行层才见真章。

---

## 2. 变体 A：四人双副「经典掼蛋 · 进贡链 + 轮转链全链加压」

### 2.1 设计意图

把 `§5.13` 的进贡全链（单贡、双下对位、抗贡、还贡、引擎自动选牌、首局跳过）与轮转全链（接风、全过回最后出牌者、跳过已出完座位、首出来源）压到极限，逐句比对 `§5.1`/`§5.2` 键集有没有承载位，重点暴露写成「引擎硬编码」而无对应键的结构：首出座位来源、局终止判据、名次并列决胜、顺子环形特例、跨 A 封顶、安全上限触顶决胜。

### 2.2 RuleSet 全文

```yaml
# 变体 A：四人双副「经典掼蛋 · 进贡链 + 轮转链全链加压」（草稿态 RuleSetBody，不含 version）。
# 依据：design_draft.md §5.1 / §5.2 / §5.10 / §5.13。
# 只使用 §5.1/§5.2 已定义的键；表达不了的规则在缺口表登记，不在本文件塞新键。
rulesetId: guandan-classic-variant-a
interpreter: ">=1.2,<2"

seats: { count: 4, teams: [[0,2],[1,3]] }

deck: { copies: 2, ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"], suits: [S,H,D,C], jokers: 2, exclude: [] }

zones:
  - { id: hand,    scope: private, capacity: 27 }
  - { id: discard, scope: shared,  capacity: -1 }

levelTable:
  id: guandan-level
  initial: "2"
  ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"]
  upgrade: [ { teamRanks: [1,2], step: 3 },
             { teamRanks: [1,3], step: 2 },
             { teamRanks: [1,4], step: 1 } ]

level:
  pass: { combos: [[1,2],[1,3]], streakLimit: 3, resetOnFail: true }
  current: { scope: by-team }

naturalOrder: ["2","3","4","5","6","7","8","9","10","J","Q","K","A",little-joker,big-joker]
levelInsert: above-A

wildcard: { source: { suit: H, rank: "@level" }, substitute: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"], inStraightFlush: false }

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
  flushRank: above-bomb-5

tribute:
  enabled: true
  trigger: last-round-rank
  pairs:
    - { payer: fourth,       receiver: first,  when: single-loss }
    - { payer: fourth,       receiver: first,  when: double-loss }
    - { payer: third,        receiver: second, when: double-loss }
  pay: { count: 1, selection: max-rank-except-wildcard }
  return: { ceiling: under-ten, count: 1, fallback: smallest }
  resist: two-big-jokers
  firstRound: skip

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
view: { schemaVersion: 1, fields: [hand, seat.levels, discard, lastPlay] }
timing: { turnTimeSec: 30, graceSec: 90, offlineAllSec: 300 }
ai: { features: [hand.count, last.play, partner.state, opponent.count], hint: false }
```

### 2.3 逐段说明：它想考什么、够不够

判定口径：「够」= 规则句子能落到 `§5.1`/`§5.2` 某个键 + 某个枚举取值；「缺」= 键集与结构体字段清单里确无承载位（对应缺口表编号）；「绕法」= 能用现有键组合近似且偏差已注明。

| 顶层块 | 想考什么 | 现行承载位（含行号/结构体） | 结论 |
| --- | --- | --- | --- |
| `rulesetId` / `interpreter` | 草稿态元信息、解释器区间 | `§5.1` L377–379 | 够 |
| `seats` | 4 座两队、对位组队 | `§5.1` L380；`model.rs` `Seats{count, teams}` L314–317 | 够（覆盖/重叠属 L2，见 §5.1） |
| `deck` | 双副 108 张、13×4×2+4 王 | `§5.1` L381；`model.rs` `Deck` L321–329 | 够（张数守恒属 L2） |
| `zones` | 私有手牌 27 上限、公共弃牌无上限 | `§5.1` L382；`model.rs` `ZoneDef` L333–338 | 够（容量合法性属 L2） |
| `levelTable` | 起点 2、13 档、升级档双下+3/1+3+2/1+4+1 | `§5.1` L383；`UpgradeRow` L350–355 | 升级档本身够；**跨 A 封顶无键**（G13） |
| `level` | 过 A 组合 1+2/1+3、连 3 局未过退起点、未过不退档、级牌每队一个 | `§5.1` L384；`LevelConfig` L385–389、`PassConfig` L368–375 | 够。队级 `levels`/`aFailStreak` 的跨局保持与开场初始化是房间状态语义（`§5.14` L908），RuleSet 只经 `current.scope: by-team` 声明口径，**不是缺口** |
| `naturalOrder` / `levelInsert` | 自然序含王、级牌插 A 后小王前 | `§5.1` L385–386 | 够 |
| `wildcard` | 红桃级牌逢人配、不得组同花顺 | `§5.1` L387；`model.rs` `Wildcard` L400–406 | 够 |
| `patterns` | 13 个牌型（单/对/三/顺 5/三连对/二连三/炸 4–8/同花顺/四王炸） | `§5.1` L388、`§5.2` L513–519；`params.rs` L173–205 | 大体够；**顺子端点环形特例无开关**（G2） |
| `compare` | 阶梯序、炸弹两级比较、同花顺插 5 炸之上 | `§5.1` L389；`CompareConfig` L483–490 | 够 |
| `tribute` | 单贡、双下对位 4→1 / 3→2、两家合计两王抗贡、还贡≤10、全大还最小、引擎自动选牌、首局跳过 | `§5.1` L390、`§5.2` L531–534；`TributeConfig` L517–528 | **配对/开关全够**。三处「写死在枚举语义里」记为内置冻结口径、不单列缺口：抗贡「两家合计不要求同人」、还贡「阈值 10」、贡牌「同点数取牌 id 最小」（`§5.2` L499 明示）。附：`enabled=true` 时 `return` 必填的 L1 校验未落位（§5 分流 L1-a） |
| `lifecycle` | 四段、`tribute` 为零动作 phase | `§5.1` L391、`§5.14` L908；`Phase` L59–69 | 够（四段固定不可增减，见 G3 的终止时机问题） |
| `actions` | 出牌受牌型约束并消耗手牌、pass 从 play 提交 | `§5.1` L392；`model.rs` `ActionDef` L532–537、`LegalFrom` L612–634 | 够 |
| `flow.deal` | 落点 hand、每人 27、不公开 | `§5.1` L404–406；`params.rs` `Shuffle` L158–162 | 够 |
| `flow.turn` | 顺时针、跳过已出完、接风、全过回最后出牌者；**首出座位来源** | `§5.1` L407–410；`TurnFlow` L758–766；轮转原语 `§5.2` L527–530 | 局内轮转四键够；**首出来源缺**（G1） |
| `flow.score` / `flow.rng` / `flow.assert` | 按队归集名次、RNG stream、4 人 108 张断言 | `§5.1` L411–415 | 够 |
| `matchCap` | 安全局数上限 12 | `§5.1` L394；`MatchCap` L691–694 | 局数够；**触顶后决胜口径无键**（G14） |
| `settle` | 按队升级、引用唯一级牌表；局终止时机、1–4 名、并列决胜、跨 A 封顶 | `§5.1` L393；`Settle` L640–688 | 变体与表引用够；**三处判定规则缺**（G3/G4/G13） |
| `view` | 白名单 hand / seat.levels / discard / lastPlay | `§5.1` L395；`ViewConfig` L719–724 | 够 |
| `timing` / `ai` | 计时三键、AI 特征与提示开关 | `§5.1` L396–397、`§5.11` L774–782 | 够。托管「出最小合法牌、同力按 id 升序」是 AI 实现层策略（`§5.10` L770），不进 RuleSet，**不是缺口** |

### 2.4 L1 试跑记录（只读，四轮）

```
轮 1  交付件 variant-A.yaml
== file: …/variant-A.yaml
L1-CONSTRUCT: OK  rulesetId=guandan-classic-variant-a
L1-ranLevels: {L1}
L1-FINDINGS: none
```

```
轮 2  反证：把 G1 的提案键 firstLead 塞进顶层
L1-CONSTRUCT: REJECT  RS_SCHEMA_INVALID @ <root>: unknown field `firstLead`, expected one of
`rulesetId`, `interpreter`, `seats`, `deck`, `zones`, `levelTable`, `level`, `naturalOrder`,
`levelInsert`, `wildcard`, `patterns`, `compare`, `tribute`, `lifecycle`, `actions`, `settle`,
`matchCap`, `view`, `timing`, `ai`, `flow`
```

```
轮 3  反证：把 G2 的提案键 aceLow 塞进 pattern.straight.params
L1-CONSTRUCT: REJECT  RS_SCHEMA_INVALID @ <root>: unknown field `aceLow`, expected `length` or `wildcardAllowed`
```

```
轮 4  探针：tribute.enabled=true 但省略 return 段（查 L1-a）
L1-CONSTRUCT: OK  rulesetId=guandan-classic-variant-a
L1-ranLevels: {L1}
L1-FINDINGS: none
```

读法：轮 2/轮 3 的 REJECT 是 **schema 缺口的硬证据**——`deny_unknown_fields` 在构造期即拒绝，且错误信息直接吐出顶层 21 键闭集、`pattern.straight` 仅 `length`/`wildcardAllowed` 两键，说明 G1/G2 无法靠换写法绕过。轮 4 的「构造 OK 且无 finding」是**校验未落位**（L1-a），不是 schema 缺口。轮 1 的 `FINDINGS: none` 只说明这份外壳被现行 ADT 接受，不说明 §5.13 的每句话都被表达（G1–G14 属「引擎硬编码无键」，L1 结构校验本来就不会报）。

---

## 3. 变体 B：六人两队三人（三副牌）「掼蛋 · 非 4 人座加压」

### 3.1 设计意图

考「非 4 人座」连带出的一整组结构性硬墙：座位数与队伍划分（6 座、两队三人、隔一人落座）、名次编码（1–6 而非 1–4）、按积分差分档的升级表、三人队的过 A 全称判定、进贡名次枚举、炸弹张数上界（最高 12 张）、王炸张数（六王）、局名次分值表。它与变体 A 的互补点：A 考的是原语串联的**流程深度**，B 考的是「四人两人队」这个前提被写死在类型里的**定长数组/闭集枚举/取值上界**。

### 3.2 RuleSet 全文

```yaml
# 变体 B：六人三副牌「掼蛋 · 非 4 人座加压」纸面推演 RuleSet（草稿态 RuleSetBody，不含 version）。
# 口径：边锋《六人掼蛋游戏怎么玩》为主；抖音教程补充条目（逆时针、三张王一炸、报牌）按可选项在说明里写。
# 标注【近似】的块：现行键集只能近似，行为偏差见后文；标注【缺口】的规则在本 YAML 中省略，
# 缺口登记与提案键形见第 4、6 节。本文件不是可跑规则实现。
rulesetId: guandan-six-player-3v3
interpreter: ">=1.2,<2"
seats: { count: 6, teams: [[0,2,4],[1,3,5]] }
deck: { copies: 3, ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"], suits: [S,H,D,C], jokers: 2, exclude: [] }
zones:
  - { id: hand,    scope: private, capacity: 27 }   # 162 ÷ 6 = 27，每座上限 27
  - { id: discard, scope: shared,  capacity: -1 }
levelTable:
  id: guandan-six-level
  initial: "2"
  ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"]
  # 【近似】六人局升级按两队积分总和之差分档（差≥7 +3 / 4–6 +2 / 1–3 +1 / <1 +0），
  # 现行 upgrade 只能按两名次组合查表；下三行沿用四人基线组合占位，不表达真实分差规则。
  upgrade: [ { teamRanks: [1,2], step: 3 },
             { teamRanks: [1,3], step: 2 },
             { teamRanks: [1,4], step: 1 } ]
level:
  # 【近似】真实过 A 条件为「赢家所有队友都没有末游」（三人队），combos 两名次组合表达不了；
  # 此处保留基线默认组合占位。
  pass: { combos: [[1,2],[1,3]], streakLimit: 3, resetOnFail: true }
  current: { scope: by-team }
naturalOrder: ["2","3","4","5","6","7","8","9","10","J","Q","K","A",little-joker,big-joker]
levelInsert: above-A
wildcard: { source: { suit: H, rank: "@level" }, substitute: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"], inStraightFlush: false }
patterns:
  - { id: single,           primitiveId: pattern.same-rank,           params: { count: 1 } }
  - { id: pair,             primitiveId: pattern.same-rank,           params: { count: 2 } }
  - { id: triple,           primitiveId: pattern.same-rank,           params: { count: 3 } }
  - { id: straight-5,       primitiveId: pattern.straight,            params: { length: 5 } }
  - { id: plate-3,          primitiveId: pattern.consecutive-pairs,   params: { pairs: 3 } }
  - { id: steel-2,          primitiveId: pattern.consecutive-triples, params: { triples: 2 } }
  # 【缺口】六人局炸弹最多 12 张（九/十/十一/十二头），现行 bomb count 闭集 4–8，9–12 无法声明。
  - { id: bomb-4,           primitiveId: pattern.bomb,                params: { count: 4 } }
  - { id: bomb-5,           primitiveId: pattern.bomb,                params: { count: 5 } }
  - { id: bomb-6,           primitiveId: pattern.bomb,                params: { count: 6 } }
  - { id: bomb-7,           primitiveId: pattern.bomb,                params: { count: 7 } }
  - { id: bomb-8,           primitiveId: pattern.bomb,                params: { count: 8 } }
  - { id: straight-flush-5, primitiveId: pattern.straight-flush,      params: { length: 5 } }
  # 【缺口】王炸无参数位：真实为六王炸（三大王 + 三小王），joker-bomb 只能写空对象。
  - { id: joker-bomb,       primitiveId: pattern.joker-bomb,          params: {} }
compare:
  ladder: [single, pair, triple, straight-5, plate-3, steel-2, bomb-4, bomb-5, bomb-6, bomb-7, bomb-8, joker-bomb]
  bombOrder: count-then-rank
  flushRank: above-bomb-5
tribute:
  # 【缺口】六人局五游/末游无法在进贡枚举中命名；主版本按关闭进贡处理，避免触发枚举闭集。
  # 【未落位】文档称 enabled:false 可省略其余子键，代码要求 pairs 必填，此处补空表占位。
  enabled: false
  pairs: []
lifecycle: [deal, tribute, play, settle]
actions:
  - { kind: play,    primitive: action.require-legal, legalFrom: patterns, params: { force: true, consume: hand } }
  - { kind: pass,    primitive: action.pass,         legalFrom: play,     params: {} }
flow:
  deal:   { zone: hand, perSeat: 27, reveal: false }
  turn:   { direction: counterclockwise, skipFinished: true, partnerLead: true, afterAllPass: last-player }
  score:  { dimension: team }
  rng:    { stream: 0 }
  assert: { minPlayers: 6, cardCount: 162 }
matchCap: { rounds: 12 }
settle: { algorithm: level-up, levelTableRef: guandan-six-level }
view: { schemaVersion: 1, fields: [hand, seat.levels, discard, lastPlay] }
```

### 3.3 逐段说明：它想考什么、够不够

| 顶层块 | 想考什么 | 现行承载位（含行号/结构体） | 结论 |
| --- | --- | --- | --- |
| `seats` | 6 座、两队三人、隔一人落座 | `§5.1` L380（`count` 2–8）；`Seats` L314–317 | **结构够**：`count: 6` 被 L1 接纳（探针主版本 `CONSTRUCT: OK`），`[[0,2,4],[1,3,5]]` 类型上可承载。覆盖/重叠属 L2（未实装，§5 分流 L1-c）。注：**「队人数 3」在 DSL 里没有独立键位**，终局判定按引擎模型假设走（并入 G3） |
| `deck` | 三副 162 张：13×4×3 + 2×3 | `§5.1` L381 | **牌堆层够**。`jokers` 固定 2（=1 大王+1 小王），六王由 `copies=3 × jokers=2` 得出，牌堆自洽；162 ÷ 6 = 27 整除，无需余量发牌 |
| `zones` | 6 × 27 = 162，与牌堆相等 | `§5.1` L382 | 够（容量合法性属 L2） |
| `levelTable` | 按**两队积分总和之差**分档升级（≥7 +3 / 4–6 +2 / 1–3 +1 / <1 +0） | `§5.1` L383；`UpgradeRow.team_ranks: [u8;2]`、`step: u8` | **缺，YAML 内为近似**。判定域只有「两名次组合→step」，无分差区间键；定长 2 装不下三人队；`step` 值域 1–3 无 +0 档（G5/G7） |
| `level` | 过 A = 赢家所有队友都没有末游；累计三次没过退回 2 | `§5.1` L384、`§5.2` L537；`PassConfig.combos: Vec<[u8;2]>` | **缺，YAML 内为近似**。真实条件是三人队全称判断，两名次组合 + 值域 1–4 双重不够（G11/G5）。`streakLimit: 3` 与「三次退回」对上，`current.scope: by-team` 够 |
| `naturalOrder` / `levelInsert` | 牌力序与级牌插入位 | `§5.1` L385–386 | 够 |
| `wildcard` | 红桃级牌逢人配 | `§5.1` L387 | 够（三副自然 3 张） |
| `patterns` | 单/对/三/顺 5/连对/钢板/炸/同花顺/**六王炸**；炸弹最多 12 张 | `§5.2` L513–519；`params.rs` `BombRaw{count: u8}`、`JokerBomb` 无参 | **两处缺**：9–12 头炸无合法 `count`（G9，`§5.2` L517 闭集 4–8）；王炸无参数位（G10，`§5.2` L519「无参：只能为空对象」）。另：`bomb.count` 区间在代码侧未接 L1，`count: 12` 静默构造成功（§5 分流 L1-b） |
| `compare` | 六王 > 六头及以上炸 > 同花顺 > 五头炸 > 四头炸 | `§5.1` L389 | **在 4–8 张范围内够**；9–12 头炸缺位后阶梯无法完整列出（受 G9 连带），王炸张数不可配（G10） |
| `tribute` | 六人局进贡配对（五游/末游） | `§5.1` L390；`SeatRank` 枚举 L119–129 | **缺**：枚举只有 first/second/third/fourth，第 5/6 名无处落（G6）；主版本以 `enabled: false` 规避，且被 `pairs` 必填卡了一次（§5 分流 L1-a） |
| `lifecycle` | 四段 | `§5.1` L391；`validate.rs` L236–248 | 够 |
| `actions` | play/pass；抖音补充的「剩 ≤7 张须报牌」 | `§5.1` L392、`§5.2` L497/L547 | 基线够；**报牌属宣告类动作，v1 无承载位**（G12，`action.declare` 为 v2 预留） |
| `flow.deal` | 每人 27、162 整除 | `§5.1` L404 | 够（整除约束满足）。首出座位见 G1（四人/六人同受约束，不重复计） |
| `flow.turn` | 逆时针、接风、全过回末家 | `§5.1` L407–410 | 够（`counterclockwise` 在枚举内） |
| `flow.score` | 按队归集 | `§5.1` L411、L419、L501 | 维度够；**名次→分值（5/4/3/3/1/0）无承载位**（G8，`score.table` 为 v2 预留） |
| `flow.rng` / `flow.assert` | RNG 流、6 人 162 张断言 | `§5.1` L412–414 | 够 |
| `matchCap` | 安全局数上限 | `§5.1` L394 | 够 |
| `settle` | level-up 挂级牌表 | `§5.1` L393 | 结构够；升级语义被 G5/G7/G8 卡住；三人队终局与 5/6 名次槽位需在 M1 结算纯函数改（并入 G3） |
| `view` | 白名单 | `§5.1` L395 | 够 |

### 3.4 L1 试跑记录（只读，六轮）

```
轮 0  主版本首稿（未写 pairs）
L1-CONSTRUCT: REJECT  RS_SCHEMA_INVALID @ <root>: missing field `pairs`
```

```
轮 1  主版本（补 pairs: []）
L1-CONSTRUCT: OK  rulesetId=guandan-six-player-3v3
L1-ranLevels: {L1}
L1-FINDINGS: none
```

```
轮 2a 反证：进贡枚举第 5 名 pairs: [{payer: fifth, receiver: first, when: single-loss}]
L1-CONSTRUCT: REJECT  RS_SCHEMA_INVALID @ <root>: unknown variant `fifth`,
expected one of `first`, `second`, `third`, `fourth`
```

```
轮 2b 反证：joker-bomb params: { count: 6 }
L1-CONSTRUCT: REJECT  RS_SCHEMA_INVALID @ <root>: unknown field `count`, there are no fields
```

```
轮 2c 反证：pass.combos: [[1,5]] + upgrade step: 0 + bomb count: 12
L1-CONSTRUCT: OK  rulesetId=guandan-six-counter-ranks
L1-ranLevels: {L1}
L1-FINDING: RS_SCHEMA_INVALID | Blocker | levelTable.upgrade.step | step 必须在 1–3 之间，实际 0
L1-FINDING: RS_SCHEMA_INVALID | Blocker | level.pass.combos | 名次组合 [1, 5] 元素必须在 1–4 之间
```

```
轮 2d 反证：teams: [[0,3],[1,4],[2,5]]（错误分组，查 L2 覆盖校验）
L1-CONSTRUCT: OK  rulesetId=guandan-six-counter-teams
L1-ranLevels: {L1}
L1-FINDINGS: none
```

```
轮 2e 反证：upgrade 行 teamRanks: [1,5]
L1-CONSTRUCT: OK  rulesetId=guandan-six-counter-upgrade-rank
L1-ranLevels: {L1}
L1-FINDINGS: none
```

读法：轮 2a/2b 的 REJECT 是 **schema 缺口硬证据**（G6/G10 连构造都过不去）；轮 2c 里 `step: 0` 与 `[1,5]` 的 Blocker 是 **取值闭集生效**（G7/G5 的子项），而同轮 `bomb count: 12` **静默通过**说明上界 8 只是文档闭集、未接校验（上界本身是 G9，校验缺失是 L1-b）；轮 2d/2e 的 `FINDINGS: none` 是 **L2 与 L1 值域校验未落位**（L1-b/L1-c），不是 schema 缺口。轮 1 的 `CONSTRUCT: OK` 只代表外壳被吃下——两份近似（upgrade 组合占位、pass combos 占位）**不等同表达成功**。

---

## 4. 缺口表（合并去重后 14 条 schema 缺口）

编号映射：G1←G-A1，G2←G-A2，G3←G-A3（并入变体 B 的三人队终局补充），G4←G-A4，G5←G-B1，G6←G-B7，G7←G-B2，G8←G-B3，G9←G-B4，G10←G-B5，G11←G-B6，G12←G-B8，G13←G-A5，G14←G-A6。两段子结果中重复的「同源缺口」（名次编码 1–4 在 `pass.combos` 与 `upgrade.teamRanks` 两处）已并为 G5，`tribute` 枚举独列为 G6 因为它是**枚举**而非 u8 数组，修法不同。

| 编号 | 缺口 | 文档位置与原文引用 | 为什么现行 schema 表达不了 | 候选新原语或键形（提案，均**不属于**现行 schema） | 影响 phase | 是否阻断 M0 |
| --- | --- | --- | --- | --- | --- | --- |
| **G1** | 首出座位来源：首局固定座位 0、之后由上局头游先出，无可配的来源声明 | `L858`「v1 固定座位 0 先出；…之后由上局头游先出」 | `flow.turn` 仅四键 `direction`/`skipFinished`/`partnerLead`/`afterAllPass`（`L407–410`）；轮转四原语 `turn.next`/`after-all-pass`/`skip-finished`/`return-to-last`（`L527–530`）全部只描述局内交接；`TurnFlow` L758–766 无首出字段。反证轮 2：塞 `firstLead` 在构造期被拒并吐出 21 键闭集 | `flow.turn.firstLead: { source: fixed-seat / last-round-first / last-round-last / lowest-level, seat?: u8 }` | deal；tribute→play | 否（基线引擎内置，与 §5.13 一致；换变体才暴露） |
| **G2** | 顺子端点环形特例：A 可前接 2 组 `A-2-3-4-5`，但 `Q-K-A-2-3` 非法、A 后接 2 非法，无开关 | `L875`「允许从 A 起（`A-2-3-4-5`）但不允许绕回（`Q-K-A-2-3` 非法）」 | `pattern.straight` 参数仅 `length: u8` 与 `wildcardAllowed: bool`（`L514`；`params.rs` `StraightRaw`），无 ace-low / 环形 wrap 字段。反证轮 3：塞 `aceLow` 被拒，报错只认这两键 | `pattern.straight.params.aces: { low: bool, wrap: bool }` | play | 否（基线由引擎内置判定） |
| **G3** | 局终止判据：某队两人均已出完即结束；三人队（变体 B）与「剩 N 人结束」类变体无终止谓词 | `L886`「一局在某一队两人均已出完牌时结束」；`L1011`「`play → settle`：某一队两人均已出完牌」 | `settle.algorithm` 只有 `level-up`/`score-only` 两 ADT 变体（`L393`），只决定升级方式；`lifecycle` 四段固定不可增减（`L391`）；全键集无终止条件结构，「队人数」本身也无键位 | `settle.termination: { kind: team-all-finished / n-remaining / hand-empty, params: { remaining?: u8, teamSize?: u8 } }` | play→settle | 否（基线内置；跨座位数变体必须改引擎） |
| **G4** | 名次并列决胜口径：双下收尾剩两人按剩余手牌张数升序排 3/4 名，同张数按座位号小者在前 | `L886`「按剩余手牌张数升序排 3、4 名（少者三游），张数相同时按座位号小者在前」 | 比较原语 `compare.by-rank`/`by-count`/`by-ladder`/`bomb-order`（`L520–523`）只比牌型大小，不产出名次；`Settle` L640–688 与 `LevelTable` L359–365 均无名次决胜字段 | `settle.rankTiebreak: { primary: hand-count-asc / seat-asc, tie: seat-asc }` | settle | 否（基线内置） |
| **G5** | 名次编码闭集 1–4：`pass.combos` 与 `upgrade.teamRanks` 都是**定长 2 位数组**且值域 1–4，六人局名次 5/6 与三人队第 3 名进不来 | `L384`「元素为两名次组合、取值 1–4 且两值不同」；`L537`「`combos: [[u8;2]]`」；`L383`「`teamRanks: [u8;2]`」 | `PassConfig.combos: Vec<[u8;2]>`、`UpgradeRow.team_ranks: [u8;2]`（`model.rs` L370、L353）；反证轮 2c：`[1,5]` 被 L1 判 Blocker「元素必须在 1–4 之间」——**拒绝本身证明值域无承载位** | 名次类型参数化为 `rank: u8`（值域 1–`seats.count`），三处共用；`combos`/`teamRanks` 元素长度随 `teamSize` 放宽 | level / settle | 否（四人基线不受影响；六人上线前必补 = 阻断 M1） |
| **G6** | 进贡名次枚举闭集 `first`~`fourth`：六人局五游/末游不能作贡方/受方 | `L390`「`payer`/`receiver` 为闭集枚举 `first`/`second`/`third`/`fourth`…无 `last` 别名」 | `SeatRank` 四值枚举（`model.rs` L119–129）；反证轮 2a：塞 `fifth` 构造期被拒「unknown variant」。主版本只能 `enabled: false` 规避 | 随 G5 参数化后 `pairs[].payer/receiver` 接受 `rank: u8`；三人队对位进贡再扩 `when` 分支 | tribute | 否（同上：阻断六人上线） |
| **G7** | 升级判定域：六人局按两队积分总和之差分档，且「<1 不升级」需要 `step: 0` | `L383`「`upgrade[]` 元素为 `{teamRanks: [u8;2], step: u8}`…`step` 取值 1–3」 | `UpgradeRow` 只有「两名次组合→step」一种判定域，无分差区间带；`step` 值域 1–3 无 +0（反证轮 2c：`step: 0` 被判 Blocker）。主版本三行只是近似占位，同组合在六人局可对应不同分差，升级数会错配 | `upgrade` 增 `basis: score-diff`，档形 `{ bands: [{minDiff, maxDiff, step}], teamSize: u8 }`，`step` 允许 0（依赖 G8 的分值表） | settle | 否（阻断六人上线） |
| **G8** | 局名次分值表（头 5 / 二 4 / 三 3 / 四 3 / 五 1 / 末 0）无承载位 | `L419`「v1 不建独立计分表本体；`score.table` 原语与其 `tableRef` 键移出 v1 清单、标 v2 预留」；`L501`、`L547` 同 | `flow.score.dimension` 仅 `team`/`player` 两值（`§5.1` L411）；v1 明确不挂外部分值表，故 G7 的分差也无源数据 | 恢复 `score.table`（v2 预留）或顶层 `scoreTable: { id, points: [u8; N] }` 供升级判定引用 | settle | 否（阻断六人上线） |
| **G9** | 炸弹张数上界 8：三副牌同点最多 12 张，九/十/十一/十二头炸无法声明 | `L495`「`bomb` 支持 4～8」；`L517`「`pattern.bomb`｜`count: u8`｜4–8，必填」；`L760`「4 张 < 5 张 < … < 8 张」 | `pattern.bomb` 的 `count` 文档闭集 4–8；`flushRank` 四值锚点也只到 `above-bomb-8`（`L389`）；12 头炸无合法取值（`count: 12` 目前静默通过，说明连校验也没有，见 L1-b） | `bomb.count` 上界由牌堆可成炸张数推导（`maxCount: u8`，默认 8）；`flushRank` 锚点补 9–12 档 | play / compare | 否（阻断六人上线） |
| **G10** | 王炸无参数位：六王炸（三大王+三小王）与「三张同王本身一炸」都表达不了 | `L519`「`pattern.joker-bomb`｜无参：只能为空对象 `{}`，出现任何键即拒绝」；`L762`「四王炸 最高」 | `PrimitiveParams::JokerBomb` 是无参变体（`params.rs` L81）；王数写死在引擎，既改不成 6，也不能声明「三张同王」的次级王炸。反证轮 2b：`count` 被拒「there are no fields」 | `pattern.joker-bomb` 参数化 `{ bigCount: u8, littleCount: u8 }`；或新增 `pattern.joker-set` 按王数分级 | play / compare | 否（阻断六人上线） |
| **G11** | 过 A 条件：三人队为「赢家所有队友都没有末游」的全称判断，两名次组合表达不了 | `L384`「`pass.combos: [[u8;2]]`…两名次组合」；`L537` 同 | `PassConfig.combos` 定长 2、值域 1–4（`model.rs` L370）；真实条件是对「其余 2 名队友名次集合」的全称否定，不是固定两两配对，定长与值域双重不够 | `level.pass` 增谓词式条件 `{ teammateCount: u8, mustNotIncludeRanks: [rank] }` | settle | 否（阻断六人上线） |
| **G12** | 报牌宣告动作（剩 ≤7 张被问须报数）无动作原语 | `L392`「`kind` 是 v1 闭集枚举，仅 `play`/`pass`；`claim`/`declare`…移出 v1、标 v2 预留」；`L497`、`L547` | `actions[].kind` 闭集只有 play/pass，无宣告类原语与 payload；手牌数通告在 v1 ADT 无承载位 | v2 启用 `action.declare`，参数 `{ threshold: u8 }` | play | 否（该条出自抖音补充口径，属可选项；阻断该可选项上线） |
| **G13** | 升级跨 A 封顶：到达 A 后超额升级数作废、不累积，无封顶/结转键 | `L898`「`level` 到达 A 后所有超额升级数作废，不累积」；`L900`「跨 A 的超额升级数作废（到 A 即停，不累积）」 | `UpgradeRow` 只有 `{teamRanks, step}`（`model.rs` L350–355），`level.pass` 只有 `combos`/`streakLimit`/`resetOnFail`（`PassConfig` L368–375）；「超额结转还是作废」无键，到 A 即停是结算纯函数内置行为 | `levelTable.cap: { atTop: clamp / carry-over }` | settle | 否（基线内置；族内变体想改口径须改引擎） |
| **G14** | 场次触顶决胜：未过 A 达 `matchCap` 时比 `levels` 档位、同档平局 | `L902`「未过 A 达上限时，以各队当前 `levels` 档位较高者为胜方，档位相同记平局」 | `matchCap` 只有 `rounds: u16`（`§5.1` L394；`MatchCap` L691–694），定位是防死循环上限；触顶后的胜方判据（比档位/比积分/平局）与 `aborted` 分支无键 | `matchCap.onReach: { winnerBy: level-rank / score / draw, tie: draw }` | settle；match 终止 | 否（基线内置） |

**缺口结构分布（直接支撑第 8 节结论）**：

- **同座位数也会撞上的 6 条**：G1、G2、G3、G4、G13、G14——全是「`§5.13` 写成引擎硬编码句子、RuleSet 无键」的**冻结口径**，换一款四人变体也得改引擎。
- **只在座位数/队伍规模/计分维度变化时才撞上的 8 条**：G5–G12——集中在定长数组、闭集枚举、取值上界、计分表四类**类型层假设**。
- **阻断 M0 的：0 条**。按 gate 表「未达标处置」（`L193`「表达不了的结构列为新原语候选，不进 M0 编码」），缺口的正确去向就是候选清单，因此不阻断 M0。真正阻断的是阶段：G5/G6/G7/G8/G9/G10/G11 阻断**六人变体上线（M1/M3）**；G1/G3 阻断**族内变体零代码配置化**；G2/G4/G12/G13/G14 可推后。

---

## 5. 「L1 未实装」与「schema 表达不了」的分流

第 4 节的 14 条是**schema 缺口**（键集里没有位）。本节 4 条是**另一类问题**，性质完全不同：键存在、文档也写了，但代码侧没落位或没接进校验。两类必须分开，否则会把「校验没做」误记成「设计缺失」，或反过来把「设计缺失」当成「实现欠债」放过去。

| 编号 | 性质 | 是什么 | 代码证据 | 判定 |
| --- | --- | --- | --- | --- |
| **L1-a** | ① 模型必填性与文档不一致（两处反向） | 文档：`enabled=true` 时 `return` 段必填、缺键即 `RS_SCHEMA_INVALID`（`L390`）；且 `enabled=false` 时其余子键可整段省略、只写 `enabled: false` 即合法（同 `L390`）。实测：前半条**没校验**（变体 A 轮 4：`enabled: true` 省略 `return` → `OK` / `FINDINGS: none`）；后半条**校验过严**（变体 B 轮 0：只写 `enabled: false` → `REJECT missing field pairs`） | `model.rs` L522–523 `return_: Option<TributeReturn>`（可选，无必填约束）；`TributeConfig.pairs: Vec<TributePair>` 非 `Option` 且无 `#[serde(default)]`；`validate.rs` 全文 `tribute` 只命中一条 lifecycle 文案 | **校验未落位**，不是 schema 缺口。`return`/`pairs` 两个键都在现行键集里，缺的是条件必填判定。会在纸面推演里造成「写对了反而报错、写错了反而不报」，M1 前应修 |
| **L1-b** | ① L1 参数值域未接 | 文档明确 `bomb.count` 4–8（`L517`）、`teamRanks` 名次 1–4（`L383`/`L537`），但 L1 对两者都不查：`bomb count: 12` 静默通过（变体 B 轮 2c）、`upgrade.teamRanks: [1,5]` 静默通过（轮 2e） | `params.rs` L195–197 `BombRaw { count: u8 }`（裸 u8 无区间）；`validate.rs` L97–113 upgrade 段只查重复与 `step 1..=3`，名次 1–4 检查只在 `pass.combos` 段（L126–142） | **校验未落位**。注意这里有两个结论要分开：**上界 8 本身是 schema 缺口（G9）**，而「上界没被校验」是校验欠债（本条）。不能用一个掩盖另一个 |
| **L1-c** | ② L2–L4 未实装 | `§5.4` L570 的 L2 检查（队伍覆盖、静态牌堆张数守恒、容量合法性、ID 引用完整性、phase 可执行原语、级牌表唯一引用）目前一条都不跑 | `validate.rs` L1–4 文件头自述「本轮只实现 L1；L2–L4 本轮不实现」；L15–19 `if !run_l1 { return result; }`；`contract.rs` L47–49 已有 `RsDeckCountMismatch`/`RsTeamCoverageInvalid`/`RsCapacityInvalid` 等标签，但 `validate.rs` 内无对应 push 分支（实测：三队两人的错误分组 `[[0,3],[1,4],[2,5]]` → `FINDINGS: none`） | **未实装，且不是 schema 问题**。所以第 4 节所有「够」结论里凡涉及覆盖/张数/引用的，只代表**文档给了承载位**，尚未经 L2 验证 |
| **L1-d** | ③ M1 执行层（**明确不属于 L1**） | `§5.13` 的语义判定（终局、名次、升级、接风实现）要跑起来才有意义，而执行层现在是桩 | `contract.rs` L268/L274/L280 三处 `todo!("M1 补原语实现，接口形状在 M0 锁定，不得改")`，分别在 `legal_actions`/`apply`/`view`；`engine.rs` L332–333 `TODO(M1-GAP-patterns)`：pair/triple/straight/…/joker-bomb 牌型原语本批不实现 | **既不是 schema 缺口，也不是 L1 缺口**。列出来是为了防止误读：本文所有「够 / 缺」都是**纸面表达力**判断，不是运行验证 |

一句话口径：**`REJECT unknown field/unknown variant` = schema 没有位（第 4 节）；`OK 但文档要求的行为没发生` = 校验或执行层欠债（本节）。两者在本文里从不混记。**

---

## 6. 为什么本次不推演麻将，以及它会额外考验哪些结构

U2 已定 v1 完全不碰麻将，`§2.4` gate 表那句「掼蛋变体/麻将纸面推演」里的麻将半句在本次范围外，**不是遗漏**。但要说清它对第 8 节结论的影响，所以单列本节——不写麻将规则，只回答「它还会额外考验什么」。

| 麻将的结构 | 现行 v1 的承载状态 | 会额外考验什么 |
| --- | --- | --- |
| 摸打循环（发牌后逐张摸、摸打交错、牌墙剩余） | `deal.draw`/`deal.reveal` 明确「v1 未启用」（`L494`）；`deal.shuffle` 一次性发完（`L404`），无牌墙剩余与逐张摸取 | 需要「可寻址牌堆 + 摸/打交替」的 phase 与动作，现行四段 `lifecycle` 装不下 |
| 副露/吃碰杠/宣告 | `action.claim`/`action.declare` 均为 v2 预留（`L392`、`L497`、`L547`），`kind` 闭集只有 play/pass | 需要「他人打出的牌可被第三方取走」的跨座位动作语义，`actions[].legalFrom` 三取值（patterns/zone/phase）表达不了 |
| 和牌判定（多牌型组合凑一副） | `patterns` 是「一手牌构成一个牌型并互相压制」的语义（`L495`、`§5.13` 出牌段） | 需要「手牌集合的子集分解 / 组合匹配」判定，与「单一牌型 key 比较」是两种模型 |
| 番种/计分表 | `score.table` v2 预留（`L419`、`L501`、`L547`）；v1 只有 `dimension: team/player` | 需要可配置的番种权重表与查表计分，正是 G8 想要而 v1 明确不给的东西 |
| 荒庄/流局与庄家轮转 | 局终止只认「某队两人出完」（`L886`/`L1011`，G3）；`seats.teams` 是静态队划分，**无阵营玩法 v2 预留**（`L380`） | 需要「无人和牌」的终止分支 + 每局座次/庄家变更，`seats`/`teams` 的静态性挡路 |
| 宝牌/翻宝 | 无任何键；`wildcard` 是单一 `{suit, rank}` 源 | 需要「动态翻出的宝牌」这类运行期派生的通配源 |

结论：麻将不只是「再加几个牌型原语」，它撞的是**摸打循环 + 副露 + 组合判定 + 番种计分 + 动态座次**五组结构，其中 3 组（claim/declare、score.table、无阵营）在设计里已被明确推到 v2。这也是本次两款变体都不选麻将的现实理由：拿它做推演只会得到一张「v1 明确不做」的清单，对 M0 的排期没有信息量。

---

## 7. 本次未覆盖的边界（诚实登记，不计入缺口条数）

- **单副牌 54 张 / 非整除发牌**：`§5.1` L404 明确「不整除时该默认值无定义，必须显式声明 `perSeat`，否则 L1 `RS_SCHEMA_INVALID`（v1 不定义余量发牌）」。4 人单副 54 张 = 13.5 张/人，落在 v1 明确排除的余量发牌上。本次两款变体都整除（108÷4=27、162÷6=27），未实测该分支——它是**已知的 v1 明确排除项**，不是本次新发现。
- **`seats.count` 的 2/3/5/7 座**：L1 接受 2–8（实测 6 座通过），但 `teams` 覆盖校验属 L2（未实装，L1-c），三人各自为战那类「无阵营」玩法又属 v2 预留（`L380`）。本次未展开。
- 上两条都不影响第 4 节的缺口计数与第 8 节结论；列在这里是为了不让「没测」被读成「测过且没问题」。

---

## 8. A14 结论：「通用 DSL 能表达国内主流棋牌」——**带保留成立**

| 判定面 | 结论 | 支撑 |
| --- | --- | --- |
| 基线掼蛋（四人双副、在役那款） | **成立** | `guandan-classic` 夹具 L1 实测 `CONSTRUCT: OK` / `FINDINGS: none`；`§5.1` 示例 YAML（L425–483）与代码闭集一致 |
| 同座位数族内变体（变体 A 型） | **带保留成立** | 21 个顶层键全部落地，`§5.13` 的进贡链/轮转链/升级档/过 A 全部有承载位；但 G1/G2/G3/G4/G13/G14 六处冻结口径无键，换变体须改引擎 |
| 跨座位规模变体（变体 B 型） | **不成立** | 8 条缺口卡在类型层：名次闭集 1–4（G5/G6）、分差升级与分值表（G7/G8）、炸弹 9–12 与王炸张数（G9/G10）、三人队过 A（G11）。外壳能构造成功，核心判定是近似 |
| 麻将类（摸打循环 + 副露 + 番种） | **不成立**（v1 明确不做） | 第 6 节五组结构，其中 3 组已被设计推到 v2 |

**结论：带保留成立。** 准确表述是——对「同座位数、以牌型压制 + 升级结算为主轴的出牌类棋牌（掼蛋及其族内规则变体）」成立；对「座位数/队伍规模/名次空间/计分表维度变化的变体」是**外壳成立、判定不成立**；对「摸打循环 + 副露 + 番种计分」类棋牌不成立。

支撑条数：**14 条 schema 缺口**（同座位数 6 条 + 跨座位规模 8 条）+ **4 条校验/执行层欠债**（L1-a~d，不参与结论但决定「能不能验」）+ **2 款变体实测**（A：`CONSTRUCT OK` 无 finding；B：外壳 OK、两处近似）+ **1 条基线通过**（`guandan-classic`）。

要跨过「不成立」那部分，最小改动集是三条（按优先级）：

1. **阻断 M1 · 名次类型参数化（G5 + G6）**：把 `[u8;2]` 定长与 1–4 值域换成随 `seats.count`/`teamSize` 的 `rank: u8`，`combos`/`teamRanks`/`tribute.pairs` 三处共用。这一条不动，六人及任何非 4 人变体连外壳都写不全。
2. **阻断 M1 · 升级判定域的第二种 basis + 分值表（G7 + G8）**：`upgrade` 从「名次组合→step」扩到「分差带→step（含 step 0）」，并把 v2 预留的 `score.table` 提前到能承载名次→分值映射。这一条决定「计分驱动的升级类棋牌」能不能进 DSL。
3. **可推后 · 首出与终止的可配置化（G1 + G3）**：先补 `flow.turn.firstLead` 与 `settle.termination` 两个键，G2/G4/G13/G14 四处的冻结口径可以随后一批补。这四条都不阻断 M0，也不阻断基线掼蛋上线，只在族内变体零代码配置化时变成硬约束。

---

## 9. 本次没有改动任何设计文档与任何代码

- 本次唯一写入的仓库文件就是本文档 `docs/assets/m0-paper-walkthrough.md`。
- `docs/designs/**`、`crates/**`、`.gitattributes` **一个字节都没改**；未执行 `git add` / `commit` / `push`（也没有跑任何其它 git 命令）；未在仓库内 `cargo build`（L1 试跑用的是 path 依赖链接 `crates/engine` 的独立探针，产物落在仓库外）。
- 可复核证据：`design_draft.md` 的 mtime 为 **2026-11-24 08:30:10**、SHA256 为 `d6a36f3e52f3170051af75ac7a0c987d94c4278434ccefef2b46cca396dc16aa`——早于本次推演全部读取与试跑，且本文所有 `L<行号>` 引用都锚在该版本上，逐条可比。推演期间 `crates/` 与 `docs/designs/` 下存在时间戳变化，来自同一工作树上并行进行的另一批改动，与本次推演无关（本次未向这些路径写入任何文件）。

### 复核方式（只读）

把任意草稿态 RuleSet YAML 交给同一入口即可复现本文所有 `CONSTRUCT`/`FINDING` 输出：`from_yaml(yaml_src)` 构造 `RuleSetBody` → `validate(body, [L1])`，输出构造结果、`ranLevels` 与逐条 finding（码 / 严重级 / 字段路径 / 消息）。本文第 2.4、3.4 节贴的是完整输出原文，未做改写。
