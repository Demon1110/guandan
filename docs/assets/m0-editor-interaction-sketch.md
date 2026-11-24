# M0 交图草图：规则编辑器与运营后台

> 对应 `design_draft.md` §2.4 gate 表第 5 项的后半——「非法结构不可构造」**含"出交互草图"**。
> 前半（反序列化层拒绝非法 RuleSet + L1 用例）已由实现端落地（`crates/engine/tests/fixtures/invalid/` 三份反例）；
> 本文件补后半。形态按用户 2026-11-24 裁定：**文本 + ASCII 线框**，不是 UI 设计稿、不含视觉规范。
> 依据：`docs/designs/2026-11-23-guandan-multiplatform/design_draft.md`（1590 行）。文中 `§5.x` / `L<行号>` 均指该版本。
> 只把文档已经写下的东西画出来；文档没写的，本文件不补（见 §10 与 §11）。

---

## 1. 三条前提

| # | 前提 | 依据 |
| --- | --- | --- |
| P1 | 编辑器读写的对象就是 `RuleSetBody` 本身，**没有第二份模型**。积木视图与源码视图是同一份 body 的两个投影 | §5.3「`RuleSetBody` 用固定变体 ADT」；A17 |
| P2 | 运营者不需要写 YAML 就能配完一款玩法；源码视图是研发的高级入口，不是主入口 | A17、D7 |
| P3 | 非法结构**在界面上就构造不出来**，不等 L1 报错。L1 是第二条防线，不是第一条 | §2.4 gate 5、§5.3、§5.4 |

---

## 2. 主界面（积木视图为默认）

```
+--------------------------------------------------------------------------------------+
| Rule Editor   guandan-classic  ·  draft d-0412  ·  rev 7   [Validate]  [Publish]     |
+--------------------------------------------------------------------------------------+
| Fields (S5.1)          | seats                                | Validation           |
|                        |                                      | L1  FAIL 1           |
| v Seats & Teams     *  | Seat count [ 4 v]   range 2-8        | +------------+       |
|   seats.count     4   |                                      | | L1 Blocker |        |
|   seats.teams     ok  | Teams [0,2] [1,3]   coverage 4/4      | | RS_SCHEMA_ |       |
| > Deck & Zones      *  |                                      | | INVALID    |       |
| > Levels & Upgrade !1 |                                      | | location:  |        |
| > Patterns/Compare ok |                                      | | level.pass.|        |
| > Tribute          ok |                                      | | combos[0]  |        |
| > Flow             ok |                                      | +------------+        |
| > View & Settle    ok |                                      | [jump to 1st]         |
| + Add field (closed  |                                      |                        |
|   set only)           |                                      | View preview  >       |
+--------------------------------------------------------------------------------------+
| deck 108 = 13x4x2 + 2x2 cards · hand cap 27x4 · UNSAVED CHANGES                      |
+--------------------------------------------------------------------------------------+
```

说明：
- 左栏是 §5.1 的字段分组，**不是文件树**；字段名与 §5.1 表逐字一致，便于对着文档评审。
- 组标题后的 `✎ / ✗1 / ✓` 是该组最高严重级的摘要，点 `✗1` 直达错误行。
- 右栏「校验结果」常驻，L1 实时跑（§5.4 预算 <10ms），L2 也实时（<50ms）；L3/L4 只在发布时跑，此处显示为「发布时执行」。
- 状态栏的基线数字来自 §5.1 `deck` 行与 §5.10，不是编辑器自己算的常量。

---

## 3. 字段组 → 控件形态

控件形态只有五种：**下拉**（闭集）、**数值**（带上下界）、**开关**、**列表**（增删/排序）、**引用选择**（候选来自本 RuleSet 已声明的 id）。没有自由文本输入框，除 `rulesetId` 与 `interpreter` 版本区间两处。

| 字段组 | 控件 | 值域 / 候选从哪来 | 非法态在界面上的表现 |
| --- | --- | --- | --- |
| `rulesetId` | 文本（唯一自由输入） | kebab-case；发布后只读 | 不合法格式即时标红；已发布则整框只读 |
| `interpreter` | 文本（semver 区间） | `>=x.y,<z` | 解析失败即时标红 |
| `seats.count` | 数值 | 2–8 | 越界禁止输入（不是提交后才报） |
| `seats.teams` | 列表（每座位归队） | 座位号 0..count-1 | 覆盖不全/重叠 → 行内 `RS_TEAM_COVERAGE_INVALID` 预览 |
| `deck.copies` | 数值 | ≥1 | — |
| `deck.ranks` | 多选 | `naturalOrder` 子集，字面 `2`..`A` | 闭集外无法勾选 |
| `deck.suits` | 多选 | `S/H/D/C` | 同上 |
| `deck.jokers` | 只读 2 | 固定 | 不可编辑（写别的值在 L1 就被拒） |
| `deck.exclude` | 多选 | 点数闭集，不含王 | 王不出现在候选里 |
| `zones` | 列表 | `{id, scope, capacity}`，`scope` 闭集 `private/shared`，`capacity` ≥0 或 −1 | 容量冲突即时提示；总张数 > 各 `hand` 容量和在 L2 报 `RS_DECK_COUNT_MISMATCH` |
| `levelTable.initial` | 下拉 | `levelTable.ranks` 之内 | — |
| `levelTable.ranks` | 列表（有序） | 牌面键 | 顺序即档位序，拖拽改序要提示会影响 `levels` 下标 |
| `levelTable.upgradeBasis` | **单选** | `rank-combo` / `score-diff` | 切换即**物理替换下半区控件**，见 §4 |
| `levelTable.upgrade[].teamRanks` | 名次组合选择器 | 长度 = 该队 `teamSize`；元素 `1..=seats.count`；升序 | 长度不符/越界/重复 → 该行红；`[2,1]` 录入即被规范化成 `[1,2]` |
| `levelTable.upgrade[].step` | 数值 | **0–3**；`0` 标注「该组合不升档」 | 越界禁止 |
| `levelTable.rankScores` | 分值表 | 长度 = `seats.count`；u16 | 行数不符即时红（随 `seats.count` 联动） |
| `levelTable.upgradeByDiff` | 区间表 | `minDiff` 降序、末行必须 `0`、`step` 0–3 | 未降序或缺 `0` 兜底 → 表格红；缺兜底时「追加兜底行」按钮 |
| `level.pass.combos` | 多选（组合） | 同上（长度 = `teamSize`、升序） | `teamSize≠2` 且留空 → 红：「该规模无默认值，必须声明」 |
| `level.pass.streakLimit` | 数值 | 1–3 | 越界禁止 |
| `level.pass.resetOnFail` | 开关 | bool | — |
| `naturalOrder` / `levelInsert` | 有序列表 / 下拉 | 后者 `above-A` / `below-2` | — |
| `wildcard` | 结构化：花色下拉 + 点数（具体键或 `@level`）+ 替代范围多选 + 开关 | `source` 不接受自由字符串 | — |
| `patterns` | 列表；`primitiveId` 下拉 → 参数表单按型替换 | 参数取值见 §5.2 参数表 | 未知键没有输入位；缺必填键时该框标红 |
| `compare.ladder` | 有序列表（拖拽） | 候选 = `patterns[].id`，**排除 `straight-flush-*`** | 插入型牌型在候选里灰掉并给原因 |
| `compare.flushRank` | 下拉 | `below-bomb-5` / `above-bomb-5` / `above-bomb-6` / `above-bomb-8` | 依次切值后右侧阶梯预览实时重排 |
| `compare.bombOrder` | 单选 | `count-then-rank` / `rank-then-count` | 两个取值下方各有一句差异说明（见 §7） |
| `tribute.enabled` | 开关 | bool | 关掉后其余子键整段收起（可省） |
| `tribute.pairs[]` | 一对一映射表 | `payer`/`receiver` 数值 `1..=seats.count`；`when` 闭集 | 第 5 名之类不存在的名次无法选；同一 `when` 下重复对位给警告 |
| `tribute.return` | 三键表单 | `ceiling` / `count` / `fallback` 皆闭集 | `enabled=true` 时缺任一键 → 红 |
| `lifecycle` | 只读四段 | `[deal, tribute, play, settle]` | 不可改（文档固定四段） |
| `actions` | 列表 | `kind` 闭集 `play`/`pass`；`primitive` 引用；`legalFrom` 三选 | `claim`/`declare` 不在候选里（v2 预留） |
| `settle.algorithm` | 单选 | `level-up` / `score-only` | 选 `level-up` 时 `levelTableRef` 变必填并自动点亮 |
| `matchCap.rounds` | 数值 | 1–99，默认 12 | 越界禁止 |
| `view.fields` | 多选白名单 | 只允许 `seat.levels`、某个 zone id、`lastPlay` | 候选就这三个来源，写不出别的 |
| `view.schemaVersion` | 数值 | u16 | 改动白名单集合或顺序时弹「必须 +1」确认，不允许静默保存 |
| `timing` | 三数值 | 5–120 / 30–180 / 60–600，默认 30/90/300 | 越界禁止 |
| `ai.hint` | 开关 | bool | — |
| `flow` | 五个折叠子表单 | 逐键见 §5.1 `flow` 表 | 默认值以灰字提示「不填则取默认」 |

---

## 4. 「非法结构不可构造」的三条 UI 规则

**R1 必填不可省。** 文档里写作 `○（条件式）` 的字段，只在条件满足时点亮；条件不满足时该控件**不存在**（不是灰掉），提交体里也没有这个键。
例：`settle.algorithm = score-only` 时，`levelTableRef` 整块从表单里消失；改成 `level-up` 才出现，且出现即必填。

**R2 闭集不可自由输入。** 所有枚举一律下拉/单选，没有"手输后校验"的通道。这条同时挡掉三类最常见的非法 RuleSet：拼错的枚举字面值、大小写变体、文档外的取值。

**R3 互斥不可并置。** `levelTable.upgradeBasis` 的两个 basis 键集互斥（§5.1 `levelTable` 行、§5.4 L1）。切换时**非活动键集从表单与提交体里一并移除**，不做"灰化保留"——灰化保留等于允许脏数据随 body 一起提交。

```
+--------------------------------------------------------------------------------------+
|  Upgrade basis:  [x] rank-combo     [ ] score-diff                                   |
+--------------------------------------------------------------------------------------+
|  rank-combo mode:                                                                    |
|    teamRanks   step 0-3     (length must equal team size, e.g. 2)                    |
|    [1,2]  ->  +3                                                                     |
|    [1,3]  ->  +2                                                                     |
|    [1,4]  ->  +1                                                                     |
|    [ + add combo ]     0 = this combo does NOT upgrade                               |
+--------------------------------------------------------------------------------------+
|  score-diff mode (replaces the table above, keys are mutually exclusive):            |
|    rankScores  (length must equal seat count)  u16                                   |
|    rank#1 [3]   rank#2 [2]   rank#3 [1]   rank#4 [0]                                 |
|    upgradeByDiff  (minDiff DESC, last row MUST be 0)                                 |
|    diff >= [ 6 ]  ->  +3                                                             |
|    diff >= [ 3 ]  ->  +2                                                             |
|    diff >= [ 0 ]  ->  +1    <- fallback row, cannot delete                           |
+--------------------------------------------------------------------------------------+
```

**R4 规模联动就地标红。** 改 `seats.count` 或队伍划分会改变 `teamSize`，而 `teamRanks`、`level.pass.combos`、`rankScores`、`tribute.pairs` 的长度/值域都挂在它上面。改动发生时，受影响的行**立刻**标红并给出"应为长度 N"，不等保存。

由此，下列非法 RuleSet 在积木视图里**根本无法提交**（这就是 gate 5「非法结构不可构造」在 UI 侧的落点）：

| # | 构造不出来的非法态 | 依据 |
| --- | --- | --- |
| 1 | `teamRanks` 长度 ≠ 该队队员数，或元素超出 `1..=seats.count` | §5.1 `levelTable` 行、§5.4 L1 |
| 2 | `teamRanks` 未升序或与另一条重复 | 同上 |
| 3 | `step` 不在 0–3 | §5.1、§5.4 L1 |
| 4 | `rank-combo` 与 `score-diff` 的键集同时出现 | §5.1、§5.4 L1 |
| 5 | `rankScores` 长度 ≠ `seats.count`；`upgradeByDiff` 未降序或缺 `minDiff: 0` | §5.4 L1 |
| 6 | `ladder` 里出现 `straight-flush-*` | §5.1 `compare` 行、§5.4 L1 |
| 7 | `level.pass.combos` 元素长度 ≠ `teamSize`、越界、未升序，或非 2 人队留空 | §5.1 `level` 行 |
| 8 | `view.fields` 写入白名单外的键（如 `seat.hand`） | §5.1 `view` 行、§5.12 |
| 9 | `tribute.pairs` 引用不存在的名次（如四人局写 `5`） | §5.1 `tribute` 行 |
| 10 | `patterns[].params` 出现未知键或缺失必填键 | §5.2 参数表、§5.3 |

---

## 5. 源码视图：同一份 body，两种投影

```
+--------------------------------------------------------------------------------------+
| Builder (read-only mirror)    |  Source (YAML)        [ SYNCED  rev 7 ]              |
+--------------------------------------------------------------------------------------+
| seats.count        4          |  seats: {count: 4, teams: [[0,2],[1,3]]}             |
| deck.copies        2          |  deck:  {copies: 2, ranks: ["2".."A"],               |
| zones.hand.cap    27          |          suits: [S,H,D,C], jokers: 2}                |
| levelTable.init    2          |  levelTable:                                         |
| levelTable.basis combo        |    id: guandan-level                                 |
| upgrade[0] [1,2]  +3          |    initial: "2"                                      |
| ...                           |    upgradeBasis: rank-combo                          |
|                               |    upgrade: [{teamRanks:[1,2], step:3}, ...]         |
+--------------------------------------------------------------------------------------+
| [to source]                   |  [to builder]    [save = parse first, else no]       |
| parse: PASS 0 error           |  L1: FAIL 1      first error: level.pass.combos[0]   |
+--------------------------------------------------------------------------------------+
```

规则：
1. **真源只有一处**：服务端 `ruleset_draft.body`（§5.6 `PUT /admin/rulesets/{id}/drafts/{draftId}`）。两个视图都从它投影，任何一边保存都是整份 body 的替换，**不允许两套序列化各写各的字段**。
2. **切换时的校验点**：积木 → 源码是纯渲染，不校验；源码 → 积木必须先跑一次解析（ADT 反序列化）**成功**才重建控件树，失败则留在源码视图、标红行列并禁止切换。理由：控件树表达不了未知键，硬切会把脏数据吞掉。
3. **源码视图允许写非法结构**（这是研发的入口），但保存前必须过解析 + L1；失败时不落库、错误定位到字段路径（`§5.4` 的 `Finding.location`），并把对应字段在切回积木时高亮。
4. 注释里允许写 `#` 说明，但保存时注释**不保留**——body 是数据不是文本，编辑器不承诺往返保真。

---

## 6. 校验结果面板

```
+--------------------------------------------------------------------------------------+
| Validation                                                                           |
+--------------------------------------------------------------------------------------+
| Level  code                        location             message                      |
| L1 !   RS_SCHEMA_INVALID           level.pass.combos[0] len must be 2 (team=2)       |
| L1 !   RS_SCHEMA_INVALID           levelTable.upgrade[1] step must be 0-3, got 4     |
| L2 !   RS_DECK_COUNT_MISMATCH      deck                 total 112 != cap sum 108     |
| L2 ?   RS_REF_MISSING              compare.ladder[3]    pattern id not declared      |
| L1 ?   RS_FLUSH_ANCHOR_MISSING     compare.flushRank    no bomb anchor count=5       |
+--------------------------------------------------------------------------------------+
| Blocker 1  -> save draft ALLOWED, publish BLOCKED                                    |
| [expand all] [copy]                                                                  |
+--------------------------------------------------------------------------------------+
```

要点：
- 三列固定为 `code / location / message`，与 §5.4 的 `Finding = {level, code, severity, location, message}` 同构，方便把后台报错原文对照实现。
- **Blocker 阻断发布、不阻断保存草稿**：草稿态本来就允许不完整（§5.6 发布状态机 `draft` 只要求 L1/L2 通过）。保存可用、发布按钮置灰并写明原因。
- L1 的两条专用码 `RS_FLUSH_ANCHOR_MISSING`（锚点缺失）/ `RS_LADDER_CONFLICT`（位次冲突）必须**分开显示、不合并**，因为处置不同：前者让你补炸弹，后者让你改顺序（§5.1 `compare` 行）。
- L3（200 局 dry-run，预算 ≤90 秒）与 L4（黄金用例重放）只在发布时跑（§5.4），发布面板显示进度而不是实时标红。

---

## 7. 牌型与比较：一屏两区

```
+--------------------------------------------------------------------------------------+
| Patterns                                      | Compare                              |
| #  id               primitive            param | Ladder (drag to reorder)            |
| 1  single           pattern.same-rank    1     | 1 single                            |
| 2  pair             pattern.same-rank    2     | 2 pair                              |
| 3  triple           pattern.same-rank    3     | 3 triple                            |
| 4  straight-5       pattern.straight     5     | 4 straight-5                        |
| 5  plate-3          pattern.consec-pairs 3     | 5 plate-3                           |
| 6  steel-2          pattern.consec-tripl 2     | 6 steel-2                           |
| 7  bomb-4           pattern.bomb         4     | 7 bomb-4                            |
| ..                                             | ..                                  |
| 12 straight-flush-5 pattern.straight-fl 5     | ! straight-flush-* NOT in ladder     |
| 13 joker-bomb       pattern.joker-bomb  none  |   (rank comes from flushRank)        |
| [ + add pattern ]                             | flushRank  [ above-bomb-5 v ]        |
|                                               | bombOrder  (x) count-first           |
|                                               |            ( ) rank-first            |
+--------------------------------------------------------------------------------------+
| flushRank preview (composed at load time):                                           |
| above-bomb-5 -> bomb-4 < bomb-5 < SF-5 < bomb-6 < bomb-7 < bomb-8 < joker-bomb       |
+--------------------------------------------------------------------------------------+
```

要点：
- `patterns` 的参数字段**随 `primitiveId` 换形**：`pattern.joker-bomb` 选上后参数区整体消失（无参、只能空对象）；`pattern.bomb` 的 `count` 值域 4–8（§5.2）。
- `compare.ladder` 的候选**不含插入型牌型**；`straight-flush-*` 在列表里灰显并写原因，否则作者会以为"漏选了"。
- `flushRank` 四值任一改动，右侧阶梯预览立即重排，并提示"改动会改变 `stateHash`，旧黄金用例需重算"（§5.1 `compare` 行末、§10.3）。
- `bombOrder` 两值各配一句差异说明，直接引文档原话的语义：`count-then-rank` = 先比张数、同张数再比点数；`rank-then-count` = 先比点数、同点数再比张数；两者点数均按载入期合成牌力序（级牌高于 A）。
- 锚点缺失（`ladder` 里没有对应张数的 `pattern.bomb`）时，`flushRank` 下拉旁出现橙色提示并指向 `RS_FLUSH_ANCHOR_MISSING`，**不做"自动插到最近位置"**——文档明确不降级。

---

## 8. 升级编辑器：两套控件联动

见 §4 的两张线框。补充联动矩阵：

| 改动 | 连带影响（界面必须立即反馈） |
| --- | --- |
| `seats.count` 4 → 6 | `rankScores` 行数 4 → 6；`teamRanks` 组合长度 2 → 3；`level.pass.combos` 元素长度 2 → 3；`tribute.pairs` 可选名次扩到 6 |
| 队伍划分变化（如从 `[[0,2],[1,3]]` 改为 `[[0,1,2],[3,4,5]]`） | `teamSize` 2 → 3，上面四项同时变；已有 `teamRanks` 行按新长度标红，不静默截断 |
| `upgradeBasis` 切换 | 非活动键集整块移除（R3）；切回时**不恢复**上次内容，避免误以为仍生效 |
| `level.pass.streakLimit` 1–3 | 提示与 `aFailStreak` 冻结值域 0–3 同源，改这里不影响 `aFailStreak` 本身 |

---

## 9. 发布面板

```
+--------------------------------------------------------------------------------------+
| Publish   guandan-classic / d-0412                                                   |
+--------------------------------------------------------------------------------------+
|   (*)------------(*)------------(o)------------(o)                                   |
|  draft        validated       canary         full                                    |
|  rev 7        L1/L2 passed    10% new rooms  all rooms                               |
|               L3/L4 pending   watch 30 min                                           |
+--------------------------------------------------------------------------------------+
| Pre-flight:  [x] L1   [x] L2   [ ] L3 (200 rounds <=90s)   [ ] L4 golden             |
| TOTP [______]   Reason (required) [__________________________]                       |
| [Publish to canary]   [Save draft only]   [Rollback (badReleaseId)]                  |
+--------------------------------------------------------------------------------------+
| Note: if canary window has <200 actions, judge by ZERO P0 events (not ratio).        |
+--------------------------------------------------------------------------------------+
```

要点：
- 四态取自 §5.6 发布状态机与 §9.4；`validated` 的必要条件是 L1/L2 通过且无 Blocker，`canary` 前必须 L3/L4 通过。
- **L3/L4 未跑时 `canary` 按钮不可点**，不是"点了再报错"。
- 发布必填 TOTP + `reason`（§8.1 矩阵、§10 的 D10）；回滚免 TOTP 但必填 `badReleaseId` 与理由。
- canary 的低样本出口（观察窗口内动作数 <200 时改按"零 P0 事件"判）在面板上以提示条出现，避免运营误读"10% 房间"这个比例。
- 派生的「发布后新房间异常」不在本面板：属告警与回滚流程（§7.5/§9.5）。

---

## 10. 本稿刻意不覆盖

| 不做 | 理由 |
| --- | --- |
| 牌桌的交互稿与动效参数 | §2.3 明确列为「不在本设计范围内」，另行成文 |
| 视觉规范（配色、字号、图标） | 素材与视觉属实现阶段（A3、§2.3） |
| 移动端布局与小屏适配 | 移动壳押后（U5），现在画等于画一个不做的界面 |
| 大厅、匹配、排行界面 | 属 M2/M3 交付物（§2.1），与本 gate 无关 |
| 回放播放器界面 | 属 M3（U8 只做回放式观战） |
| 积木视图的具体拖拽手势 | 属交互细节，文字草图无法定格，留给实现阶段 |

---

## 11. 与 gate 5 的对应（自证）

- gate 5 原文（`design_draft.md` §2.4 第 196 行）：**「非法结构不可构造」——反序列化层验证非法 RuleSet 无法构造并通过 L1；出交互草图**。
- 前半已由实现端证过：`crates/engine/tests/fixtures/invalid/{unknown-key,missing-required-key,type-mismatch}` 三份反例在构造期即被 `RS_SCHEMA_INVALID` 拒绝，且拒绝信息打印闭集清单。
- 本文件的落点：§4 的三条 UI 规则（R1–R4）+ 10 条"构造不出来的非法态"表，把同一批约束前移到编辑期。**UI 层拦不住的，才是 L1 该拦的**；L1 仍是权威，草图不是替代品。
- 一句话结论：**编辑期与载入期用同一张约束表，只是错误呈现从"日志里的 code"换成"控件旁的提示"**。

---

## 12. 未决与遗留（照实记，不替文档补）

1. **积木视图的字段顺序与分组名**（本稿按 §5.1 表序分组，如「座位与队伍 / 牌堆与区 / 级牌与升级」）属交互取舍，文档未规定，实现阶段可调。
2. **源码 ↔ 积木切换时的"未保存改动"**如何处理（强制保存前解析、还是允许暂存一份文本 draft），文档未规定；本稿取"必须解析成功才能切回"的严格口径，若要放宽需另裁。
3. 错误 `location` 的字符串格式（示例用 `level.pass.combos[0]` 这种路径写法）文档只要求"能定位"，未规定语法；本稿按字段路径写，实现阶段可统一。
</content>
