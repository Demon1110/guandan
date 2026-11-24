//! 重连缓冲与恢复判定（`design_draft.md` §6.2 第 974–979 行、§6.5 第 1032–1034 行）。
//!
//! **待迁入 `crates/table-runtime`**（§2.4 第 186 行转正判据）：本模块的代码与测试
//! 会进入 §10.1 契约层双产物比对，M1 时整体迁移，不留副本。所有 public item 均带
//! 同一标注。
//!
//! 本轮边界（M0 in-process 原型，范围写死）：
//! - 快照直接传 [`State`] 克隆，**不做 wire 编解码**——那是 M1 `table-runtime` +
//!   `gf-protocol` 的事；本 crate 不依赖 `gf-protocol`（协议 crate 正由另一路并行修改）。
//! - 事件 / 记录用引擎侧 [`Action`] 承载；`tsMs` 用单调序号占位，不接系统时钟（§3.3 约束 2）。
//! - 不铺运行期框架，不建 `crates/table-runtime`。

use std::collections::VecDeque;

use gf_engine::contract::{Action, Engine, EngineError, State};
use gf_engine::engine::M0Engine;
use gf_engine::hash::{state_hash, CanonicalState};

/// §6.2 第 978 行：环形缓冲按 `roomSeq` 索引，保留最近 200 条事件。
///
/// 容量是真实值；**不许为了伪造 `SnapshotReset` 路径把容量调小**（路径②必须真的
/// 越过缓冲起点）。
pub const RECONNECT_BUFFER_CAPACITY: usize = 200;

/// §6.5 第 1032 行动作记录：`{seatSeq, roomSeq, seat, requestId, kind, payload, tsMs}`，
/// **只此一份记录类型**；重放水位一律用 `roomSeq`，座位序号只用于 `Reconnect` 水位与换算。
///
/// M0 边界：`kind` / `payload` 直接镜像在引擎 [`Action`] 上（`Play{cards}` / `Pass`）；
/// `ts_ms` 用房间序号占位，in-process 原型不接时钟。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionRecord {
    /// 每座位单调递增（§6.2 第 979 行），从 0 起算。
    pub seat_seq: u64,
    /// 房间内全局递增广播序（§6.2 第 979 行），从 0 起算；缓冲与回放水位都用它。
    pub room_seq: u64,
    /// 动作发起座位。
    pub seat: u8,
    /// 幂等键（§6.4）；M0 占位序号。
    pub request_id: u64,
    /// 动作本体（kind/payload 的引擎侧镜像）。
    pub action: Action,
    /// 时间戳占位（§6.5 `tsMs`）；原型不接时钟。
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

/// §5.9 第 716 行 `RoomSnapshot` 的 in-process 镜像。
///
/// M0 边界：直接挂 [`State`] 克隆，不做 canonical bytes 编解码；`viewHash` /
/// `rulesetId` / `viewSchemaVersion` 等协议字段本轮不投影（wire 层属 M1）。
#[derive(Clone, Debug)]
pub struct RoomSnapshot {
    /// 全量状态克隆（服务端权威状态）。
    pub state: State,
    /// §6.2 第 978 行：`true` 表示客户端必须丢弃本地状态并接受本快照。
    pub snapshot_reset: bool,
    /// 快照覆盖到的最大 `roomSeq`（与 §6.5 第 1034 行 `snapSeq` 同一 u64 序列）。
    pub room_seq: u64,
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
    /// 待迁入：缓冲容量（§6.2 第 978 行，固定 200）。
    pub fn capacity() -> usize {
        RECONNECT_BUFFER_CAPACITY
    }

    /// 待迁入：追加一条房间事件；超过容量时淘汰最旧的一条。
    pub fn push(&mut self, record: ActionRecord) {
        self.records.push_back(record);
        while self.records.len() > RECONNECT_BUFFER_CAPACITY {
            self.records.pop_front();
        }
    }

    /// 待迁入：缓冲内现存记录条数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 待迁入：缓冲是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 待迁入：缓冲起点 = 缓冲内最小 `roomSeq`（队首）。
    pub fn min_room_seq(&self) -> Option<u64> {
        self.records.front().map(|r| r.room_seq)
    }

    /// 待迁入：缓冲内最大 `roomSeq`（队尾）。
    pub fn latest_room_seq(&self) -> Option<u64> {
        self.records.back().map(|r| r.room_seq)
    }

    /// 待迁入：按动作记录双序号列做 `seatSeq → roomSeq` 换算（§6.2 第 978 行、
    /// §6.5 第 1032 行）；记录已被淘汰时返回 `None`。
    pub fn room_seq_for_seat(&self, seat: u8, seat_seq: u64) -> Option<u64> {
        self.records
            .iter()
            .find(|r| r.seat == seat && r.seat_seq == seat_seq)
            .map(|r| r.room_seq)
    }

    /// 待迁入：取 `room_seq > from_room_seq` 的增量事件，按 `roomSeq` 升序返回。
    pub fn delta_after(&self, from_room_seq: u64) -> Vec<ActionRecord> {
        self.records
            .iter()
            .filter(|r| r.room_seq > from_room_seq)
            .cloned()
            .collect()
    }

    /// 待迁入：缓冲内记录快照（测试 / 诊断用，按 `roomSeq` 升序）。
    pub fn records(&self) -> &VecDeque<ActionRecord> {
        &self.records
    }
}

/// 待迁入：§6.2 第 978 行重连判定。
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

/// 待迁入：增量重放。从客户端本地副本出发，按 `roomSeq` 升序逐条 `apply`；
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

/// 待迁入：恢复后哈希自证。`CanonicalState` 单向投影 → BLAKE3-256 32 字节全量。
pub fn state_hash_of(state: &State) -> [u8; 32] {
    state_hash(&CanonicalState::from(state))
}

/// 待迁入：32 字节全量十六进制（测试打印 / 比对用）。
///
/// 注意：`gf_engine::hash::present_8` 只取前 8 字节对外呈现，**不能替代 32 字节全等比对**。
pub fn hex_full(hash: &[u8; 32]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(room_seq: u64, seat: u8, seat_seq: u64) -> ActionRecord {
        ActionRecord {
            seat_seq,
            room_seq,
            seat,
            request_id: room_seq,
            action: Action::Pass,
            ts_ms: room_seq,
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
}
