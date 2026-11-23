//! gf-protocol：牌桌协议层骨架（M0 本轮范围不含协议实现）。
//!
//! 依据 design_draft.md §5.9「牌桌协议（`crates/protocol`）」：
//! - M0 要交付的消息表：`JoinRoom` / `JoinAck` / `Reconnect` / `SubmitAction` /
//!   `Heartbeat` / `RoomSnapshot` / `RoomDelta` / `ActionAck` / `PhaseChange` /
//!   `SettleResult` / `TrusteeState`；
//! - proto 文件固定路径：`crates/protocol/proto/gf/v1.proto`；
//! - proto package：`gf.protocol.v1`；
//! - 服务端与 wasm 客户端共用同一份生成代码。
//!
//! 本轮（M0 最短可验证链路）**不引入 protobuf 代码生成**：本 crate 只出骨架，
//! 消息字段与 `oneof` 分型在后续 M0 迭代按 §5.9 表落到上述 `.proto` 文件。
//!
//! 注意：`RoomSnapshot.view` 的类型是 `bytes`，内容为 canonical JSON（编码规则见 §5.12），
//! 协议层只声明 `bytes` 与 `viewSchemaVersion: u16` 两个字段，不做静态 schema。
