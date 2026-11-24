//! M0 golden 用例（native 端）：跑通 发牌 → 单张出牌 → score-only 结算，
//! 打印 32 字节全量 stateHash、终局诊断字段与动作日志。
//!
//! 与 wasm 端（`gf_m0_golden_report`）编译同一份 `cases::run_m0_golden`，
//! 双端逐字段比对（§2.4 gate 第 1 项、§5.12 第 840 行）。

use gf_engine::cases::{run_m0_golden, M0_SEED};
use gf_engine::hash::present_8;

fn main() {
    let outcome = run_m0_golden().expect("m0 golden 运行失败");
    let hex: String = outcome
        .state_hash
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let finish_order: Vec<String> = outcome.finish_order.iter().map(|s| s.to_string()).collect();
    let terminal_team = outcome
        .terminal_team
        .map(|t| t.to_string())
        .unwrap_or_else(|| "-".to_owned());

    println!("seed: 0x{M0_SEED:016X}");
    println!("stateHash: {hex}");
    println!("present: {}", present_8(&outcome.state_hash));
    println!("steps: {}", outcome.steps);
    println!("phase: {}", outcome.phase);
    println!("finishOrder: [{}]", finish_order.join(","));
    println!("terminalTeam: {terminal_team}");
    println!("discardCount: {}", outcome.discard_count);
    println!("handsTotal: {}", outcome.hands_total);
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
