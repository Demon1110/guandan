# 掼蛋 M1 · Web 端（本地房间）

浏览器里直接跑 `gf-table-runtime` 的房间主干：大厅开局 → 牌桌行动（含计时与托管）→
本局结算 → 手动「再来一局」。人类座位可在大厅选（默认座位 0），其余座位挂 bot，
档位可选 AI-L0 / L1 / L2（默认 L2，只读 `view(seat)` + 本座位合法集 + 搭档状态）。

## 为什么是 wasm 直跑

`crates/client-wasm` 把房间编成 `cdylib`，产物零 import，浏览器 `WebAssembly.instantiate`
裸加载即可，不需要 wasm-bindgen / npm / 构建工具链（和 `crates/engine/tests/wasm` 的 M0
验证同一套路）。规则集在编译期由 `include_str!` 内嵌，前端不需要额外加载夹具。

## 跑起来

```powershell
pwsh -File apps/client/build.ps1      # cargo build wasm + 复制到 apps/client
pwsh -File apps/client/serve.ps1      # 起静态服务 http://127.0.0.1:8123/
```

`file://` 打不开（wasm 必须走 fetch）。带查询参数会直接进牌桌（跳过大厅）：

```
http://127.0.0.1:8123/index.html?seed=0x51EDA101&seat=2&level=1
```

不带参数则停在**大厅**：填种子（1–16 位十六进制）、选座位与 bot 档位，点「开始比赛」。
该种子 + 座位已有存档时，大厅会提示条数并要求续局（也可点「继续上次对局」）。

## 表现层

纯 DOM + CSS，不引 PixiJS、不引外部素材。

- **大厅**：种子 / 座位 / 档位 / 开始（含存档检测与提示）。
- **牌桌**：四方位座位（自己永远在下方，其余按顺时针排）；左上角记分板显示双方本局级牌
  （「我方 X / 对方 Y」），打级方黄框、当前行动方 `▶` 加亮；座位卡给出我方/对方、手牌张数、
  最近一手，出完的座位打「头游 / 二游 / 三游 / 末游」标签。
  手牌**同点数一列纵向叠放**（只露左上角标），列序按牌力降序：大王、小王、级牌、A、K…2。
  点一张选一张，也可以**按住左键横向/纵向滑过**连续选中（起点是已选牌则整段改为取消）。
  出牌区停到**当前行动家桌前**（下/右/上/左四向），上家的牌横向展开并标注「上家/下家/对家 +
  牌型中文名」；连续两家过牌后牌面自动收起（点一下展开）。
  按钮：出牌 / 提示 / 过牌 / 取消选牌压在「选牌与牌型」行**上方**，只在轮到自己时出现，其余时间整行收起。
  出牌按 `gf_room_can_play` 判合法（选牌不合法时置灰）、**提示**在 `gf_room_legal` 里循环选一手
  （要不起时置灰）、领出时过牌置灰；非法出牌在中央区红字提示并保留选牌。
  手牌下方是**理牌条**，纯布局层：不动手牌、不动选中、不影响判定。
  「组牌建议」按合法集给每种牌型各挑一手自动理牌，炸弹 / 同花顺进最左列、其它进最右列，各最多 4 组；
  中间四个花色 ♠♥♦♣ 在能凑出该花色同花顺时高亮，反复点击在多个同花顺窗口间循环切换（中央提示「方案 x/y」）；
  「理成一列」把选中的牌整理掉——炸弹 / 同花顺去最左、其余去最右（只认牌型，不看压不压得住），
  重复理同一组会替换而不是叠加；「回复理牌」在选中牌正好等于某个理牌组时只恢复该组，否则恢复全部。
  理牌列可整组点选：点一列＝选中整组，再点＝取消。
- **结算**：名次表（**头游 / 二游 / 三游 / 末游** · 座位 · 我方-对方）、名次串、头游队；**不再自动续局**，
  点「再来一局」才发下一局；场次结束（过 A / 安全上限）时按钮置灰并提示回大厅。
- **设置**：出牌速度（120/240/600ms，影响 bot 连续行动的观感）、显示牌 id（调试）、
  动作日志侧栏开关、清空当前存档、刷新页面。设置存 `localStorage` 的 `guandan-m1-settings`。

窄屏（<1200px）自动收起日志侧栏，日志改为右侧浮层。

## 存档与续局

IndexedDB 库 `guandan-m1`：

- `matches`（主键 `matchId`）：`seedHi/seedLo/humanMask/humanSeat/level/lastHash/lastRoomSeq/lastRoundNo`
- `actions`（主键 `[matchId, seq]`）：`seq(roomSeq)/roundNo/seat/pass/cards`

每落一手动作就与「当前权威 `stateHash`」写进同一个事务，两者必然同步。所以「杀进程」
（关掉浏览器进程）之后只剩这份日志也能把局面复原：

1. `gf_room_new_level(seed, mask, level)` + `gf_room_start`；
2. 遇到 `roundNo` 递增先 `gf_room_next_round`，再逐条 `gf_room_replay_action`；
3. `gf_room_hash` 与存档 `lastHash` 比对，相等才继续 `gf_room_resume_window` + 推进。

页面上的「刷新页面（验证续局）」就是这条路径的等价演练：刷新会重建页面进程与 wasm 实例，
内存状态全部丢弃，只有 IndexedDB 留下。日志会打印恢复条数与「哈希一致 ✓/✗」。

## wasm 导出

| 导出 | 说明 |
|---|---|
| `gf_room_new(seed_hi, seed_lo, human_mask)` | 建桌，bot 固定 L2（兼容旧页面/脚本） |
| `gf_room_new_level(seed_hi, seed_lo, human_mask, bot_level)` | 建桌，`bot_level` 0/1/2 → L0/L1/L2 |
| `gf_room_start / step / submit / replay_action / next_round / resume_window` | 房间推进 |
| `gf_room_view / hash / meta / seats / last_record` | 视图与自证 |
| `gf_room_legal` | 当前行动座位的合法动作集（`[{"pass":false,"patternId":"bomb-4","cards":[…]}]`，Pass 项只含 `pass:true`），`patternId` 走 `preview_play` 得出；「提示」与「组牌建议」按它选牌 |
| `gf_room_can_play` | 预检一手牌能否出（与 `submit` 同口径）：结构不成立、不在手牌、压不过上家都返回 `ok:false`，同时回传识别出的 `patternId`；「出牌」按钮读它、理牌按 `patternId` 认牌型 |
| `gf_room_info` | 桌面公开信息（不进座位视图、不影响 `stateHash`）：相位、行动座位、连续过牌数、双方级牌、打级队、各家手牌张数与已锁定名次 |

## 无头回归

```powershell
node apps/client/test_restore.mjs
node apps/client/verify_wasm_hash.mjs
```

`test_restore.mjs` 在一个 Node 进程内跑完「两局 → 存档 → 丢弃 Instance → 新建 Instance 回放」，
断言恢复哈希与存档哈希相等且恢复后还能继续推进。这是 `apps/client` 的可自动化 gate。

## 已知边界

- 花色显示顺序按引擎 id 编码 `(suit×13+rank)×2+copy` 映射为 ♠♥♦♣；纯表现层，不参与判定。
- 其他座位的牌面仍不下发（座位视图只含本人手牌）；座位卡上的「手牌 N 张」来自房间公开信息
  `gf_room_info`（真实牌桌同样可见张数），名次同理。
- 「连续两家过牌后收起上家牌面」是纯表现层规则，按 `passesSince >= 2` 折叠，不影响可出牌判定。
- 人类超时托管走房间的 `Timeout` 分支：页面按 400ms 轮询 `step`，到点由房间代打。
- bot 在发牌时绑定，档位开局后锁定；换档位要回大厅重开。
