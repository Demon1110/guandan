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

最后一条是设计缺口 `M2-GAP-proto-04`：`ActionAck.reason` 要求取自 §5.7 全表，
但"玩家误点被引擎拒绝"在表里没有码。裁决前先用常量占位，裁决后只改一处常量。

## 已知取舍（都在代码里有注释）

- **重连两条路径**：水位仍在 200 条环形缓冲内、且没跨局、没落在结算空窗期 → 逐条补发
  `RoomDelta`，末尾附 `snapshotReset=false` 的快照（客户端保留本地状态、用 `viewHash` 自证）；
  其余情形一律 `snapshotReset=true`。跨局必须重发视图（新手牌表达不了），这条是刻意选择。
- **动作序号两个口径**：`ActionRecord.seatSeq` 从 0 起算（`lastSeatSeq` / 快照水位取它）；
  `SubmitAction.seatSeq` 与 `STALE_SEQ` 比的是「下一条动作序号」，比记录值大 1。
  两者由 `Room::seat_watermark` / `Room::seat_seq` 分开暴露，不要混用。
- **自动续局**：`M2-OPEN-01`，联机"再来一局"触发方未裁定（会动 `actions.kind` 闭集）。
- **进房即建房**：只为本机联调，正式匹配/房间码在账号链路上线后替换。
- **未接持久化**：房间状态只在内存，进程重启即丢；PG/Redis 在 M2 后段接入。

## 加测试的落点

- 规则相关 → 不要写在 `apps/server`，写 `crates/table-runtime/tests/`。
- 服务端行为（幂等、水位、入座分配、广播帧形状）→ `tests/online_round.rs`。
- 传输层（握手、帧编解码、版本闸门）→ `tests/ws_smoke.rs`。
- 帧头编号 → `crates/protocol/src/wire.rs` 的单元测试（只增不减）。
