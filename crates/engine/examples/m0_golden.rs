//! M0 golden 用例（native 端）：跑通 发牌 → 单张出牌 → score-only 结算，
//! 打印 32 字节全量 stateHash、16 字符呈现串与动作日志。
//!
//! 与 wasm 端（`gf_m0_golden_hash`）编译同一份 `cases::run_m0_golden`，
//! 双端逐字节比对（§2.4 gate 第 1 项、§5.12 第 840 行）。

use gf_engine::cases::{run_m0_golden, M0_SEED};
use gf_engine::hash::present_8;

fn main() {
    let outcome = run_m0_golden().expect("m0 golden 运行失败");
    let hex: String = outcome
        .state_hash
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    println!("seed: 0x{M0_SEED:016X}");
    println!("stateHash: {hex}");
    println!("present: {}", present_8(&outcome.state_hash));
    println!("steps: {}", outcome.actions.len());
    for (i, action) in outcome.actions.iter().enumerate() {
        println!(
            "{:03} seat={} kind={} cards={:?}",
            i + 1,
            action.seat,
            action.kind,
            action.cards
        );
    }
}
