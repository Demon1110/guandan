//! 一张联机桌的宿主：权威 `Room` + 每座位下行通道 + 主循环。
//!
//! 职责边界：
//! - 判定全部交给 `gf-table-runtime::Room`，本层不重复规则；
//! - 本层只做「入座 / 收动作 / 广播下行 / 断线记账」四件事；
//! - 账号、匹配、落库不在本层（M2 后续切片）。
//!
//! 时钟：一律用 `RoomHost` 自己的单调毫秒（进程起点起算），不读挂钟，
//! 房间层的时间参数保持调用方可控。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gf_engine::contract::{Action, State};
use gf_engine::engine::PHASE_SETTLE;
use gf_protocol::gf::protocol::v1 as pb;
use gf_table_runtime::{
    AutoReason, PlayerKind, Room, RoomConfig, RoomError, SettleSummary, StepOutcome,
    DEFAULT_RULESET_VERSION,
};
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::events;
use crate::store::{RoundRow, StorageEvent};

/// 掼蛋经典四人座。
pub const SEATS: u8 = 4;

/// 房间码/匹配成房后，HTTP 先把座位预留给客户端，等待它接 WS 的最长时长。
///
/// 预留只是「别让两个人抢同一把椅子」的软占位：带 token 时预留绑定玩家 ID，
/// 只有同一玩家能兑现；无账号（token 缺省）时退化为「带同一座位号即兑现」。
pub const RESERVATION_TTL_MS: u64 = 60_000;

/// 未在 §5.7 错误码全表内的临时动作级拒绝码。
///
/// 设计缺口（M2-GAP-proto-04，待项目方裁决）：玩家误点导致的引擎拒绝
/// （`EngineError::IllegalAction`）在 §5.7 全表里没有对应码，而 `ActionAck.reason`
/// 又要求「不在表内即违约」。本切片先用本常量占位，裁决后替换为正式码。
pub const REASON_ILLEGAL_ACTION: &str = "ILLEGAL_ACTION";

/// 房间主循环参数。
#[derive(Clone, Copy, Debug)]
pub struct RoomOptions {
    /// 主循环节拍（毫秒）。
    pub tick_ms: u64,
    /// 结算后自动续局间隔（毫秒）；0 表示不自动续局。
    pub next_deal_ms: u64,
}

impl Default for RoomOptions {
    fn default() -> Self {
        Self {
            tick_ms: 50,
            next_deal_ms: 5_000,
        }
    }
}

/// 入座拒绝原因。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinError {
    /// 目标座位或被占、或房间已满（`RS_SEAT_TAKEN`）。
    SeatTaken { available: Vec<u8> },
}

/// 一条座位预留。
#[derive(Clone, Debug)]
struct Reservation {
    until_ms: u64,
    /// `Some(player_id)` 表示该座位绑定到某个账号；`None` 是匿名预留（本机联调）。
    owner: Option<String>,
}

/// 一张联机桌。
pub struct RoomHost {
    match_id: String,
    room: Mutex<Room>,
    out: RwLock<HashMap<u8, mpsc::UnboundedSender<Vec<u8>>>>,
    /// HTTP 阶段预占的座位，WS 入座时兑现。
    reserved: Mutex<HashMap<u8, Reservation>>,
    /// 已入座的座位属于哪个玩家（账号阶段用于重连时校验身份）。
    owners: RwLock<HashMap<u8, String>>,
    /// 存档出口：结算 / 场次终止时投递（不阻塞房间锁）。
    recorder: std::sync::Mutex<Option<mpsc::UnboundedSender<StorageEvent>>>,
    ack_cache: Mutex<HashMap<(u8, String), Vec<u8>>>,
    started_at: Instant,
    opts: RoomOptions,
    started: AtomicBool,
    next_deal_at: AtomicU64,
    announced_settle: AtomicU64,
    /// 本局发牌时刻（墙钟毫秒），结算行要用。
    round_start_ms: AtomicU64,
    closed: AtomicBool,
}

impl RoomHost {
    /// 建桌并启动主循环（需在 tokio 运行时内调用）。
    pub fn spawn(
        match_id: impl Into<String>,
        seed: u64,
        opts: RoomOptions,
    ) -> Result<Arc<Self>, RoomError> {
        let match_id = match_id.into();
        let room = Room::new(RoomConfig {
            match_id: match_id.clone(),
            ruleset_version: DEFAULT_RULESET_VERSION.to_owned(),
            seed,
            players: vec![PlayerKind::Human; SEATS as usize],
        })?;
        let host = Arc::new(Self {
            match_id,
            room: Mutex::new(room),
            out: RwLock::new(HashMap::new()),
            reserved: Mutex::new(HashMap::new()),
            owners: RwLock::new(HashMap::new()),
            recorder: std::sync::Mutex::new(None),
            ack_cache: Mutex::new(HashMap::new()),
            started_at: Instant::now(),
            opts,
            started: AtomicBool::new(false),
            next_deal_at: AtomicU64::new(0),
            announced_settle: AtomicU64::new(0),
            round_start_ms: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        });

        let weak = Arc::downgrade(&host);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(opts.tick_ms.max(5)));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                let Some(host) = weak.upgrade() else { break };
                host.pump().await;
            }
        });
        Ok(host)
    }

    /// 房间是否已终止（过 A / 安全上限 / 流局）。
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    /// 单调毫秒（进程起点起算）。
    pub fn now_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }

    /// 已在线座位（按座位号升序）。
    pub async fn connected_seats(&self) -> Vec<u8> {
        let mut seats: Vec<u8> = self.out.read().await.keys().copied().collect();
        seats.sort_unstable();
        seats
    }

    /// 入座：分配空座位、登记下行通道。
    ///
    /// `hint` 只用于续座（带回原座位）；不传则按最小空座分配。
    /// 满员是否发牌由 [`RoomHost::start_if_full`] 负责——调用方必须先回 `JoinAck`
    /// 再调它，否则新座位的第一帧会变成快照而不是入座回执。
    pub async fn join(
        &self,
        hint: Option<u8>,
        tx: mpsc::UnboundedSender<Vec<u8>>,
    ) -> Result<u8, JoinError> {
        self.join_as(hint, tx, None).await
    }

    /// 带身份的入座：账号预留的座位只由同一 `owner` 兑现（账号切片起收紧）。
    pub async fn join_as(
        &self,
        hint: Option<u8>,
        tx: mpsc::UnboundedSender<Vec<u8>>,
        owner: Option<String>,
    ) -> Result<u8, JoinError> {
        let now = self.now_ms();
        // 锁序固定：先 `out` 再 `reserved`（`reserve` 同序），避免交叉等待。
        let mut out = self.out.write().await;
        let mut reserved = self.reserved.lock().await;
        reserved.retain(|_, r| r.until_ms > now);
        let occupied: Vec<u8> = out.keys().copied().collect();
        let owner_ref = owner.as_deref();
        let seat = match hint {
            Some(seat) if seat < SEATS && !occupied.contains(&seat) => {
                if let Some(reservation) = reserved.get(&seat) {
                    // 预留者若不是本人（且预留带了身份），不得抢占。
                    if reservation
                        .owner
                        .as_deref()
                        .is_some_and(|o| Some(o) != owner_ref)
                    {
                        return Err(JoinError::SeatTaken {
                            available: free_for(&occupied, &reserved, owner_ref),
                        });
                    }
                }
                reserved.remove(&seat);
                seat
            }
            Some(_) => {
                return Err(JoinError::SeatTaken {
                    available: free_for(&occupied, &reserved, owner_ref),
                })
            }
            None => {
                // 有身份的入座优先兑现自己那条预留。
                let mine = owner_ref.and_then(|o| {
                    reserved
                        .iter()
                        .find(|(s, r)| !occupied.contains(s) && r.owner.as_deref() == Some(o))
                        .map(|(s, _)| *s)
                });
                match mine.or_else(|| {
                    (0..SEATS).find(|s| !occupied.contains(s) && !reserved.contains_key(s))
                }) {
                    Some(seat) => {
                        reserved.remove(&seat);
                        seat
                    }
                    None => {
                        return Err(JoinError::SeatTaken {
                            available: free_for(&occupied, &reserved, owner_ref),
                        })
                    }
                }
            }
        };
        out.insert(seat, tx);
        drop(out);
        drop(reserved);
        if let Some(owner) = owner {
            self.owners.write().await.insert(seat, owner);
        }
        Ok(seat)
    }

    /// 该座位绑定的玩家（未带 token 入座的座位为 `None`）。
    pub async fn seat_owner(&self, seat: u8) -> Option<String> {
        self.owners.read().await.get(&seat).cloned()
    }

    /// 挂上存档通道（`App` 启动时挂一次；不挂时结算不落库）。
    pub fn attach_recorder(&self, tx: mpsc::UnboundedSender<StorageEvent>) {
        *self
            .recorder
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(tx);
    }

    /// 投递一条存档事件；通道满/无接收者都直接丢弃（对局不因落库失败卡住）。
    fn record(&self, event: StorageEvent) {
        let recorder = self
            .recorder
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(tx) = recorder.as_ref() {
            let _ = tx.send(event);
        }
    }

    /// 房间快照的元信息（`roomSeq`、`roundNo`、`stateHash`、相位），供 Redis 快照任务用。
    pub async fn snapshot_meta(&self) -> Option<(u64, u16, [u8; 32], u16)> {
        let room = self.room.lock().await;
        let state = room.state().ok()?;
        Some((
            room.room_seq(),
            state.round_no,
            room.state_hash().ok()?,
            state.phase,
        ))
    }

    /// 预占一个空座位（房间码 / 匹配成房后由 HTTP 返回给客户端）。
    ///
    /// 预留 `RESERVATION_TTL_MS` 后自动失效；WS 带该座位号入座即兑现。
    pub async fn reserve(&self) -> Result<u8, JoinError> {
        self.reserve_for(None).await
    }

    /// 带身份预占：只有同一 `owner` 能兑现该座位。
    pub async fn reserve_for(&self, owner: Option<String>) -> Result<u8, JoinError> {
        let now = self.now_ms();
        let out = self.out.read().await;
        let mut reserved = self.reserved.lock().await;
        reserved.retain(|_, r| r.until_ms > now);
        let seat = (0..SEATS).find(|s| !out.contains_key(s) && !reserved.contains_key(s));
        match seat {
            Some(seat) => {
                reserved.insert(
                    seat,
                    Reservation {
                        until_ms: now.saturating_add(RESERVATION_TTL_MS),
                        owner,
                    },
                );
                Ok(seat)
            }
            None => Err(JoinError::SeatTaken {
                available: Vec::new(),
            }),
        }
    }

    /// 满四人即发牌开桌，并给每个座位各发一份全量快照 + 相位切换。
    ///
    /// 幂等：已开局则什么都不做。
    pub async fn start_if_full(&self) {
        if self.started.load(Ordering::Relaxed) || self.out.read().await.len() as u8 != SEATS {
            return;
        }
        let now = self.now_ms();
        let (snapshots, broadcast) = {
            let mut room = self.room.lock().await;
            if room.state().is_err() {
                let _ = room.start(now);
                self.round_start_ms
                    .store(crate::now_unix_ms(), Ordering::Relaxed);
            }
            self.started.store(true, Ordering::Relaxed);
            let deadline = room.turn_deadline_ms().unwrap_or(now);
            let mut snapshots: Vec<(u8, Vec<u8>)> = Vec::new();
            let Ok(state) = room.state().cloned() else {
                return;
            };
            for seat in 0..SEATS {
                if let Ok(wire) = room.snapshot_wire(seat, true) {
                    snapshots.push((seat, events::snapshot_frame(&wire, true)));
                }
            }
            (snapshots, vec![events::phase_frame(state.phase, deadline)])
        };
        self.send_seats(snapshots).await;
        self.send_all(&broadcast).await;
    }

    /// 提交动作：校验幂等 → 委托房间判定 → 广播增量 → 回执。
    ///
    /// 返回要回给提交者的 `ActionAck` 帧。
    pub async fn submit(&self, seat: u8, msg: pb::SubmitAction, now: u64) -> Vec<u8> {
        let request_id = msg.request_id.clone();
        if let Some(cached) = self.ack_cache.lock().await.get(&(seat, request_id.clone())) {
            return cached.clone();
        }

        let action = match action_of(&msg) {
            Ok(action) => action,
            Err(()) => return self.reject(seat, &request_id, REASON_ILLEGAL_ACTION).await,
        };
        if self.closed.load(Ordering::Relaxed) {
            return self.reject(seat, &request_id, "TOO_LATE").await;
        }

        let mut frames: Vec<(u8, Vec<u8>)> = Vec::new();
        let mut broadcast: Vec<Vec<u8>> = Vec::new();
        let ack = {
            let mut room = self.room.lock().await;
            let watermark = room.seat_watermark(seat).unwrap_or(0);
            if room.state().is_err() {
                events::action_ack_frame(
                    &request_id,
                    watermark,
                    room.room_seq(),
                    false,
                    Some("TOO_LATE"),
                )
            } else if room.seat_seq(seat).is_ok_and(|next| msg.seat_seq < next) {
                // 座位水位：低于「下一条动作序号」即 STALE_SEQ（§6.2）。
                events::action_ack_frame(
                    &request_id,
                    watermark,
                    room.room_seq(),
                    false,
                    Some("STALE_SEQ"),
                )
            } else {
                let before = room.state().ok().cloned().unwrap();
                let outcome = room.submit(seat, action, now);
                let ack_seat_seq = room.seat_watermark(seat).unwrap_or(0);
                let mut ack_room_seq = room.room_seq();
                let ack = match outcome {
                    Ok(StepOutcome::Applied {
                        seat: acted,
                        action,
                        auto,
                    }) => {
                        if let Some(record) = room.log().last() {
                            ack_room_seq = record.room_seq;
                        }
                        if let Ok(state) = room.state() {
                            let deadline = room.turn_deadline_ms().unwrap_or(now);
                            let events = events::events_for(&before, state, &action, deadline);
                            broadcast.push(events::delta_frame(
                                &self.match_id,
                                ack_room_seq,
                                events,
                            ));
                        }
                        if let Some(reason) = auto {
                            frames.push((
                                acted,
                                events::trustee_frame(acted, auto_reason_str(reason)),
                            ));
                        }
                        events::action_ack_frame(
                            &request_id,
                            ack_seat_seq,
                            ack_room_seq,
                            true,
                            None,
                        )
                    }
                    Ok(StepOutcome::Settled(summary)) => {
                        broadcast.extend(self.settle_frames(&room, &before, &summary, now));
                        events::action_ack_frame(
                            &request_id,
                            ack_seat_seq,
                            ack_room_seq,
                            true,
                            None,
                        )
                    }
                    // 人类提交只在行动窗口内合法；走到这两个分支按竞态落败处理。
                    Ok(StepOutcome::Waiting { .. }) | Ok(StepOutcome::MatchEnded(_)) => {
                        events::action_ack_frame(
                            &request_id,
                            ack_seat_seq,
                            ack_room_seq,
                            false,
                            Some("TOO_LATE"),
                        )
                    }
                    Err(err) => {
                        let reason = match err {
                            RoomError::WrongTurn { .. }
                            | RoomError::NotInPlay
                            | RoomError::NotStarted => "TOO_LATE",
                            _ => REASON_ILLEGAL_ACTION,
                        };
                        events::action_ack_frame(
                            &request_id,
                            ack_seat_seq,
                            ack_room_seq,
                            false,
                            Some(reason),
                        )
                    }
                };
                ack
            }
        };

        self.send_seats(frames).await;
        self.send_all(&broadcast).await;
        self.ack_cache
            .lock()
            .await
            .insert((seat, request_id), ack.clone());
        ack
    }

    /// 重连：增量帧（走增量时）+ 全量快照。
    ///
    /// 两条路径（§6.2）：
    /// - 增量：客户端水位仍在 200 条缓冲内且没跨局/没落在结算空窗期 → 逐条补发 `RoomDelta`，
    ///   末尾附一份 `snapshotReset = false` 的快照（用于校验 `viewHash` 与刷新水位）；
    /// - 快照重置：其余情况 → 只发 `snapshotReset = true` 的快照，客户端丢弃本地状态。
    ///
    /// 返回的帧按发送顺序排列，调用方必须顺序下发（增量在快照之前）。
    pub async fn reconnect(&self, seat: u8, last_seat_seq: u64) -> Result<Vec<Vec<u8>>, RoomError> {
        let mut room = self.room.lock().await;
        let delta = room.reconnect_delta(seat, last_seat_seq)?;
        let plan = room.reconnect(seat, last_seat_seq)?;
        let mut frames = Vec::new();
        if let Some((_from_room_seq, steps)) = delta {
            let deadline = room.turn_deadline_ms().unwrap_or(0);
            frames.extend(events::replay_frames(&self.match_id, &steps, deadline));
        }
        frames.push(events::snapshot_frame(&plan.wire, plan.wire.snapshot_reset));
        Ok(frames)
    }

    /// 入座时的首帧快照（`snapshotReset = true`）。
    ///
    /// 与 [`RoomHost::reconnect`] 的区别：新入座的客户端本地没有状态，也不该拿座位历史做增量，
    /// 只能接受全量视图。
    pub async fn join_snapshot(&self, seat: u8) -> Result<Vec<u8>, RoomError> {
        let room = self.room.lock().await;
        let wire = room.snapshot_wire(seat, true)?;
        Ok(events::snapshot_frame(&wire, true))
    }

    /// 房间号。
    pub fn match_id(&self) -> &str {
        &self.match_id
    }

    /// 重连前把本座位的新连接登记为下行通道（旧连接若还在，直接被顶掉）。
    pub async fn rebind(&self, seat: u8, tx: mpsc::UnboundedSender<Vec<u8>>) -> bool {
        if seat >= SEATS {
            return false;
        }
        self.out.write().await.insert(seat, tx);
        true
    }

    /// 座位「下一条动作序号」（`STALE_SEQ` 判定用）。
    pub async fn seat_seq(&self, seat: u8) -> u64 {
        self.room.lock().await.seat_seq(seat).unwrap_or(0)
    }

    /// 座位已应用到的水位（`ActionAck.seat_seq` / 快照 `lastSeatSeq` / 重连水位用）。
    ///
    /// 与 [`RoomHost::seat_seq`] 相差 1：动作记录里的 `seatSeq` 从 0 起算。
    pub async fn seat_watermark(&self, seat: u8) -> u64 {
        self.room.lock().await.seat_watermark(seat).unwrap_or(0)
    }

    /// 房间广播序。
    pub async fn room_seq(&self) -> u64 {
        self.room.lock().await.room_seq()
    }

    /// 座位离线记账（WS 断开 / 心跳超时）。
    pub async fn leave(&self, seat: u8, now: u64) {
        self.out.write().await.remove(&seat);
        let mut room = self.room.lock().await;
        let _ = room.disconnect(seat, now);
    }

    /// 权威状态副本（管理端巡检 / 联机测试驱动用）。
    pub async fn state_snapshot(&self) -> Option<State> {
        self.room.lock().await.state().ok().cloned()
    }

    /// 当前行动座位的合法动作（联机测试驱动用）。
    pub async fn legal_actions(&self) -> Vec<Action> {
        self.room.lock().await.legal_now().unwrap_or_default()
    }

    /// 权威状态哈希（服务端自证与回放比对用，永不下发）。
    pub async fn state_hash(&self) -> Result<[u8; 32], RoomError> {
        self.room.lock().await.state_hash()
    }

    /// 回放日志重算哈希（与 `state_hash` 相等即双端同解自证）。
    pub async fn replay_hash(&self) -> Result<[u8; 32], RoomError> {
        self.room.lock().await.replay_hash()
    }

    async fn reject(&self, seat: u8, request_id: &str, reason: &str) -> Vec<u8> {
        let room = self.room.lock().await;
        events::action_ack_frame(
            request_id,
            room.seat_watermark(seat).unwrap_or(0),
            room.room_seq(),
            false,
            Some(reason),
        )
    }

    /// 主循环一拍：托管 / 超时 / 结算续局，并把增量发出去。
    async fn pump(&self) {
        if self.closed.load(Ordering::Relaxed) {
            return;
        }
        let now = self.now_ms();
        let mut per_seat: Vec<(u8, Vec<u8>)> = Vec::new();
        let mut broadcast: Vec<Vec<u8>> = Vec::new();

        {
            let mut room = self.room.lock().await;
            if room.state().is_err() {
                return;
            }
            let next_at = self.next_deal_at.load(Ordering::Relaxed);
            let before = room.state().ok().cloned().unwrap();

            if before.phase == PHASE_SETTLE && next_at != 0 && now >= next_at {
                // 结算后定时续局（M2-OPEN-01：触发方待裁定）。
                if room.next_round(now).is_ok() {
                    self.next_deal_at.store(0, Ordering::Relaxed);
                    self.round_start_ms
                        .store(crate::now_unix_ms(), Ordering::Relaxed);
                    let deadline = room.turn_deadline_ms().unwrap_or(now);
                    if let Ok(state) = room.state().cloned() {
                        for seat in 0..SEATS {
                            if let Ok(wire) = room.snapshot_wire(seat, true) {
                                per_seat.push((seat, events::snapshot_frame(&wire, true)));
                            }
                        }
                        broadcast.push(events::phase_frame(state.phase, deadline));
                    }
                }
            } else {
                let _ = room.tick(now);
                match room.step(now) {
                    Ok(StepOutcome::Applied { seat, action, auto }) => {
                        if let Ok(state) = room.state().cloned() {
                            let deadline = room.turn_deadline_ms().unwrap_or(now);
                            let room_seq = room
                                .log()
                                .last()
                                .map(|r| r.room_seq)
                                .unwrap_or_else(|| room.room_seq());
                            let events = events::events_for(&before, &state, &action, deadline);
                            broadcast.push(events::delta_frame(&self.match_id, room_seq, events));
                            if let Some(reason) = auto {
                                per_seat.push((
                                    seat,
                                    events::trustee_frame(seat, auto_reason_str(reason)),
                                ));
                            }
                        }
                    }
                    Ok(StepOutcome::Settled(summary)) => {
                        broadcast.extend(self.settle_frames(&room, &before, &summary, now));
                    }
                    Ok(StepOutcome::MatchEnded(end)) => {
                        self.closed.store(true, Ordering::Relaxed);
                        self.record(StorageEvent::MatchEnded {
                            match_id: self.match_id.clone(),
                            winner_team: end.winner_team,
                            end_reason: end.reason.code_str(),
                            ended_at_ms: now,
                        });
                    }
                    Ok(StepOutcome::Waiting { .. }) | Err(_) => {}
                }
            }
        }

        self.send_seats(per_seat).await;
        self.send_all(&broadcast).await;
    }

    /// 结算帧：增量（含最后一手）+ 结算帧 + 相位切换；同一局只发一次。
    fn settle_frames(
        &self,
        room: &Room,
        before: &State,
        summary: &SettleSummary,
        now: u64,
    ) -> Vec<Vec<u8>> {
        let announced = self.announced_settle.load(Ordering::Relaxed);
        if announced == u64::from(summary.round_no) {
            return Vec::new();
        }
        self.announced_settle
            .store(u64::from(summary.round_no), Ordering::Relaxed);

        let mut frames = Vec::new();
        if let Some(record) = room.log().last() {
            if let Ok(state) = room.state() {
                let deadline = room.turn_deadline_ms().unwrap_or(now);
                let events = events::events_for(before, state, &record.action, deadline);
                frames.push(events::delta_frame(&self.match_id, record.room_seq, events));
            }
        }
        let levels = room.state().map(|s| s.levels.clone()).unwrap_or_default();
        frames.push(events::settle_frame(
            summary,
            room.room_seq(),
            &levels,
            self.opts.next_deal_ms,
        ));
        frames.push(events::phase_frame(PHASE_SETTLE, 0));

        // 结算事实投给存档任务：players 由 App 按座位归属补，房间不碰账号。
        let actions: Vec<_> = room
            .log()
            .iter()
            .filter(|record| record.round_no == summary.round_no)
            .cloned()
            .collect();
        self.record(StorageEvent::RoundSettled {
            match_id: self.match_id.clone(),
            row: RoundRow {
                match_id: self.match_id.clone(),
                round_no: summary.round_no,
                players: Vec::new(),
                ranks: summary.ranks.clone(),
                result_json: events::settle_json(summary),
                levels_after: levels,
                ruleset_version: gf_table_runtime::DEFAULT_RULESET_VERSION.to_owned(),
                started_at_ms: self.round_start_ms.load(Ordering::Relaxed),
                ended_at_ms: crate::now_unix_ms(),
            },
            actions,
        });
        self.next_deal_at.store(
            now.saturating_add(self.opts.next_deal_ms),
            Ordering::Relaxed,
        );
        frames
    }

    async fn send_seats(&self, frames: Vec<(u8, Vec<u8>)>) {
        let out = self.out.read().await;
        for (seat, frame) in frames {
            if let Some(tx) = out.get(&seat) {
                let _ = tx.send(frame);
            }
        }
    }

    async fn send_all(&self, frames: &[Vec<u8>]) {
        let out = self.out.read().await;
        for frame in frames {
            for tx in out.values() {
                let _ = tx.send(frame.clone());
            }
        }
    }
}

/// 可分配座位：空座里排除「被别人预留」的那些。
///
/// 匿名预留（`owner = None`）仍算可分配：它的兑现条件本来就是「带同一座位号」；
/// 带身份的预留只对同一玩家可见。
fn free_for(
    occupied: &[u8],
    reserved: &HashMap<u8, Reservation>,
    requester: Option<&str>,
) -> Vec<u8> {
    (0..SEATS)
        .filter(|seat| !occupied.contains(seat))
        .filter(
            |seat| match reserved.get(seat).and_then(|r| r.owner.as_deref()) {
                None => true,
                Some(owner) => requester == Some(owner),
            },
        )
        .collect()
}

fn auto_reason_str(reason: AutoReason) -> &'static str {
    match reason {
        AutoReason::Timeout => "timeout",
        AutoReason::Offline => "offline",
        AutoReason::BotIllegal => "bot_illegal",
    }
}

/// 把 `SubmitAction` 翻成引擎动作；`kind` 与 `payload` 分支必须一致（§5.9）。
fn action_of(msg: &pb::SubmitAction) -> Result<Action, ()> {
    let kind = pb::ActionKind::try_from(msg.kind).map_err(|_| ())?;
    match (kind, msg.payload.as_ref()) {
        (pb::ActionKind::Play, Some(pb::submit_action::Payload::Play(play))) => {
            let mut cards = Vec::with_capacity(play.cards.len());
            for card in &play.cards {
                cards.push(u16::try_from(*card).map_err(|_| ())?);
            }
            Ok(Action::Play { cards })
        }
        (pb::ActionKind::Pass, Some(pb::submit_action::Payload::Pass(_))) => Ok(Action::Pass),
        // `kind` 与分支不一致：服务端双向校验，直接拒。
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_action_kind_and_payload_must_agree() {
        let mut msg = pb::SubmitAction {
            match_id: "m".to_owned(),
            seat: 0,
            request_id: "r1".to_owned(),
            seat_seq: 1,
            kind: pb::ActionKind::Play as i32,
            payload: Some(pb::submit_action::Payload::Pass(pb::PassPayload {})),
        };
        assert!(action_of(&msg).is_err(), "play + pass 分支必须拒");

        msg.payload = Some(pb::submit_action::Payload::Play(pb::PlayPayload {
            cards: vec![0, 1],
            pattern_hint: None,
        }));
        match action_of(&msg) {
            Ok(Action::Play { cards }) => assert_eq!(cards, vec![0, 1]),
            other => panic!("期望 play，实际 {other:?}"),
        }
    }

    #[test]
    fn pass_without_payload_is_rejected() {
        let msg = pb::SubmitAction {
            match_id: "m".to_owned(),
            seat: 0,
            request_id: "r2".to_owned(),
            seat_seq: 1,
            kind: pb::ActionKind::Pass as i32,
            payload: None,
        };
        assert!(action_of(&msg).is_err());
    }

    #[test]
    fn card_id_out_of_u16_range_is_rejected() {
        let msg = pb::SubmitAction {
            match_id: "m".to_owned(),
            seat: 0,
            request_id: "r3".to_owned(),
            seat_seq: 1,
            kind: pb::ActionKind::Play as i32,
            payload: Some(pb::submit_action::Payload::Play(pb::PlayPayload {
                cards: vec![70_000],
                pattern_hint: None,
            })),
        };
        assert!(action_of(&msg).is_err(), "牌 id 必须落在 u16 域");
    }

    #[test]
    fn free_seats_excludes_occupied_and_other_owners_reservations() {
        let reservation = |owner: Option<&str>| Reservation {
            until_ms: u64::MAX,
            owner: owner.map(|o| o.to_owned()),
        };
        assert_eq!(free_for(&[], &HashMap::new(), None), vec![0, 1, 2, 3]);

        let mut reserved = HashMap::new();
        reserved.insert(1, reservation(Some("p-1")));
        reserved.insert(2, reservation(None));
        // p-2 看不到 p-1 的座位，但能看到匿名预留（带座位号即可兑现）。
        assert_eq!(free_for(&[0], &reserved, Some("p-2")), vec![2, 3]);
        // p-1 能看到自己的预留。
        assert_eq!(free_for(&[0], &reserved, Some("p-1")), vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn reservation_binds_to_owner() {
        let host = RoomHost::spawn("m-owner", 7, RoomOptions::default()).expect("建桌");
        let seat = host
            .reserve_for(Some("p-1".to_owned()))
            .await
            .expect("预留");
        let (tx, _rx) = mpsc::unbounded_channel();

        let other = host
            .join_as(Some(seat), tx.clone(), Some("p-2".to_owned()))
            .await;
        assert!(
            matches!(other, Err(JoinError::SeatTaken { .. })),
            "别人不得抢占已绑定的预留"
        );

        let same = host
            .join_as(Some(seat), tx, Some("p-1".to_owned()))
            .await
            .expect("本人兑现预留");
        assert_eq!(same, seat);
    }
}
