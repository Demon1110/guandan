//! 重连缓冲与恢复判定（`design_draft.md` §6.2 第 974–979 行、§6.5 第 1032–1034 行）。
//!
//! 由 `crates/runtime-spike/src/recovery.rs` 整体迁入（§2.4 第 186 行转正判据：
//! 进入 §10.1 契约层双产物比对的代码与测试必须保留并迁入 `crates/table-runtime`），
//! 迁移后不复留副本。相对 spike 版的唯一改动：
//! - 快照载体从 `State` 克隆的 in-process 结构改为 [`RoomSnapshotState`]，`viewHash` 等
//!   wire 头字段由 [`crate::snapshot::RoomSnapshotWire`] 承载（服务端下发前自证）；
//! - `decide` / `replay_delta` / `recover_client` 的签名与判定规则不变。
//!
//! 边界（保持不变）：`tsMs` 用房间序号占位，不接系统时钟（§3.3 约束 2）。

use std::collections::VecDeque;

use gf_engine::contract::{Action, Engine, EngineError, State};
use gf_engine::engine::M0Engine;
use gf_engine::hash::{state_hash, CanonicalState};

/// §6.2 第 978 行：环形缓冲按 `roomSeq` 索引，保留最近 200 条事件。
///
/// 容量是真实值；**不许为了伪造 `SnapshotReset` 路径把容量调小**（路径②必须真的
/// 越过缓冲起点）。
pub const RECONNECT_BUFFER_CAPACITY: usize = 200;

/// §6.5 第 1100 行动作记录：`{seatSeq, roomSeq, roundNo, seat, requestId, kind, payload, tsMs}`，
/// **只此一份记录类型**；重放水位一律用 `roomSeq`，座位序号只用于 `Reconnect` 水位与换算。
///
/// `kind` / `payload` 直接镜像在引擎 [`Action`] 上（`Play{cards}` / `Pass`）；
/// `ts_ms` 用房间序号占位（虚拟时钟毫秒另由 [`crate::room::Room`] 的窗口字段承载）。
/// `round_no`（第二十三次变更）与 `SettleResult.roundNo` 同口径：取动作发生时的当前局号；
/// 同一 `matchId` 内跨局连续，`settle → deal` 之间的记录仍属本局。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionRecord {
    /// 每座位单调递增（§6.2 第 979 行），从 0 起算；跨局不归零。
    pub seat_seq: u64,
    /// 房间内全局递增广播序（§6.2 第 979 行），从 0 起算；跨局不归零。缓冲与回放水位都用它。
    pub room_seq: u64,
    /// 局序号 `roundNo: u16`，回放按它切段（§6.5 第 1100 行）。
    pub round_no: u16,
    /// 动作发起座位。
    pub seat: u8,
    /// 幂等键（§6.4）；本层用 `roomSeq` 占位（同一房间内唯一且单调）。
    pub request_id: u64,
    /// 动作本体（kind/payload 的引擎侧镜像）。
    pub action: Action,
    /// 时间戳占位（§6.5 `tsMs`）；本层取虚拟时钟毫秒。
    pub ts_ms: u64,
}

/// §6.2 第 978 行重连判定结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReconnectDecision {
    /// 增量路径：`lastSeatSeq` 对应 `roomSeq` 仍在缓冲覆盖范围内，回放
    /// `(from_room_seq, ..]` 区间内的事件；对应 §5.9 `RoomDelta{matchId, roomSeq, events[]}`。
    Delta {
        /// 客户端最后已应用的房间序号；回放事件 `room_seq` 严格大于它。
        from_room_seq: u64,
        events: Vec<ActionRecord>,
    },
    /// 快照重置路径：`lastSeatSeq` 对应 `roomSeq` 早于缓冲起点（或记录已被淘汰），
    /// 不回放增量；客户端必须丢弃本地状态，接受服务端全量快照（`snapshotReset = true`）。
    SnapshotReset,
}

/// 服务端恢复用的全量快照（in-process 形态：权威 `State` + wire 头字段）。
///
/// 线上下发走 [`crate::snapshot::RoomSnapshotWire`]（只带 `view` 字节，不带 `State`）；
/// 本结构只用于「服务端自身也要按快照状态重建」的恢复判定与测试。
#[derive(Clone, Debug)]
pub struct RoomSnapshotState {
    /// 全量状态克隆（服务端权威状态）。
    pub state: State,
    /// §6.2 第 978 行：`true` 表示客户端必须丢弃本地状态并接受本快照。
    pub snapshot_reset: bool,
    /// 快照覆盖到的最大 `roomSeq`（与 §6.5 第 1034 行 `snapSeq` 同一 u64 序列）。
    pub room_seq: u64,
    /// 快照头 `roundNo`（§6.2 裁定 B）：快照时刻正在打的局号；不在 `view` 键集内。
    pub round_no: u16,
    /// §6.5 `viewHash`：`BLAKE3(seat ‖ roomSeq ‖ viewBytes)` 前 8 字节。
    pub view_hash: [u8; 8],
    /// §6.5 `viewSchemaVersion`：取自规则集 `view.schemaVersion`。
    pub view_schema_version: u16,
}

/// 环形事件缓冲：按 `roomSeq` 索引，只保留最近 [`RECONNECT_BUFFER_CAPACITY`] 条。
///
/// 缓冲起点 = 缓冲内最小 `roomSeq`（§6.2 第 978 行）。记录按 `roomSeq` 到达顺序入队，
/// 超出容量时队首（最旧）被淘汰。
#[derive(Clone, Debug, Default)]
pub struct ReconnectBuffer {
    records: VecDeque<ActionRecord>,
}

impl ReconnectBuffer {
    /// 缓冲容量（§6.2 第 978 行，固定 200）。
    pub fn capacity() -> usize {
        RECONNECT_BUFFER_CAPACITY
    }

    /// 追加一条房间事件；超过容量时淘汰最旧的一条。
    pub fn push(&mut self, record: ActionRecord) {
        self.records.push_back(record);
        while self.records.len() > RECONNECT_BUFFER_CAPACITY {
            self.records.pop_front();
        }
    }

    /// 缓冲内现存记录条数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 缓冲是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 缓冲起点 = 缓冲内最小 `roomSeq`（队首）。
    pub fn min_room_seq(&self) -> Option<u64> {
        self.records.front().map(|r| r.room_seq)
    }

    /// 缓冲内最大 `roomSeq`（队尾）。
    pub fn latest_room_seq(&self) -> Option<u64> {
        self.records.back().map(|r| r.room_seq)
    }

    /// 按动作记录双序号列做 `seatSeq → roomSeq` 换算（§6.2 第 978 行、
    /// §6.5 第 1032 行）；记录已被淘汰时返回 `None`。
    pub fn room_seq_for_seat(&self, seat: u8, seat_seq: u64) -> Option<u64> {
        self.records
            .iter()
            .find(|r| r.seat == seat && r.seat_seq == seat_seq)
            .map(|r| r.room_seq)
    }

    /// 取 `room_seq > from_room_seq` 的增量事件，按 `roomSeq` 升序返回。
    pub fn delta_after(&self, from_room_seq: u64) -> Vec<ActionRecord> {
        self.records
            .iter()
            .filter(|r| r.room_seq > from_room_seq)
            .cloned()
            .collect()
    }

    /// 缓冲内记录快照（测试 / 诊断用，按 `roomSeq` 升序）。
    pub fn records(&self) -> &VecDeque<ActionRecord> {
        &self.records
    }
}

/// §6.2 第 978 行重连判定。
///
/// 先按双序号列把 `(seat, last_seat_seq)` 换算成 `roomSeq`；该 `roomSeq` 不早于缓冲
/// 起点 → [`ReconnectDecision::Delta`]；早于缓冲起点（或记录已被淘汰）→
/// [`ReconnectDecision::SnapshotReset`]。
pub fn decide(buffer: &ReconnectBuffer, seat: u8, last_seat_seq: u64) -> ReconnectDecision {
    let Some(room_seq) = buffer.room_seq_for_seat(seat, last_seat_seq) else {
        // 记录已被淘汰：其 roomSeq 必然早于缓冲起点，保守落 SnapshotReset。
        return ReconnectDecision::SnapshotReset;
    };
    match buffer.min_room_seq() {
        Some(start) if room_seq >= start => ReconnectDecision::Delta {
            from_room_seq: room_seq,
            events: buffer.delta_after(room_seq),
        },
        _ => ReconnectDecision::SnapshotReset,
    }
}

/// 增量重放。从客户端本地副本出发，按 `roomSeq` 升序逐条 `apply`；
/// 任一步失败即返回 [`EngineError`]，不修补状态（§6.6：日志与快照对不上不允许修补）。
pub fn replay_delta(
    engine: &M0Engine,
    mut state: State,
    events: &[ActionRecord],
) -> Result<State, EngineError> {
    for record in events {
        state = engine.apply(state, record.action.clone())?;
    }
    Ok(state)
}

/// 客户端本地副本（线上为客户端私有状态，不进服务端权威源）。
#[derive(Clone, Debug)]
pub struct ClientLocal {
    /// 本地状态副本；`SnapshotReset` 丢弃后为 `None`，接受快照后再填回。
    pub state: Option<State>,
    /// 本地所属座位（`Reconnect` 绑定座位，§5.9 第 713 行）。
    pub seat: u8,
    /// 该座位本地已应用到的最后 `seatSeq`（`lastSeatSeq` 水位）。
    pub last_seat_seq: u64,
    /// 记账字段：本次恢复是否走过"丢弃本地状态"分支（证明清空先于接受快照）。
    pub discarded: bool,
}

impl ClientLocal {
    /// 用本地副本与断线座位水位构造；`discarded` 初始为 false。
    pub fn new(state: Option<State>, seat: u8, last_seat_seq: u64) -> Self {
        Self {
            state,
            seat,
            last_seat_seq,
            discarded: false,
        }
    }
}

/// [`recover_client`] 的恢复结果；用 enum 表达分支，不只返回裸状态。
#[derive(Clone, Debug)]
pub enum RecoveryOutcome {
    /// 增量路径：本地原状态为 `Some`，逐条重放增量后得到新状态。
    Delta {
        state: State,
        last_seat_seq: u64,
        events_applied: usize,
    },
    /// 快照重置路径：已先丢弃本地状态（`discarded`），再接受全量快照。
    SnapshotReset {
        state: State,
        /// 丢弃前本地是否持有状态副本。
        discarded: bool,
        /// 对应 `RoomSnapshot.snapshot_reset`（§6.2 第 978 行，应为 true）。
        snapshot_reset: bool,
    },
}

/// 客户端恢复统一入口（两条路径共用一套，不由测试各自拼装）。
///
/// - `Delta`：本地 `state` 必须为 `Some`（`None` 直接返回错误，不许凭空造状态），
///   从 `from_room_seq + 1` 逐条重放，并把该座位 `last_seat_seq` 推进到增量末条对应值。
/// - `SnapshotReset`：先把 `local.state` 置 `None`、`last_seat_seq` 清零、记 `discarded`，
///   **再**接受快照填回本地状态；清空动作发生在接受快照之前。
pub fn recover_client(
    engine: &M0Engine,
    local: &mut ClientLocal,
    decision: &ReconnectDecision,
    snapshot: &RoomSnapshotState,
) -> Result<RecoveryOutcome, EngineError> {
    match decision {
        ReconnectDecision::Delta { events, .. } => {
            let Some(state) = local.state.take() else {
                return Err(EngineError::Internal {
                    message: "Delta 重放需要本地状态副本，但 local.state 为 None".to_owned(),
                });
            };
            let state = replay_delta(engine, state, events)?;
            // 推进该座位水位到增量内属于本座位的末条 seatSeq。
            let mut last_seq = local.last_seat_seq;
            for record in events {
                if record.seat == local.seat {
                    last_seq = last_seq.max(record.seat_seq);
                }
            }
            let applied = events.len();
            local.state = Some(state.clone());
            local.last_seat_seq = last_seq;
            Ok(RecoveryOutcome::Delta {
                state,
                last_seat_seq: last_seq,
                events_applied: applied,
            })
        }
        ReconnectDecision::SnapshotReset => {
            // 协议要求：先丢弃本地状态，再接受新快照（§6.2 第 978 行）。
            let discarded = local.state.is_some();
            local.state = None;
            local.last_seat_seq = 0;
            local.discarded = true;
            let state = snapshot.state.clone();
            local.state = Some(state.clone());
            Ok(RecoveryOutcome::SnapshotReset {
                state,
                discarded,
                snapshot_reset: snapshot.snapshot_reset,
            })
        }
    }
}

/// 恢复后哈希自证。`CanonicalState` 单向投影 → BLAKE3-256 32 字节全量。
pub fn state_hash_of(state: &State) -> [u8; 32] {
    state_hash(&CanonicalState::from(state))
}

/// 32 字节全量十六进制（测试打印 / 比对用）。
///
/// 注意：`gf_engine::hash::present_8` 只取前 8 字节对外呈现，**不能替代 32 字节全等比对**。
pub fn hex_full(hash: &[u8; 32]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gf_engine::cases::classic_engine;
    use gf_engine::contract::Seat;
    use gf_engine::view::view_hash;

    fn record(room_seq: u64, seat: u8, seat_seq: u64) -> ActionRecord {
        ActionRecord {
            seat_seq,
            room_seq,
            round_no: 1,
            seat,
            request_id: room_seq,
            action: Action::Pass,
            ts_ms: room_seq,
        }
    }

    fn make_snapshot(
        engine: &M0Engine,
        state: State,
        snapshot_reset: bool,
        room_seq: u64,
    ) -> RoomSnapshotState {
        let view = engine.view(&state, Seat(0));
        RoomSnapshotState {
            round_no: state.round_no,
            view_hash: view_hash(0, room_seq, &view.bytes),
            view_schema_version: engine.ruleset.view.schema_version,
            state,
            snapshot_reset,
            room_seq,
        }
    }

    #[test]
    fn buffer_keeps_only_latest_200() {
        let mut buffer = ReconnectBuffer::default();
        for room_seq in 0..250u64 {
            let seat = (room_seq % 4) as u8;
            buffer.push(record(room_seq, seat, room_seq / 4));
        }
        assert_eq!(buffer.len(), RECONNECT_BUFFER_CAPACITY);
        assert_eq!(buffer.min_room_seq(), Some(50));
        assert_eq!(buffer.latest_room_seq(), Some(249));
        // 被淘汰的旧记录不应再按双序号列查到。
        assert!(buffer.room_seq_for_seat(0, 0).is_none());
        // 缓冲内首条记录的双序号列仍可换算。
        assert_eq!(buffer.room_seq_for_seat(2, 12), Some(50));
    }

    #[test]
    fn seat_seq_to_room_seq_conversion_uses_double_index_column() {
        let mut buffer = ReconnectBuffer::default();
        buffer.push(record(0, 0, 0));
        buffer.push(record(1, 1, 0));
        buffer.push(record(2, 0, 1));
        assert_eq!(buffer.room_seq_for_seat(0, 0), Some(0));
        assert_eq!(buffer.room_seq_for_seat(0, 1), Some(2));
        assert_eq!(buffer.room_seq_for_seat(1, 0), Some(1));
        // 别的座位同号 seatSeq 不得串号。
        assert_eq!(buffer.room_seq_for_seat(2, 0), None);
    }

    #[test]
    fn decide_delta_when_last_seq_still_within_buffer() {
        let mut buffer = ReconnectBuffer::default();
        for room_seq in 0..30u64 {
            let seat = (room_seq % 4) as u8;
            buffer.push(record(room_seq, seat, room_seq / 4));
        }
        // seat 0 最后一条记录 roomSeq=28（seatSeq=7）。
        let decision = decide(&buffer, 0, 7);
        match decision {
            ReconnectDecision::Delta {
                from_room_seq,
                events,
            } => {
                assert_eq!(from_room_seq, 28);
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].room_seq, 29);
            }
            other => panic!("期望 Delta，实际 {other:?}"),
        }
    }

    #[test]
    fn decide_snapshot_reset_when_record_evicted() {
        let mut buffer = ReconnectBuffer::default();
        for room_seq in 0..250u64 {
            let seat = (room_seq % 4) as u8;
            buffer.push(record(room_seq, seat, room_seq / 4));
        }
        // seat 0 的 seatSeq=0 对应 roomSeq=0，已被淘汰 → SnapshotReset。
        let decision = decide(&buffer, 0, 0);
        assert!(matches!(decision, ReconnectDecision::SnapshotReset));
        assert!(!matches!(decision, ReconnectDecision::Delta { .. }));
    }

    #[test]
    fn delta_rejects_missing_local_state() {
        let engine = classic_engine().expect("构造引擎失败");
        let decision = ReconnectDecision::Delta {
            from_room_seq: 0,
            events: Vec::new(),
        };
        let snapshot = make_snapshot(
            &engine,
            engine.deal(gf_engine::cases::M0_SEED).expect("发牌失败"),
            false,
            0,
        );
        let mut local = ClientLocal::new(None, 0, 0);
        let err = recover_client(&engine, &mut local, &decision, &snapshot)
            .expect_err("Delta 分支本地状态为 None 必须返回错误，不许凭空造状态");
        assert!(matches!(err, EngineError::Internal { .. }));
        assert!(local.state.is_none());
    }

    #[test]
    fn reset_discards_local_state_before_accepting_snapshot() {
        let engine = classic_engine().expect("构造引擎失败");
        let local_state = engine.deal(gf_engine::cases::M0_SEED).expect("发牌失败");
        let snap_state = engine.deal(gf_engine::cases::M0_SEED).expect("发牌失败");
        let snapshot = make_snapshot(&engine, snap_state.clone(), true, 5);
        let mut local = ClientLocal::new(Some(local_state), 0, 7);

        let outcome = recover_client(
            &engine,
            &mut local,
            &ReconnectDecision::SnapshotReset,
            &snapshot,
        )
        .expect("快照重置恢复失败");

        match outcome {
            RecoveryOutcome::SnapshotReset {
                state,
                discarded,
                snapshot_reset,
            } => {
                assert!(discarded, "丢弃前本地持有副本应记 discarded=true");
                assert!(snapshot_reset, "快照重置分支必须带出 snapshot_reset=true");
                assert_eq!(state, snap_state);
            }
            other => panic!("期望 SnapshotReset，实际 {other:?}"),
        }
        // 记账字段证明清空发生在接受快照之前；水位已清零。
        assert!(local.discarded);
        assert_eq!(local.last_seat_seq, 0);
        assert_eq!(local.state, Some(snap_state));
    }
}
