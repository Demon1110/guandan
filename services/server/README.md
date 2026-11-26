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

关键环境变量（全部有开发默认值，生产必须覆盖前两项）：

| 变量 | 作用 |
| --- | --- |
| `GF_AUTH_SALT` / `GF_JWT_SECRET` | 手机号哈希盐、token 签名密钥（默认值是写死的开发串，启动会告警） |
| `GF_SMS_TEST_WHITELIST` | 逗号分隔的手机号；短信模板未批前用固定测试码走通登录 |
| `GF_SMS_TEST_CODE` | 白名单固定验证码，默认 `123456` |
| `GF_REQUIRE_AUTH` | `1`/`true` 时玩家侧接口与 WS 都必须带 access token |
| `GF_ROOM_CODE_TTL_MS` / `GF_MATCH_TIMEOUT_MS` | 房间码有效期 / 匹配等待上限 |
| `GF_DATABASE_URL` / `GF_REDIS_URL` | PostgreSQL / Redis 连接串；不填即纯内存运行（见「持久化」） |

## 分层

| 文件 | 负责 | 不负责 |
| --- | --- | --- |
| `src/lib.rs` | HTTP 路由、WS 会话（鉴权/入座/收帧/断线记账） | 任何规则判定 |
| `src/host.rs` | 一张桌的权威 `Room` + 每座位下行通道 + 20Hz 主循环 | 账号、匹配、落库 |
| `src/events.rs` | 前后状态差分 → §5.9 下行帧（`RoomDelta` / `SettleResult` / `RoomSnapshot`） | 事件语义的新增裁定 |
| `src/lobby.rs` | 房间码、票号、匹配队列（内存态） | 房间托管、规则 |
| `src/auth.rs` | 手机号规范化/哈希、验证码、access/refresh token、玩家档案（内存态） | 短信通道报备、PG 落库 |
| `src/store.rs` | PG 连接与建表、副局/场次/账号落库、Redis 快照/动作流/队列镜像（全部可缺席） | 规则判定、重试与死信（M3） |
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

## 账号接口（手机号 + 短信）

| 方法 | 路径 | 入参 / 出参 |
| --- | --- | --- |
| `POST` | `/api/auth/sms` | `{phone}` → `{requestId, cooldownSec}`；同号 60s/次、10 次/日，超限 `429 RATE_LIMITED` + `retryAfterMs` |
| `POST` | `/api/auth/login` | `{phone, code, clientVersion}` → `{token, refreshToken, expiresIn, playerId}`；`clientVersion` 低于下限 `426 CLIENT_TOO_OLD`，码错/超期 `401 AUTH_INVALID` |
| `POST` | `/api/auth/refresh` | `{refreshToken}` → `{token, refreshToken, expiresIn}`；旧 refresh 即刻作废（单端轮换） |
| `GET` | `/api/profile` | 需 `Authorization: Bearer <token>` → `{playerId, nickname, phoneTail4, stats}`；`stats` 在 PG 切片前恒为空表 |

- access 是 HS256 JWT（15 分钟），refresh 是 32 字节随机串（30 天），库里只存其 SHA-256。
- 手机号规范化后取 `HMAC-SHA256(server_salt, phone)` 与尾号 4 位入库（`player_profile` 字段位），原文只在发码链路内存里出现。
- 短信通道未报备前，`GF_SMS_TEST_WHITELIST` 里的号用固定测试码直通（不发短信）；白名单为空时该口子关闭。
- 带 token 入座会把座位绑定到玩家；重连回座位时校验同一玩家，HTTP 预占也按玩家 ID 绑定（见下节）。

## 持久化（PostgreSQL / Redis）

`M2` 第四切片。两侧各自独立可缺席：连接串不填、连不上、建表失败都只打日志并降级，
联机对局不因存储不可用而拒客。

| 变量 | 落点 |
| --- | --- |
| `GF_DATABASE_URL` | 启动时执行 `migrations/0001_init.sql`（幂等）；写 `player_profile`、`match_record`、`round_record`、`settlement_event` |
| `GF_REDIS_URL` | `room:{matchId}:snapshot`（30s 刷新、TTL 60s）、`room:{matchId}:actions`（Stream，`MAXLEN ~ 4096`）、`match:queue:{gameId}`（zset 镜像） |

- 落库发生在房间之外的独立任务里：`RoomHost` 只把「结算 / 场次终止」两类事实投进
  `mpsc`，房间锁从不 `await` 数据库；`App` 侧补座位归属（`player_id`）后写 PG。
- 一局落库在同一事务里先插 `settlement_event` 再写 `round_record`（§6.6 的结算计数口径），
  两条都 `ON CONFLICT DO NOTHING`，重放同一局不会写出两条战绩。
- `round_record.players` 形如 `[{seat, player_id, rank}]`；匿名入座 `player_id` 为 `null`，
  等 `GF_REQUIRE_AUTH` 全面打开后才会全是有值。
- `round_record.started_at / ended_at` 用 epoch 毫秒（`bigint`），不用 PG 的时区类型。
- 匹配队列以内存为准，Redis 里只是镜像；队列跨实例共享要等粘性路由。

## 已知取舍（都在代码里有注释）

- **重连两条路径**：水位仍在 200 条环形缓冲内、且没跨局、没落在结算空窗期 → 逐条补发
  `RoomDelta`，末尾附 `snapshotReset=false` 的快照（客户端保留本地状态、用 `viewHash` 自证）；
  其余情形一律 `snapshotReset=true`。跨局必须重发视图（新手牌表达不了），这条是刻意选择。
- **动作序号两个口径**：`ActionRecord.seatSeq` 从 0 起算（`lastSeatSeq` / 快照水位取它）；
  `SubmitAction.seatSeq` 与 `STALE_SEQ` 比的是「下一条动作序号」，比记录值大 1。
  两者由 `Room::seat_watermark` / `Room::seat_seq` 分开暴露，不要混用。
- **自动续局**：`M2-OPEN-01`，联机"再来一局"触发方未裁定（会动 `actions.kind` 闭集）。
- **大厅是内存态**：房码表、票号表、匹配队列都在进程内，重启即清；Redis 里的
  `match:queue:{gameId}` 只是镜像，读回来仍以内存为准。
- **撮合只在入队时触发**：没有后台撮合线程，同时入队的四人能立刻成房；错峰入队靠下一
  个入队者带着前面的票成房，等价于「有人进队才撮一次」。
- **预留座位不校验身份**：`RoomHost::reserve` 只按座位号兑现，谁先连上谁坐；
  账号切片接 token 后收紧。
- **预留座位绑定玩家**：带 token 时预占与入座都记 `player_id`，别人拿同一座位号也抢不走；
  不带 token（`GF_REQUIRE_AUTH` 关闭）退化为匿名预留，仅按座位号兑现。
- **账号表已落库、令牌仍在内存**：`player_profile` 登录即 upsert；验证码表与 refresh 表还在
  进程内（重启失效），换存储不影响 HTTP 契约与字段位。
- **落库无重试**：写失败只打日志；`settlement_dead_letter` / `settlement_compensation` 两表已建好，
  补偿流程排在 M3。
- **动作流按局整体写入**：结算时把本局动作一次性 `XADD`，Redis 不可用时该局动作只留在进程内，
  不阻塞结算。
- **refresh 只存哈希**：库里（现在是内存）存 `sha256(refresh)`，泄库也拿不到可用凭据；
  但每账号只允许一条活跃 refresh，多端会互相顶掉（§5.15「不允许多端并行刷新」的直译）。
- **房间状态只在内存**：进程重启后 PG 里有战绩、Redis 里有最后一次快照，但房间对象不恢复；
  冷启动续桌属 M3。

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
| 发码过频/超日限 | `RATE_LIMITED`（429，附 `retryAfterMs`） |
| 大厅入参不合规（未知房码/票号/游戏/模式/座位数） | `RS_SCHEMA_INVALID` + `detail`（见 `M2-GAP-05`） |

最后两条是设计缺口：`M2-GAP-proto-04`（`ActionAck.reason` 要求取自 §5.7 全表，但"玩家误点
被引擎拒绝"在表里没有码）；`M2-GAP-05`（§5.7 也没有"邀请码/票号不存在"的码，先用
`RS_SCHEMA_INVALID` + `detail` 承载原因）。裁决后各改一处常量。

## 加测试的落点

- 规则相关 → 不要写在 `apps/server`，写 `crates/table-runtime/tests/`。
- 服务端行为（幂等、水位、入座分配、广播帧形状）→ `tests/online_round.rs`。
- 传输层（握手、帧编解码、版本闸门）→ `tests/ws_smoke.rs`。
- 帧头编号 → `crates/protocol/src/wire.rs` 的单元测试（只增不减）。
