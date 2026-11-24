//! M0 golden 确定性回归：双端比对锚点、投影稳定性、动作敏感性。
//!
//! 期望哈希先由 native 用例跑出、确认动作日志合法后再写死（不许先写期望再凑实现）。

use gf_engine::cases::{build_engine, run_m0_golden, M0_SEED};
use gf_engine::contract::{Action, Engine};
use gf_engine::engine::PHASE_SETTLE;
use gf_engine::hash::{canonical_bytes, state_hash, CanonicalState};

/// native 用例跑出并核对过动作日志后的 32 字节 stateHash（双端逐字节比对锚点）。
const EXPECTED_STATE_HASH: &str =
    "3e562fe289db6fb0ce59aa20d4ad47b6abfc23c721b9c181aa2f94290e90085a";

#[test]
fn golden_reaches_settle_and_matches_recorded_hash() {
    let outcome = run_m0_golden().expect("m0 golden 运行失败");
    assert_eq!(
        outcome.state.phase,
        PHASE_SETTLE,
        "golden 必须跑到 settle；finish_order={:?}, actions={}",
        outcome.state.finish_order,
        outcome.actions.len()
    );
    let hex: String = outcome
        .state_hash
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hex, EXPECTED_STATE_HASH, "native 用例哈希与记录锚点不一致");
}

#[test]
fn projection_is_stable_for_same_state() {
    let outcome = run_m0_golden().expect("m0 golden 运行失败");
    let c1 = CanonicalState::from(&outcome.state);
    let c2 = CanonicalState::from(&outcome.state);
    // 同一个 State 两次投影：canonical 字节与哈希都必须稳定。
    assert_eq!(canonical_bytes(&c1), canonical_bytes(&c2));
    assert_eq!(state_hash(&c1), state_hash(&c2));
}

#[test]
fn different_action_changes_state_hash() {
    let engine = build_engine().expect("构造引擎失败");
    let s0 = engine.deal(M0_SEED).expect("发牌失败");
    let legal = engine.legal_actions(&s0).expect("枚举合法动作失败");
    let plays: Vec<&Action> = legal
        .iter()
        .filter(|a| matches!(a, Action::Play { .. }))
        .collect();
    assert!(plays.len() >= 2, "领出阶段至少应有两张合法单张");

    let s1 = engine
        .apply(s0.clone(), plays[0].clone())
        .expect("落子失败");
    let s2 = engine.apply(s0, plays[1].clone()).expect("落子失败");
    let h1 = state_hash(&CanonicalState::from(&s1));
    let h2 = state_hash(&CanonicalState::from(&s2));
    assert_ne!(h1, h2, "换一张出牌，stateHash 必须变化");
}
