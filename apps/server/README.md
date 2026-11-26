# apps/server · 联机服务端（M2）

口袋级说明：怎么跑、代码怎么分层、哪些口径还没定。规则与协议的权威仍然是
`docs/designs/2026-11-23-guandan-multiplatform/design_draft.md`，排期见 `docs/开发计划.md` §3。

## 跑起来

```powershell
cd apps/server
cargo run --bin gf-server          # 默认 127.0.0.1:8080
cargo test                         # 单测 + 房间闭环 + WS 冒烟
cargo clippy --all-targets -- -D warnings
```

手工探针：先起服务，再 `cargo run --example ws_probe`，会发一条 `JoinRoom` 并打印收到的帧类型。

## 分层

| 文件 | 负责 | 不负责 |
| --- | --- | --- |
| `src/lib.rs` | HTTP 路由、WS 会话（入座/收帧/断线记账） | 任何规则判定 |
| `src/host.rs` | 一张桌的权威 `Room` + 每座位下行通道 + 20Hz 主循环 | 账号、匹配、落库 |
| `src/events.rs` | 前后状态差分 → §5.9 下行帧（`RoomDelta` / `SettleResult` / `RoomSnapshot`） | 事件语义的新增裁定 |
| `src/config.rs` | 环境变量、`minClientVersion` 比较 | 规则集加载（由 room 锁定） |

判定链路只有一条：`WS → RoomHost::submit → gf-table-runtime::Room::submit → gf-engine`。
服务端不重算牌型、不缓存"合法动作"，免得与 `stateHash` 口径分叉。

## 一局的时序

1. 四个座位 `JoinRoom` → `JoinAck`（各自），第四座入座后 `start_if_full()` 发牌；
   每个座位收到一份 `RoomSnapshot(snapshotReset=true)` + `PhaseChange(play)`。
2. 主循环 50ms 一拍：`tick`（全员离线判定）→ `step`（bot / 托管 / 人类等待）。
   人类在线且未超时 → `Waiting`，什么都不发。
3. 人类 `SubmitAction`：幂等查 `requestId` → 校验 `seatSeq` → `Room::submit`
   → 广播 `RoomDelta`（同一 `roomSeq` 可带多条事件）→ 回 `ActionAck`。
4. 终局 → `RoomDelta`（含最后一手 + `RoundSettledEvent`）+ `SettleResult` + `PhaseChange(settle)`；
   `next_deal_ms` 后自动开下一局（触发方待裁，见下）。

## 大厅接口（房间码 / 匹配队列）

`M2` 第二切片，按 §5.15 的玩家侧接口实现，状态全在内存（`src/lobby.rs`）：

| 方法 | 路径 | 入参 / 出参 |
| --- | --- | --- |
| `POST` | `/api/room/create` | `{gameId, mode, seatsCount}` → `{matchId, inviteCode, matchType, seatsCount}` |
| `POST` | `/api/room/join` | `{inviteCode}` → `{matchId, seat, accept, availableSeats, reason?}`（与 `JoinAck` 同构） |
| `POST` | `/api/match/queue` | `{gameId, mode}` → `{ticketId}` |
| `GET` | `/api/match/queue/{ticketId}` | → `{state: waiting\|matched\|timeout, matchId?}` |
| `DELETE` | `/api/match/queue/{ticketId}` | → `{ok}` |

- `mode` 是 `casual` / `ranked` / `mixed`，映射到 `matchType`：`human-casual` / `human-ranked` / `mixed`。
- 房间码 6 位、去掉易混字符（无 `0/1/I/L/O`），大小写不敏感，有效期默认 24h（`GF_ROOM_CODE_TTL_MS`）。
- 满员走 `reason: RS_SEAT_TAKEN` + `availableSeats: []`，**不返回 HTTP 409**（§5.15 明确）。
- 匹配队列按 `(gameId, mode)` 分池、FIFO；每次入队触发一次撮合，四张票成房后各自轮询到 `matched`。
  等待超过 `GF_MATCH_TIMEOUT_MS`（默认 30s）的票在下次轮询/撮合时转 `timeout`。
- 入座前调用 `RoomHost::reserve()` 预占座位（60s TTL），`POST /api/room/join` 拿到的 `seat`
  在随后 WS `JOIN` 里兑现；座位被占时按最小空闲座位重新预占。

## 已知取舍（都在代码里有注释）

- **重连两条路径**：水位仍在 200 条环形缓冲内、且没跨局、没落在结算空窗期 → 逐条补发
  `RoomDelta`，末尾附 `snapshotReset=false` 的快照（客户端保留本地状态、用 `viewHash` 自证）；
  其余情形一律 `snapshotReset=true`。跨局必须重发视图（新手牌表达不了），这条是刻意选择。
- **动作序号两个口径**：`ActionRecord.seatSeq` 从 0 起算（`lastSeatSeq` / 快照水位取它）；
  `SubmitAction.seatSeq` 与 `STALE_SEQ` 比的是「下一条动作序号」，比记录值大 1。
  两者由 `Room::seat_watermark` / `Room::seat_seq` 分开暴露，不要混用。
- **自动续局**：`M2-OPEN-01`，联机"再来一局"触发方未裁定（会动 `actions.kind` 闭集）。
- **大厅是内存态**：房码表、票号表、匹配队列都在进程内，重启即清；Redis
  键位（`match:queue:{gameId}`）留到持久化切片接入。
- **撮合只在入队时触发**：没有后台撮合线程，同时入队的四人能立刻成房；错峰入队靠下一
  个入队者带着前面的票成房，等价于「有人进队才撮一次」。
- **预留座位不校验身份**：`RoomHost::reserve` 只按座位号兑现，谁先连上谁坐；
  账号切片接 token 后收紧。
- **未接持久化**：房间状态只在内存，进程重启即丢；PG/Redis 在 M2 后段接入。

## 错误码怎么落

只允许 §5.7 全表里的值。当前映射：

| 场景 | `reason` |
| --- | --- |
| 目标座位被占 / 房间已满 | `RS_SEAT_TAKEN`（经 `JoinAck`，附 `availableSeats`） |
| 请求 `seat` 与绑定座位不一致 | `AUTH_INVALID` |
| 客户端版本低于下限 | `CLIENT_TOO_OLD` |
| `seat_seq` 低于该座位水位 | `STALE_SEQ` |
| 非行动座位提交 / 不在出牌阶段 / 场次已终止 | `TOO_LATE` |
| 引擎拒绝（误点非法牌型） | **`ILLEGAL_ACTION`（占位，表内无对应码）** |
| 大厅入参不合规（未知房码/票号/游戏/模式/座位数） | `RS_SCHEMA_INVALID` + `detail`（见 `M2-GAP-05`） |

最后两条是设计缺口：`M2-GAP-proto-04`（`ActionAck.reason` 要求取自 §5.7 全表，但"玩家误点
被引擎拒绝"在表里没有码）；`M2-GAP-05`（§5.7 也没有"邀请码/票号不存在"的码，先用
`RS_SCHEMA_INVALID` + `detail` 承载原因）。裁决后各改一处常量。

## 加测试的落点

- 规则相关 → 不要写在 `apps/server`，写 `crates/table-runtime/tests/`。
- 服务端行为（幂等、水位、入座分配、广播帧形状）→ `tests/online_round.rs`。
- 传输层（握手、帧编解码、版本闸门）→ `tests/ws_smoke.rs`。
- 帧头编号 → `crates/protocol/src/wire.rs` 的单元测试（只增不减）。
