//! 组牌推荐同解探针（native 端）：classic 领出态 seat 0 跑 `recommend`，打印
//! `Layout` 摘要与确定性摘要 `recommendDigest`。与 wasm 端 `gf_recommend_digest`
//! 编译同一份 `cases::recommend_digest`，双端逐值比对（§5.11、§10.4）。
//!
//! 独立探针：不改既有 64 字节 golden ABI，也不加字段到旧报告。

use gf_engine::cases::{classic_lead_state, recommend_digest};
use gf_engine::contract::{Engine, Seat};

fn main() {
    let (engine, state) = classic_lead_state();
    let layout = engine
        .recommend(&state, Seat(0))
        .expect("classic recommend 不应报错");
    let digest = recommend_digest();

    println!("recommendDigest: {digest}");
    println!("score: {}", layout.score);
    println!("singleCount: {}", layout.single_count);
    println!("groups: {}", layout.groups.len());
    for g in &layout.groups {
        println!(
            "group: {} key=({},{}) cards={:?}",
            g.pattern_id, g.pattern_key.0, g.pattern_key.1, g.cards
        );
    }
    println!(
        "straightFlushCandidates: {}",
        layout.straight_flush_candidates.len()
    );
    println!("hasStraightFlush: {}", layout.has_straight_flush);
    println!("disabled: {}", layout.disabled);
}
