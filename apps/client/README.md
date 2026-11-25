# 掼蛋 M1 · Web 端（本地房间）

浏览器里直接跑 `gf-table-runtime` 的房间主干：发牌 → 行动窗口（含计时与托管）→ 出牌 →
结算 → 再来一局。座位 0 是你，座位 1–3 是 AI-L2（只读 `view(seat)` + 本座位合法集 + 搭档状态，不碰引擎状态）。

## 为什么是 wasm 直跑

`crates/client-wasm` 把房间编成 `cdylib`，产物零 import，浏览器 `WebAssembly.instantiate`
裸加载即可，不需要 wasm-bindgen / npm / 构建工具链（和 `crates/engine/tests/wasm` 的 M0
验证同一套路）。规则集在编译期由 `include_str!` 内嵌，前端不需要额外加载夹具。

## 跑起来

```powershell
pwsh -File apps/client/build.ps1      # cargo build wasm + 复制到 apps/client
pwsh -File apps/client/serve.ps1      # 起静态服务 http://127.0.0.1:8123/
```

`file://` 打不开（wasm 必须走 fetch）。种子可用查询参数指定：
`http://127.0.0.1:8123/index.html?seed=0x51EDA101`。

## 存档与续局

IndexedDB 库 `guandan-m1`：

- `matches`（主键 `matchId`）：`seedHi/seedLo/humanMask/lastHash/lastRoomSeq/lastRoundNo`
- `actions`（主键 `[matchId, seq]`）：`seq(roomSeq)/roundNo/seat/pass/cards`

每落一手动作就与「当前权威 `stateHash`」写进同一个事务，两者必然同步。所以「杀进程」
（关掉浏览器进程）之后只剩这份日志也能把局面复原：

1. `gf_room_new(seed)` + `gf_room_start`；
2. 遇到 `roundNo` 递增先 `gf_room_next_round`，再逐条 `gf_room_replay_action`；
3. `gf_room_hash` 与存档 `lastHash` 比对，相等才继续 `gf_room_resume_window` + 推进。

页面上的「模拟重开（刷新页面）」就是这条路径的等价演练：刷新会重建页面进程与 wasm 实例，
内存状态全部丢弃，只有 IndexedDB 留下。**实测**：13 条动作恢复后哈希逐字节一致。

## 无头回归

```powershell
node apps/client/test_restore.mjs
```

一个 Node 进程内跑完「两局 + 43 手 → 存档 → 丢弃 Instance → 新建 Instance 回放」，断言
恢复哈希与存档哈希相等且恢复后还能继续推进。这是 `apps/client` 的可自动化 gate。

## 已知边界

- 花色显示顺序按引擎 id 编码 `(suit×13+rank)×2+copy` 映射为 ♠♥♦♣；纯表现层，不参与判定。
- 其他座位的剩牌张数不下发（视图只含本人手牌），所以只显示等级。
- 人类超时托管走房间的 `Timeout` 分支；本页把虚拟时钟推进到 deadline 后由房间代打。
