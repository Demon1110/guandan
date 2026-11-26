//! gf-protocol：牌桌协议消息层（§5.9）。
//!
//! - proto 固定路径 `crates/protocol/proto/gf/v1.proto`，package `gf.protocol.v1`；
//! - 生成代码由 `build.rs`（prost-build，0.13）写入 `src/generated/` 并随仓库跟踪，
//!   服务端与 wasm 客户端共用这同一份生成字节（§2.4 第 184 行）；
//! - 类型入口：`gf_protocol::gf::protocol::v1::*`。
//!
//! 纪律：`crates/engine` 不依赖本 crate——引擎不该知道网络形状。
//!
//! 传输层：消息体之外的 WS 帧头见 [`wire`]（`[kind: u16 大端][protobuf body]`），
//! 服务端与客户端共用这一份编解码。

// 生成代码不在此手工改动；prost 输出的 clippy 告警统一在本层级 suppress，
// 不回改生成内容。
#[allow(clippy::all)]
pub mod gf {
    pub mod protocol {
        pub mod v1 {
            include!("generated/gf.protocol.v1.rs");
        }
    }
}

pub mod wire;
