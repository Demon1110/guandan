//! `gf-table-runtime`：房间层（M1）。
//!
//! 三块：
//! - [`snapshot`]：`RoomSnapshot` wire 编解码与服务端 `viewHash` 自证（§5.9 / §5.12）；
//! - [`recovery`]：重连环形缓冲、`seatSeq → roomSeq` 换算、增量重放与快照重置
//!   （由 `crates/runtime-spike` 迁入，§2.4 转正判据）；
//! - [`room`]：建桌 → 发牌 → 行动窗口（计时 + 托管）→ 出牌 → 结算 → 再来一局，
//!   外加回放录制与断线恢复。
//!
//! 本层不含网络、协议帧与账号：只保证一桌纯本地 bot 局可无人值守跑完并续局
//! （`docs/开发计划.md` §2 M1 硬 gate）。

pub mod recovery;
pub mod room;
pub mod snapshot;

pub use recovery::{
    decide, replay_delta, state_hash_of, ActionRecord, ClientLocal, ReconnectBuffer,
    ReconnectDecision, RecoveryOutcome, RoomSnapshotState, RECONNECT_BUFFER_CAPACITY,
};
pub use room::{
    normalize_action, replay, AutoAction, AutoReason, BotPolicy, PlayerKind, PlayerStatus,
    ReconnectPlan, Room, RoomConfig, RoomError, SettleSummary, StepOutcome,
    DEFAULT_OFFLINE_ALL_MS, DEFAULT_GRACE_MS, DEFAULT_RULESET_VERSION, DEFAULT_TURN_MS,
};
pub use snapshot::{CodecError, RoomSnapshotWire};
