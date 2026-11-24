//! 一次性夹具：造局、固定策略驱动、模拟断线 / 掉线时长、把服务端推进 N 条事件。
//!
//! **不迁入 `crates/table-runtime`**（§2.4 第 186 行）：M1 直接丢弃。
//!
//! 分层红线：服务端权威状态推进、客户端本地副本、断线模拟都归本模块；
//! 环形缓冲 / 判定 / 增量重放 / 哈希自证归 [`crate::recovery`]，两段逻辑不许揉在
//! 同一个函数里——本模块只负责"推进服务端并产出 [`ActionRecord`]"，不做重连判定。

#![allow(dead_code)]

use gf_engine::cases::{build_engine, M0_SEED};
use gf_engine::contract::{Action, Engine, EngineError, State};
use gf_engine::engine::{M0Engine, PHASE_SETTLE};

use crate::recovery::{ActionRecord, ReconnectBuffer, RoomSnapshot};

/// 构造 m0-minimal 引擎（夹具口径见 `gf_engine::cases`）。
pub fn engine() -> M0Engine {
    build_engine().expect("m0-minimal 夹具构造失败")
}

/// 固定策略：取 `legal_actions` 排序后的第一个（与 m0 golden 同一确定性序列）。
pub fn first_action(engine: &M0Engine, state: &State) -> Action {
    engine
        .legal_actions(state)
        .expect("枚举合法动作失败")
        .into_iter()
        .next()
        .expect("非终局候选不应为空")
}

/// 服务端房间推进器（一次性夹具）：持有权威状态、`roomSeq` 全局计数器与每座位
/// `seatSeq` 水位。
///
/// 序号口径（§6.2 第 979 行）：`roomSeq` 房间内全局递增；`seatSeq` 每座位各自递增；
/// 两者均从 0 起算，不混用。
///
/// TODO(M0-GAP-SEQ-CROSS-ROUND)：设计稿未写清跨局（settle 后重新 `deal`）时
/// `seatSeq` 是否归零、`roomSeq` 是否连续。本原型按保守读法：**跨局都不归零**，
/// `roomSeq` 连续递增、`seatSeq` 按座位各自继续递增；待 §6.2 / §5.9 裁决后同步。
pub struct ServerRoom {
    pub engine: M0Engine,
    pub state: State,
    /// 下一条房间广播序（全局连续）。
    next_room_seq: u64,
    /// 每座位下一条动作序（各自单调）。
    seat_seq: [u64; 4],
    next_request_id: u64,
}

impl ServerRoom {
    /// 开局：`deal` 固定种子，序号从 0 起算。
    pub fn new(engine: M0Engine) -> Result<Self, EngineError> {
        let state = engine.deal(M0_SEED)?;
        Ok(Self {
            engine,
            state,
            next_room_seq: 0,
            seat_seq: [0; 4],
            next_request_id: 0,
        })
    }

    /// 该座位当前 `seatSeq` 水位（下一条动作将用的序号）。
    pub fn seat_seq(&self, seat: u8) -> u64 {
        self.seat_seq[seat as usize]
    }

    /// 该座位**最后一次已落子**的 `seatSeq`（`Reconnect.lastSeatSeq` 口径，§5.9 第 713 行）；
    /// 尚未落过子返回 `None`。
    pub fn last_seat_seq(&self, seat: u8) -> Option<u64> {
        self.seat_seq[seat as usize].checked_sub(1)
    }

    /// 当前最大 `roomSeq`（尚无事件时为 `None`）。
    pub fn latest_room_seq(&self) -> Option<u64> {
        self.next_room_seq.checked_sub(1)
    }

    /// 推进一条动作：枚举固定策略候选 → 落子 → 产出 [`ActionRecord`] 并压入缓冲。
    /// 已到 `settle` 相位时返回 `Ok(None)`，不再产事件。
    pub fn advance_one(
        &mut self,
        buffer: &mut ReconnectBuffer,
    ) -> Result<Option<ActionRecord>, EngineError> {
        if self.state.phase == PHASE_SETTLE {
            return Ok(None);
        }
        let action = first_action(&self.engine, &self.state);
        let seat = self.state.turn_seat;
        let record = ActionRecord {
            seat_seq: self.seat_seq[seat as usize],
            room_seq: self.next_room_seq,
            round_no: self.state.round_no,
            seat,
            request_id: self.next_request_id,
            action: action.clone(),
            // 占位：in-process 原型不接时钟（§3.3 约束 2），tsMs 用房间序号顶替。
            ts_ms: self.next_room_seq,
        };
        self.state = self.engine.apply(self.state.clone(), action)?;
        self.seat_seq[seat as usize] += 1;
        self.next_room_seq += 1;
        self.next_request_id += 1;
        buffer.push(record.clone());
        Ok(Some(record))
    }

    /// 推进至多 `max_steps` 条；中途到 `settle` 即停。返回实际推进条数。
    pub fn advance_steps(
        &mut self,
        buffer: &mut ReconnectBuffer,
        max_steps: usize,
    ) -> Result<usize, EngineError> {
        let mut done = 0;
        for _ in 0..max_steps {
            if self.advance_one(buffer)?.is_none() {
                break;
            }
            done += 1;
        }
        Ok(done)
    }

    /// 打到本局 `settle` 为止（固定策略）。返回本局剩余推进条数。
    pub fn run_to_settle(&mut self, buffer: &mut ReconnectBuffer) -> Result<usize, EngineError> {
        let mut done = 0;
        while self.state.phase != PHASE_SETTLE {
            self.advance_one(buffer)?;
            done += 1;
        }
        Ok(done)
    }

    /// 同一房间开第二局：`settle` 后重新 `deal`。
    ///
    /// 序号**不重置**（见 [`ServerRoom`] 上的 `M0-GAP-SEQ-CROSS-ROUND` 注释）。
    pub fn deal_next_round(&mut self) -> Result<(), EngineError> {
        assert_eq!(self.state.phase, PHASE_SETTLE, "只有 settle 局才能开下一局");
        self.state = self.engine.deal(M0_SEED)?;
        Ok(())
    }

    /// 构造服务端全量快照（in-process 克隆，不做 wire 编解码）。
    pub fn snapshot(&self, snapshot_reset: bool) -> RoomSnapshot {
        RoomSnapshot {
            state: self.state.clone(),
            snapshot_reset,
            room_seq: self.latest_room_seq().unwrap_or(0),
        }
    }
}
