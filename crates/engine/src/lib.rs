//! gf-engine：掼蛋多端框架 M0 引擎契约层。
//!
//! 依据 `docs/designs/2026-11-23-guandan-multiplatform/design_draft.md`：
//! - §2.4 M0 落地清单第 2 项：引擎最小对外接口（trait），签名在 M0 锁定，M1 只补原语实现；
//! - §5.4 校验四级与 `Validation` / `Finding` / `EngineError`；
//! - §5.12 `stateHash` / canonical view 编码。
//!
//! 确定性纪律见 §3.3：判定链路禁用浮点、`HashMap` 迭代顺序依赖、`SystemTime::now()`、
//! `rand::thread_rng()`、文件 I/O 与网络 I/O。

pub mod cases;
pub mod contract;
pub mod engine;
pub mod hash;
pub mod patterns;
pub mod recommend;
mod recommend_decompose;
mod recommend_key;
pub mod ruleset;
pub mod settlement;
pub mod view;
