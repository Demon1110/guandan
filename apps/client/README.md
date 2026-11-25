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
- **牌桌**：四方位座位（自己永远在下方，其余按顺时针排）、中央出牌区（牌型 + 牌面）、
  手牌按点数升序（2…A、小王、大王）横排可横向滚动、点选高亮、领出时「过牌」禁用、
  非法出牌在中央区红字提示并保留选牌。
- **结算**：名次表（名次 / 座位 / 我方-对方）、名次串、头游队；**不再自动续局**，
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

## 无头回归

```powershell
node apps/client/test_restore.mjs
node apps/client/verify_wasm_hash.mjs
```

`test_restore.mjs` 在一个 Node 进程内跑完「两局 → 存档 → 丢弃 Instance → 新建 Instance 回放」，
断言恢复哈希与存档哈希相等且恢复后还能继续推进。这是 `apps/client` 的可自动化 gate。

## 已知边界

- 花色显示顺序按引擎 id 编码 `(suit×13+rank)×2+copy` 映射为 ♠♥♦♣；纯表现层，不参与判定。
- 其他座位的剩牌张数不下发（视图只含本人手牌），座位卡显示「最近一手」；两队等级在中央区。
- 人类超时托管走房间的 `Timeout` 分支：页面按 400ms 轮询 `step`，到点由房间代打。
- bot 在发牌时绑定，档位开局后锁定；换档位要回大厅重开。
