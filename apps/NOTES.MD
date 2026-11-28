# 掼蛋客户端对接规范（Web / Phaser 通用）

本文档面向后续端开发，综合 `apps/web`（DOM 表现层）与 `apps/phaser`（Phaser 3 表现层）两套客户端的对接实践，沉淀**与 wasm 引擎对接**的契约、要求与踩坑规则。两端共用同一份 `crates/client-wasm` 产物，表现层可替换，但对接契约与状态语义必须一致。

全文采用连续编号，规则条目为每个 `§` 小节。

---

## 1. 总体架构与职责边界

1.1 **三层结构**：`crates/engine`（规则引擎）→ `crates/client-wasm`（浏览器绑定层，编成 `cdylib`）→ 客户端表现层（web / phaser）。规则集在编译期由 `include_str!` 内嵌进 wasm，前端无需额外加载规则夹具。

1.2 **为什么 wasm 直跑**：`gf_client_wasm.wasm` 零 import，浏览器 `WebAssembly.instantiate` 裸加载即可，不依赖 wasm-bindgen / npm / 打包工具链。种子、房间、bot、判定、计时、结算全部在 wasm 内完成。

1.3 **职责边界**：
- wasm 负责：房间状态机、牌型识别/压牌判定、bot 决策、计时托管、结算升一级、权威哈希。
- 客户端负责：交互与选牌、动画与主题渲染、把 wasm 事件翻译成表现、动作日志落盘与回放恢复。
- 客户端**不得**自行判牌力或改判定口径：牌力比较一律用引擎算好的值（级牌按 `levelInsert` 提升、王最大），否则会把级牌当小牌、出的手被房间判非法。

1.4 **单一真相源**：房间状态只在 wasm 里；客户端内存里的手牌、名次、级牌都是 `view()` / `info()` 的投影。`stateHash` 由 wasm 计算，客户端只做"存档 vs 恢复"比对，不自行生成。

1.5 **两种表现层的定位差异**：web 是完整参考实现（大厅 + 牌桌 + 存档 + 主题工坊），phaser 是对同一 wasm 契约的 Phaser 复刻。新端开发应**先对契约（§3–§7），再对表现（§8–§11）**。

---

## 2. 构建、加载与运行前提

2.1 **构建与复刻产物**：
```powershell
cargo build -p gf-client-wasm --target wasm32-unknown-unknown --release
# 产物复制到两端目录（两端各存一份 wasm，修改 Rust 后必须同步）：
cp target/wasm32-unknown-unknown/release/gf_client_wasm.wasm apps/web/gf_client_wasm.wasm
cp target/wasm32-unknown-unknown/release/gf_client_wasm.wasm apps/phaser/gf_client_wasm.wasm
```
2.2 **必须走 HTTP**：`file://` 打不开（wasm 必须 `fetch`）。用各端 `serve.ps1` 起静态服务。

2.3 **裸加载模板**（两端一致）：
```js
const bytes = await (await fetch('gf_client_wasm.wasm')).arrayBuffer();
const module = await WebAssembly.compile(bytes);
const ex = (await WebAssembly.instantiate(module, {})).exports;
```
2.4 **导出兼容性**：新导出（`gf_room_new_level` / `legal` / `can_play` / `plan` / `info` / `display_order` / `theme_*`）在旧产物上可能缺失。客户端应做能力探测与降级（web 全部有降级，phaser 主题无降级——见 §9.1）。

---

## 3. FFI 内存契约（最高优先级）

3.1 **入参**：全部走**标量 + 定长数组**，不引入 JSON 入参（仓库离线依赖里没有 serde_json）。种子拆成两个 u32 `(seed_hi, seed_lo)`；动作 = `(seat, now_ms, is_pass, cards_ptr, cards_len)`；牌 id 用 u16 数组。

3.2 **出参**：一律是**以 `\0` 结尾的 JSON 字符串**。指针指向 `[u64 长度头][bytes...][\0]` 的 bytes 起点。

3.3 **生命周期（必守）**：每次拿到出参指针后**必须**调用 `gf_str_free(ptr)`；异常路径也要释放。两端都把解码与释放放在 `try/finally` 里：
```js
function cstr(ptr) {
  if (!ptr) throw new Error('wasm 返回空指针');
  try {
    const view = new Uint8Array(ex.memory.buffer, ptr);
    let end = 0;
    while (end < view.length && view[end] !== 0) end++;
    return new TextDecoder().decode(view.subarray(0, end));
  } finally { ex.gf_str_free(ptr); }
}
```
3.4 **写入数组**：用 `gf_alloc(len)` 分配 → 写入 `Uint16Array`/`Uint8Array` → 回调 → `gf_mem_free(ptr, len)`。牌数组按 u16 对齐，长度 = `cards.length * 2`。
3.5 **上限**：单次入参 ≤ 16MB（主题包），牌数组 ≤ 256 张。wasm 侧校验指针必须由 `gf_alloc` 登记、对齐且长度不越界，非法输入按空动作处理，绝不跨 FFI 读越界。
3.6 **panic 隔离**：主题相关导出用 `catch_unwind` 包裹，跨 FFI 的 panic 只能返回错误串，不能 abort 整个实例。
3.7 **单实例串行**：wasm32-unknown-unknown 单线程，所有导出在同一条 JS 调用栈上串行；全局 `ROOM` / `DECK` / `ALLOCS` 槽用 `UnsafeCell`（native 目标用 `Mutex`）。**不要**并发调用导出。

---

## 4. 牌数据契约

4.1 **牌 id 编码**（与引擎一致）：普通牌 `id = (suit × 13 + rank) × 2 + copy`；`suit` 0..3 = ♠♥♦♣，`rank` 0..12 = 2..A，`copy` 0/1（两副牌）。王从 104 起：104/105 小王、106/107 大王。

4.2 **牌力排序键**（客户端手牌列序，`columnKey`）：大王=1000、小王=999、级牌=998、其余按 `100 + 位置`（A→K→…→除级牌外的小牌），同点数按花色再按 id。手牌按**牌力降序**排（键大靠左），与 id 数值升序（=按花色排）不是一回事——**绝不能直接用 id 数值当展示序**。

4.3 **点数标签**：`RANKS = ['2'..'10','J','Q','K','A']`，`rankIdx 0..12`。

4.4 **花色显示序**：引擎/手牌用 ♠♥♦♣（suitIdx 0,1,2,3）；但**展示排序**里同点数花色序是 ♠♥♣♦（映射 0→0, 1→1, 3→2, 2→3，见 §10.2）。两者不同，是刻意的视觉规范，不要混用。

4.5 **deckart Face code**：`gf_theme_card` 的入参，普通牌 `S/H/D/C + 点数`（如 `S10`、`HA`），王为 `JOKER-S` / `JOKER-B`。

4.6 **三个"等级"参数语义不同（高频踩坑）**，务必区分：

| 名称 | 取值 | 用途 |
|---|---|---|
| `bot_level` | 0/1/2 | `gf_room_new_level` 第 4 参，选 AI-L0/L1/L2；**web 里变量名叫 `level` 或 `state.level`，但它其实是 bot 档位，不是级牌** |
| 级牌下标 `level_rank`（展示排序） | 0..12（2..A），-1=无 | `gf_display_order` 入参，标记红配（红桃级牌）用于补位 |
| `Rank` 编号（主题渲染） | 1=A、2=2…13=K，0=无级牌 | `gf_theme_card` 的 `level_rank` 入参 |

4.7 **级牌读取**：客户端通过 `view(seat)['round.level']`（0..12 下标）拿到当前级牌标签；`info().levels` 给出双方级牌（升级后变化）。**级牌由引擎在结算时升级**，客户端不自行推算升级。

---

## 5. wasm 导出全量清单

5.1 **内存管理**：`gf_alloc(len)` / `gf_mem_free(ptr,len)` / `gf_str_free(ptr)`。

5.2 **房间生命周期**：

| 导出 | 签名/返回 | 说明 |
|---|---|---|
| `gf_room_new` | (hi,lo,mask)→u8 | 建桌，bot 固定 L2（兼容旧页） |
| `gf_room_new_level` | (hi,lo,mask,bot_level)→u8 | 建桌并选 bot 档位；第 4 参是 bot 档位（§4.6） |
| `gf_room_start` | (now_ms)→u8 | 发牌开局 |
| `gf_room_step` | (now_ms)→JSON | 推进一行：bot 行动/托管/结算/场次终止都在这里出 |
| `gf_room_submit` | (seat,now_ms,is_pass,ptr,len)→JSON | 人类提交；`is_pass`：0 出牌、1 过牌、2 还贡（cards 传一张） |
| `gf_room_replay_action` | (seat,is_pass,ptr,len)→u8 | 重演单条录制动作，不校验玩家类型、不推进时钟 |
| `gf_room_next_round` | (now_ms)→JSON | 结算态再来一局，返回 `{"kind":"dealt",...}` |
| `gf_room_resume_window` | (now_ms)→u8 | 重开当前行动窗口（回放后必调，见 §8.5） |

5.3 **视图与自证**：

| 导出 | 返回 | 说明 |
|---|---|---|
| `gf_room_view(seat)` | JSON | 座位视图（只含本人手牌等，不泄露他人牌面） |
| `gf_room_hash()` | hex 字符串 | 权威 `stateHash`（32 字节 hex） |
| `gf_room_replay_hash()` | hex 字符串 | 从首局按全量日志重放的哈希，应与 hash 相等 |
| `gf_room_meta()` | `{"roundNo","roomSeq","records"}` | 局号/广播水位/已落动作数 |
| `gf_room_last_record()` | JSON 或 null | 全量日志最后一条；结算那一手靠它补记 |
| `gf_room_seats()` | `{"seats":[{seat,human}]}` | 座位身份 |
| `gf_room_info()` | JSON | 桌面公开信息（§6.3） |

5.4 **决策辅助（纯只读）**：

| 导出 | 返回 | 说明 |
|---|---|---|
| `gf_room_can_play(ptr,len)` | `{"ok","patternId","reason"}` | 预检一手牌，与 submit 同口径但不推进状态；"出牌"按钮判定源 |
| `gf_room_legal()` | `[{"pass","patternId","cards"}]` | 当前行动座位合法动作集；"提示"/理牌按它选牌。tribute 相位返回 `{"pass":false,"tributeReturn":true,"cards":[card]}` |
| `gf_room_plan()` | `{"ok","seat","score","singleCount","groups":[...]}` | `recommend` 最优整手拆分；bot 与"组牌建议"共用，不进 stateHash |
| `gf_l2_probe` / `gf_l0_probe` | JSON | bot 决策自证（仅当前行动座位） |
| `gf_display_order(ptr,len,pat_ptr,pat_len,level_rank)` | `{"cards":[...]}` | 出牌展示排序（§10），纯函数不碰房间 |

5.5 **主题**：`gf_theme_load` / `gf_theme_load_verified(pack,pub)` / `gf_theme_card(code,level_rank,pct)` / `gf_theme_preview` / `gf_theme_back` / `gf_theme_unload`。生产用 `*_verified` 强制验签（见 §9.1）。

---

## 6. 事件协议（step / submit 的返回）

6.1 **返回类型 `kind`**：`waiting` / `applied` / `settled` / `matchEnd` / `error`。所有分支都带 `phase`、`roundNo`；`applied` 额外带 `roomSeq`。

6.2 各分支载荷：

| kind | 关键字段 | 客户端动作 |
|---|---|---|
| `waiting` | `seat`、`deadlineMs` | 挂起主循环等人类；bot 回合稍后继续；记录 deadline 供倒计时 |
| `applied` | `seat`、`action`、`auto`、`roomSeq` | 落一条动作日志 + 派发出牌动画；bot 出牌加间隔 |
| `settled` | `summary`、`roomSeq` | 补记尾手 → 展示结算层 → 停在主循环外 |
| `matchEnd` | `winnerTeam`、`summary` | 同上，标记场次结束 |
| `error` | `error` | 记录并停止主循环 |

6.3 **`action` 结构**：出牌 `{"pass":false,"patternId":"...","cards":[...]}`；过牌 `{"pass":true}`；还贡 `{"pass":false,"tributeReturn":true,"cards":[card]}`。

6.4 **`patternId` 权威来源**：出牌的 `patternId` 由引擎 `last_play.pattern_id` 带出（`applied` / `can_play` / `legal` / `plan` 都有）。红配补位的炸弹牌结构看不出来，**只有引擎的 `bomb-N` 认得**——所有牌型/炸弹/同花顺判断一律以 `patternId` 为准，不要按牌结构猜（见 §10.4）。

6.5 **`auto`**：`null` 或 `"Timeout"` / `"Offline"` / `"BotIllegal"`，表示托管原因，只用于日志展示。

6.6 **`summary`**：`{"roundNo","ranks":[seat...],"winnerTeam","completedTeam","step","points","matchEnd"}`。`ranks` 顺序即头游→末游。

6.7 **`info()` 结构**（公开信息，不进 stateHash）：
```
{ phase, roundNo, turnSeat, passesSince, levels:[双方级牌],
  attackingTeam, seats:[{seat,human,hand,rank}],
  tribute:{ active, resisted, leader, pending:[{receiver,payer,eligible}], events:[{from,to,card,return}] } }
```
`rank` 为已锁定名次（0=未定）；`hand` 为张数（真实牌桌也可见张数）。**对局中不下发他人牌面**；结算摊牌时才读 `view(seat).hand.cards`，仅展示、不参与判定。

6.8 **`phase` 名称映射**：0=deal、1=tribute、2=play、3=settle。客户端自己的 `phase` 状态机见 §7.1。

---

## 7. 客户端状态机与主循环

7.1 **状态机**：`idle → dealing → (tribute) → play → settle → [下一局 dealing]`，`settle` 在场次结束时进 `ended`。发牌是独立阶段：先建桌进入 `dealing`，播完发牌动画再 `beginPlay()` 进主循环。

7.2 **主循环（pump）流程**：连续调 `step()`；遇 `applied` 落日志+动画后继续；遇 `waiting` 挂起（人类等提交，bot 延迟后继续）；遇 `settled`/`matchEnd` 收尾并停循环。循环带上限（400–500 次）防死循环。

7.3 **时钟契约（高频踩坑）**：`start` / `step` / `submit` / `resume_window` 必须用**同一时间基准**（`performance.now() - clockBase`，u64 毫秒）。若建桌用 `BigInt(0)` 而窗口 deadline 用真实时钟，或反之，deadline 会停在极小值/极大值，导致引擎立即判超时自动托管、玩家永远轮不到操作。web 用 `start(0)` + `resumeWindow(now())` 的配对方式，phaser 用 `start(now())`，**两种都可工作，但同一实例内必须自洽**。

7.4 **人类回合低频轮询**：web 每 400ms `step` 一次只为检查超时托管，bot 回合 ≥120ms；phaser 由 `speed`（默认 320ms）控制 bot 观感。

7.5 **最后一手自动出完**：轮到人类且 `canPlay(整个手牌).ok` 时直接 submit 出完，无需选牌（phaser `tryAutoFinish`）。

7.6 **最后一手结算陷阱**：出完最后一张时 `submit` 直接返回 `settled`/`matchEnd` 而非 `applied`。客户端必须走统一结算处理（`settleFromSubmit`），并用 `lastRecord()` 补记最后一手，否则动作日志缺一条、回放哈希对不上。

7.7 **结算定时器先注册**：最后一手的出牌动画/炸弹特效可能抛渲染异常（如级牌升级后纹理未预热）。结算定时器必须在派发动画事件**之前**注册，且事件监听器逐个加 `try/catch`，保证单个渲染异常不会吞掉结算弹框。

7.8 **级牌升级时的渲染**：引擎在 `submit` 内结算时才升级级牌（如 6→7）。最后一手渲染要用**打出当时的级牌**（升级前），因为纹理只预热了旧级牌；用新级牌取纹理会抛"主题纹理缺失"并中断结算。

---

## 8. 动作日志与回放恢复（IndexedDB）

8.1 **不存 whole state**：只存"动作日志 + 场次元信息"。恢复时用同种子重建房间、按序重演动作，最后哈希自证。

8.2 **两个 objectStore**：
- `matches`（主键 `matchId`）：`seedHi/seedLo/humanMask/humanSeat/level(档位)/botLevel/lastHash/lastRoomSeq/lastRoundNo/updatedAt`。
- `actions`（主键 `[matchId, seq]`）：`seq(roomSeq)/roundNo/seat/pass/tributeReturn/patternId/cards`。

8.3 **matchId**：web = `web-{seed:016x}`，phaser = `phaser-{hi}{lo8}-s{seat}`。种子 + 人类座位是续局指纹；换种子/换座位是另一盘，不命中旧存档。

8.4 **同事务落盘**：每手动作与"当前权威哈希"在**同一事务**写入，两者必然同步。事务提交成功后才更新 UI 计数。`lastHash = room.hash()`，`lastRoomSeq = 本手 roomSeq`，`lastRoundNo = 本手 roundNo`。

8.5 **跨局检查点**：新局一旦没有动作就点"再来一局"（停在 tribute），也要写一个检查点更新 `lastRoomSeq/lastRoundNo/lastHash`，否则刷新会退回上一局结算。`lastHash` 必须是**跨局发牌后的哈希**（不是上一局末的哈希），否则跨局刷新校验失败。

8.6 **恢复流程（严格顺序）**：
1. 从 `matches` 取存档、`actions` 按 seq 排序取日志；
2. `gf_room_new_level` + `gf_room_start` 重建房间；
3. 逐条：若 `rec.roundNo > currentRound` 先 `gf_room_next_round(0)` 补齐局号，再 `gf_room_replay_action`；
4. 跨局检查点：若 `lastRoundNo > currentRound` 继续 `next_round` 补齐（覆盖"新局无动作"情形）；
5. `gf_room_hash()` 与 `lastHash` 比对；**有基线且不一致必须拒绝续局**（不能带错误状态继续打）；
6. `gf_room_resume_window(now())` 重开行动窗口（回放不推进时钟，deadline 仍是旧值）；
7. 已发生的贡牌只补日志、**不重播飞牌动画**；停在当前 tribute/play 相位继续。

8.7 **回放不写盘**：恢复时用 `remember(rec, persist=false)` 重建表现层，不能覆盖 IndexedDB 的哈希基线。

8.8 **哈希自证是硬要求**：老存档无 `lastHash` 可按动作重演放行；一旦有基线就必须相等。这是"杀进程后仅凭日志复原"的唯一正确性保障。

8.9 **恢复前的准备钩子**：恢复跳过发牌动画 → 主题纹理可能未预热 → 同步渲染手牌抛错中断主循环。phaser 在派发恢复事件前 `await onReady(game)` 预热当前级牌的 54 张纹理；新端若有"预热/异步资源"，必须在恢复渲染前完成。

8.10 **重开清理**：同种子重开是全新一场，先按键范围删掉旧 `actions` 再写检查点，避免高 seq 残留污染新局回放。

---

## 9. 主题规范（gfdeck）

9.1 **验签是生产要求**：用 `gf_theme_load_verified`（强制公钥验签，pub_len 必须 32），`gf_theme_load` 的 `FLAG_SIGNED` 可被清除绕过，仅供调试。签名包缺失/伪造一律拒绝。
- web：加载失败**回退 DOM 字卡**，牌桌不阻塞（适合降级）。
- phaser：主题是**硬依赖**，加载/验签失败直接中止启动，不回退自画牌面。

9.2 **包内容**：`.gfdeck` 只装 `theme.json` + 分层素材，不存预览图；由 wasm 现场解密+验签+逐张合成 SVG。换包不需重编 wasm。生成/重打/校验命令见 `crates/deckart/README.md` 第 8 节。

9.3 **角标字号**：`cornerScale`（1.0–3.0）来自 `theme.json`，`gf_theme_card` 的第 4 参 `corner_pct`（0=用主题默认，100=素材原样）。优先级：URL `?corner=` / 用户手动设置 > 主题默认 > 客户端兜底。

9.4 **角标位置**：`corners` 可选 topLeft/topRight/bottomLeft/bottomRight。横排手牌只露左上一条边，主题若去掉 `topLeft` 就读不出点数——换主题时要验证角标位置与手牌布局匹配。

9.5 **不做整图放大裁切**：整张牌 1:1 显示，只在牌面内部放大角标区。早先把牌放大 350% 只露左上角，导致圆角/描边/花阵被裁掉，看着像白底字卡。

9.6 **四花色墨色**：`gf_theme_load` 返回 `suitInks[4]`（♠♥♦♣ 的 #RRGGBB），由 `palette.suits` 优先、回落 `red/black` 算出。UI 给花色按钮等着色直接用它，不要在 JS 侧再解gfdeck 加密包。

9.7 **牌背**：`gf_theme_back()` 返回一张与级牌无关的 SVG 牌背。主题切换时旧纹理/SVG 缓存按 `themeId@cornerPct` 键隔离，不删除 Phaser 纹理对象（删除会让 WebGL 持有失效引用）。

9.8 **纹理预热**：发牌前/恢复前必须按当前级牌预热全部 54 张牌面纹理，否则动画中 `makeCardView` 因纹理缺失抛错。

---

## 10. 出牌展示排序规范（gf_display_order）

10.1 **要解决的问题**：提交/渲染用的是点牌插入顺序（`[...selected]`），引擎 recognize 与 `action_json` 都原样透传/按 id 排，都不满足展示需求，导致 `2-3-4-5-6` 出到桌面变成 `4-5-2-3-6`。故在**渲染前**调用 wasm 纯函数按规范重排。

10.2 **排序规范（定稿）**：

| 牌型 patternId | 排序规则 |
|---|---|
| `joker-bomb` 天王炸 | 两大王在前、小王在后（唯一逆序特例），组内按 id 升序 |
| `triple-pair` 三代二 | 三个的在前、对子在后；组内按点数、花色 |
| `straight-5` 顺子 | 点数从小到大；**轮子顺 A-2-3-4-5 的 A 排最前** |
| `straight-flush-5` 同花顺 | 点数从小到大；禁用轮子特例 |
| `plate-3` 木板（三连对） | 每对点数小的在前，组内按花色 |
| `steel-2` 钢板（三同连） | 每组点数小的在前，组内按花色 |
| `bomb-4..8` 炸弹 | 点数从小到大（红配按补位点数） |
| 单/对/三/未知 | 点数从小到大；王排最后 |

10.3 **通用键**：主序（点数/窗口位置）→ 花色序 ♠♥♣♦ → copy → id。红配（红桃级牌）按展示序补进缺孔；连续牌型用"顶牌最大"窗口选择（对齐引擎 naturalRankKey），轮子顺顶牌记最小档。

10.4 **调用约束**：
- 只在 **`animatePlay` 渲染前**调用，送引擎的 cards、存档 replay、炸弹识别都用原始顺序。
- `patternId` 传引擎权威值；`levelRank` 用**打出当时**的级牌下标（升级前），算红配位置。
- 炸弹识别 `detectBomb` 优先用 `patternId`，无 `patternId`（如 step 的 applied 历史事件）才回退牌结构判定；明确不是炸弹的 pattern 不再按结构猜，避免红配牌误判。

---

## 11. 渲染与层级规范

11.1 **叠列必须"下压上"**：同点数叠一列时，下面的牌要盖住上面的牌，才能露出每张牌的顶部角标看到所有数字。实现：**depth 随 y 递增**（y 越大越靠下、depth 越高）。

11.2 **两端一致的 depth 规则**：
- 手牌 cols 模式：`depth = 20 + round(y / 8)`（选中也不抬 depth，靠金色 tint + 粗金边标识，避免拖选时盖住邻牌角标）。
- 发牌飞牌：`depth = 20 + round(落点y / 8)`，与正式手牌**同规则**，落位后无缝切换（若按发牌顺序叠，动画中上牌盖下牌、发完才恢复）。
- fan 横排模式：统一 `depth=12`，靠创建顺序横叠。

11.3 **结算/观察区展示的 depth**（phaser）：遮罩 200、弹窗 201–204、按钮 203/204；剩余手牌/队友手牌在其上（210+ / 208+），按 §11.1 下压上，标签 depth 高于所有牌。

11.4 **队友手牌只读展示**（phaser 约定）：我出完牌（`rank>0`）后把对家真实剩余手牌用小号 cols 摊在我的出牌区，**纯 image 不挂交互、不进手牌集合**，只读；结算弹窗出现时下移到弹窗下方避让；新局/读档清理。出完牌后隐藏所有操作按钮。

11.5 **标签与牌的间距**：按牌的实际上边缘定位（牌半高 = `CARD_H × SCALE / 2`），留固定 ~24px 间隔，不要让标签底部压进牌面。

11.6 **连续过牌折叠**：连续两家过牌后收起上家牌面，按 `passesSince >= 2` 折叠，纯表现层，不影响判定。

11.7 **结算摊牌**：结束后把其他三家未出完的手牌用小号 cols 亮在其牌区（遮罩之上、避开中央弹窗），已出完的标"已出完"。最后一手已经打出去，出牌区同时清空，不再留在桌面冒充"还没出完"。

---

## 12. 进贡与还贡

12.1 **相位**：每局续发结束后引擎先按上局名次生成进贡关系，进 `tribute` 相位，再进 `play`。

12.2 **规则**：单下=末游→头游，双下=末游→头游、三游→二游，各交一张（红桃级牌除外，自动选最大）。两张大王在贡方手里整段抗贡。还贡方自主选一张点数 ≤10 的牌；没有 ≤10 时退化整手可还，避免卡死。首出由贡牌牌力最大者开始。

12.3 **数据契约**：`info().tribute.pending[].eligible` 给可还牌做校验；`info().tribute.events[]` 驱动飞牌队列；还款走 `submit(..., is_pass=2, cards=[card])`。**联机服 v1 协议还没有还贡动作**，可交互路径是本地 wasm；联机要另加 `TributeReturn` 协议动作。

12.4 **动画与时序**：先进贡动画，受贡方点"还贡"后才走还贡动画；飞牌在播时挂起主循环，避免出牌抢在飞牌前面。回放时不重播历史飞牌。

12.5 **超时托管**：bot 用规则集默认还贡牌；人类等待倒计时，离线宽限/超时由房间自动还。

---

## 13. 两端差异对照表

| 维度 | web | phaser |
|---|---|---|
| 表现层 | DOM + CSS | Phaser 3（Canvas/WebGL） |
| 主循环 | `setTimeout` 轮询，400ms 人类 / ≥120ms bot | `Game.pump` + `speed`(320ms) |
| 建桌时钟 | `start(0)` + `resumeWindow(now())` | `start(now())` |
| 主题失败 | 回退 DOM 字卡 | 中止启动（硬依赖） |
| 主题预览 | 工坊 `theme-studio.html` + 实时角标 | 设置面板 + 热切换 |
| 角标字号键 | `guandan-corner-pct` / `?corner=` | 随主题 `cornerScale` |
| 存档库 | `guandan-m1` | `gf-phaser-replay` |
| 手牌布局 | 列/横排（可切） | cols/fan（可切） |
| 调试入口 | `window.__gd`（`?debug=1`） | `window.__gf`（常驻） |
| 展示排序 | 无独立 wasm API 调用 | `gf_display_order`（渲染前） |

13.1 **共通不变**：牌 id 编码、wasm 导出契约、事件 kind 语义、动作日志+哈希自证恢复流程、进贡还贡规则、主题验签要求——新端必须对齐，差异只在表现层。

---

## 14. 踩坑清单（按出现频率）

| # | 现象 | 根因 | 处理 |
|---|---|---|---|
| 1 | 玩家永远轮不到操作、一起手就托管 | 建桌/step/submit/resume 时钟基准不一致，deadline 停在 0 | §7.3 全用同一 `now()` |
| 2 | 还贡被引擎立即自动处理 | 同上，时钟不一致 | 同上 + 回放后 `resumeWindow` |
| 3 | 最后一手出不去 / 提示已头游却卡死 | submit 返回 settled 被当成失败 | §7.6 统一结算处理 + lastRecord 补记 |
| 4 | 结算弹框不弹 | 最后一手渲染抛错中断了定时器注册 | §7.7 定时器先注册 + 监听器异常隔离 |
| 5 | "主题纹理缺失 card=N level=M" 偶发 | 结算时级牌已升级，渲染用新级牌取纹理 | §7.8 用打出当时的级牌；预热纹理 |
| 6 | 刷新后停在"已恢复"但没人走牌 | 恢复渲染纹理未预热抛错，中断主循环 | §8.9 恢复前 `onReady` 预热 + 异常隔离 |
| 7 | 跨局刷新哈希不一致 | nextRound 检查点用了旧 hash | §8.5 用发牌后 hash |
| 8 | 回放后多一手 / 少一手 | roomSeq 去重错位 / 结算手没补记 | §7.6 + 用 `r.log().last().room_seq`（不是 `r.room_seq()`，后者是下一条） |
| 9 | 出牌牌序乱（2-3-4-5-6→4-5-2-3-6） | 用点牌插入顺序直接铺牌 | §10 渲染前 `gf_display_order` |
| 10 | 红配炸弹/同花顺没特效 | 按牌结构判炸弹认不出红配 | §6.4 / §10.4 以 `patternId` 为准；applied 事件带 `last_play.pattern_id` |
| 11 | 叠列只能看到最上一张数字 | 固定 depth，上牌盖下牌 | §11.1 depth 随 y 递增 |
| 12 | 发牌动画中下牌被盖、发完恢复 | 飞牌按发牌顺序定 depth | §11.2 飞牌按落点 y 定 depth |
| 13 | 队友/结算手牌标签压住牌 | 标签固定偏移算错牌半高 | §11.5 按牌上边缘 + 固定间距 |
| 14 | 自己出炸弹没特效 | submit 消费动作后没派发 play 事件 | §6.2 自派发 play，特效播完再弹结算 |
| 15 | 花色按钮高亮色不对 | 用错花色序或自己解主题包 | §9.6 用 `suitInks`；§4.4 区分两套花色序 |
| 16 | WebGL 切主题报 glTexture null | 切主题时删除了 Phaser 纹理 | §9.7 纹理键带 themeId 隔离，不删对象 |

---

## 15. 新端接入自检清单

15.1 **契约层**：裸加载 wasm 成功；`cstr` 每指针都 free；牌 id 解码正确；能读到 `view`/`info`/`hash`。
15.2 **流程层**：建桌→发牌（独立阶段）→主循环→出牌/提示/过牌→结算→下一局，全通；时钟自洽；最后一手结算与补记正确。
15.3 **恢复层**：落盘→杀进程→重建→按 roundNo 跨局回放→哈希一致→`resumeWindow`→继续，全通；坏档（篡改 hash）被拒。
15.4 **主题层**：验签加载成功；54 张牌面纹理预热；角标位置与手牌布局匹配；切主题无残留。
15.5 **表现层**：所有叠列下压上；展示排序按 §10 规范；事件监听器有异常隔离；结算弹框必弹。

15.6 **可自动化 gate（web 参考）**：`test_restore.mjs`（单进程两局→存档→丢弃 Instance→新建回放，断言哈希一致且能继续）、`verify_wasm_hash.mjs`、`test_flush_windows.mjs`（红配补位/同手去重/轮子顺三边界）。新端应提供等价回归脚本。
