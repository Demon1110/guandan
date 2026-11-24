//! `gf-runtime-spike`：M0 断线恢复原型（`design_draft.md` §2.4 第 194 行 gate 判据）。
//!
//! 分层约定（§2.4 第 186 行转正判据）：
//! - [`recovery`]：将来要进入 §10.1 契约层双产物比对的逻辑，**待迁入 `crates/table-runtime`**；
//! - [`harness`]：一次性夹具与模拟脚本，**不迁入**，M1 可直接丢弃。
//!
//! `crates/table-runtime` 在 M0 并不存在（迁入是 M1 的事），本 crate 不铺开运行期框架。

pub mod harness;
pub mod recovery;
