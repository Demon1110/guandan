//! `gf-runtime-spike`：M0 断线恢复原型（`design_draft.md` §2.4 第 194 行 gate 判据）。
//!
//! **迁移状态（2026-11-25）**：重连缓冲 / 判定 / 增量重放 / 快照重置 / 哈希自证
//! 已整体迁入 `crates/table-runtime`（`recovery` 模块 + `room` 模块的真实房间），
//! 原 `recovery.rs` 与端到端 `tests/reconnect.rs` 已删除，不复留副本。
//! 本 crate 只剩 [`harness`]：一次性造局夹具（§2.4 第 186 行：压测脚本与一次性夹具不迁入）。
//!
//! 迁移后的等价覆盖在 `crates/table-runtime/tests/room_flow.rs`：
//! 增量重放对哈希、缓冲淘汰落快照重置、wire 帧 `viewHash` 自证。

pub mod harness;
