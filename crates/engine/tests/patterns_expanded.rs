//! 牌型判定的「反向 / 轮子 / 混王」补测：每条只针对 `patterns.rs` 里一个特定判定
//! 条件，用最小手牌把它逼出来（合法侧与非法侧都要走一遍）。
//!
//! 与 `patterns.rs` 的分工：那边是全表正常 / 边界 / 非法三例；这里补的是容易被
//! 「正常用例顺带完成」掩盖的分支——轮子顺、同花顺花色不一致、三连对超点、
//! 三带二主牌四张、王炸混真牌、跟牌比较的两侧。

use gf_engine::cases::M0_SEED;
use gf_engine::contract::{Action, Engine, EngineError, State};
use gf_engine::engine::M0Engine;
use gf_engine::hash::LastPlay;
use gf_engine::patterns::card_id;
use gf_engine::ruleset::{from_yaml, RuleSetBody};
use std::fs;
use std::path::PathBuf;

fn fixture_body() -> RuleSetBody {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/guandan-classic/ruleset.yaml");
    from_yaml(&fs::read_to_string(p).expect("夹具读取失败")).expect("夹具解析失败")
}

fn engine() -> M0Engine {
    M0Engine::new(fixture_body())
}

fn c(r: &RuleSetBody, rank: usize, suit: usize, copy: u8) -> u16 {
    card_id(r, rank, suit, copy)
}

fn joker(r: &RuleSetBody, big: bool, copy: u8) -> u16 {
    gf_engine::patterns::joker_id(r, big, copy)
}

fn state_with(engine: &M0Engine, hand: Vec<u16>, last: Option<LastPlay>) -> State {
    let mut state = engine.deal(M0_SEED).expect("发牌失败");
    let idx = engine
        .ruleset
        .zones
        .iter()
        .position(|z| z.id.0 == "hand")
        .expect("hand 区") as u16;
    for z in state.zones.iter_mut() {
        if z.seat == 0 && z.zone_id == idx {
            z.cards = hand.clone();
        }
    }
    state.turn_seat = 0;
    state.last_play = last;
    state.passes_since_last_play = 0;
    state
}

/// 用「手牌 = 提交牌」造领出态并出牌，返回结果。
fn play_lead(engine: &M0Engine, cards: Vec<u16>) -> Result<State, EngineError> {
    let state = state_with(engine, cards.clone(), None);
    engine.apply(state, Action::Play { cards })
}

#[test]
fn wheel_straight_and_wheel_flush_are_legal() {
    let engine = engine();
    let r = &engine.ruleset;

    // 非花轮子顺 A-2-3-4-5：rankKey 恒取保留值 0。
    let wheel: Vec<u16> = vec![
        c(r, 0, 0, 0),  // 2
        c(r, 1, 1, 0),  // 3
        c(r, 2, 2, 0),  // 4
        c(r, 3, 3, 0),  // 5
        c(r, 12, 0, 0), // A
    ];
    let next = play_lead(&engine, wheel).expect("轮子顺应合法");
    let last = next.last_play.expect("应有桌面记录");
    assert_eq!(last.pattern_id, "straight-5");
    assert_eq!(last.pattern_key.1, 0, "非花轮子顺 rankKey 必须为 0");

    // 同花轮子顺：走同花顺，rankKey 按自然序顶牌（不走轮子特例）。
    let flush_wheel: Vec<u16> = vec![
        c(r, 0, 0, 0),
        c(r, 1, 0, 0),
        c(r, 2, 0, 0),
        c(r, 3, 0, 0),
        c(r, 12, 0, 0),
    ];
    let next = play_lead(&engine, flush_wheel).expect("同花轮子顺应合法");
    let last = next.last_play.expect("应有桌面记录");
    assert_eq!(last.pattern_id, "straight-flush-5");
    assert_ne!(last.pattern_key.1, 0, "同花顺不走轮子保留值");
}

#[test]
fn mixed_suit_straight_falls_back_to_plain_straight() {
    // 五张连续但花色混杂：同花顺判定必须放行，最终成普通顺子。
    let engine = engine();
    let r = &engine.ruleset;
    let cards = vec![
        c(r, 1, 0, 0),
        c(r, 2, 1, 0),
        c(r, 3, 2, 0),
        c(r, 4, 3, 0),
        c(r, 5, 0, 0),
    ];
    let next = play_lead(&engine, cards).expect("杂顺应合法");
    let last = next.last_play.expect("应有桌面记录");
    assert_eq!(last.pattern_id, "straight-5");
}

#[test]
fn joker_mixed_hands_are_rejected() {
    let engine = engine();
    let r = &engine.ruleset;

    // 王 + 真牌（对子牌型）→ 有王即不成型。
    let cards = vec![c(r, 3, 0, 0), joker(r, false, 0)];
    assert!(matches!(
        play_lead(&engine, cards),
        Err(EngineError::IllegalAction { .. })
    ));

    // 王炸混真牌：2 小 + 1 大 + 1 真 → 不是王炸。
    let cards = vec![
        joker(r, false, 0),
        joker(r, false, 1),
        joker(r, true, 0),
        c(r, 5, 0, 0),
    ];
    assert!(matches!(
        play_lead(&engine, cards),
        Err(EngineError::IllegalAction { .. })
    ));

    // 王炸混百搭：2 小 + 1 大 + 百搭 → 百搭不得代王。
    let cards = vec![
        joker(r, false, 0),
        joker(r, false, 1),
        joker(r, true, 0),
        c(r, 0, 1, 0),
    ];
    assert!(matches!(
        play_lead(&engine, cards),
        Err(EngineError::IllegalAction { .. })
    ));
}

#[test]
fn triple_pair_rejects_four_main_cards() {
    // 主牌四张（+ 一张杂牌）不是三带二。
    let engine = engine();
    let r = &engine.ruleset;
    let cards = vec![
        c(r, 4, 0, 0),
        c(r, 4, 1, 0),
        c(r, 4, 2, 0),
        c(r, 4, 3, 0),
        c(r, 7, 0, 0),
    ];
    assert!(matches!(
        play_lead(&engine, cards),
        Err(EngineError::IllegalAction { .. })
    ));
}

#[test]
fn same_joker_pair_is_pair_and_beats_ace_pair() {
    let engine = engine();
    let r = &engine.ruleset;
    let big = vec![joker(r, true, 0), joker(r, true, 1)];
    let small = vec![joker(r, false, 0), joker(r, false, 1)];

    // 领出：两张同王应识别为普通对子，而不是只能单张 / 王炸。
    let next = play_lead(&engine, big.clone()).expect("两大王应成对");
    assert_eq!(next.last_play.expect("桌面记录").pattern_id, "pair");

    // 造一对 A 压场，再由两大王 / 两小王跟牌；对王应高于对 A。
    let aces = vec![c(r, 12, 0, 0), c(r, 12, 1, 0)];
    let lead_state = state_with(&engine, aces.clone(), None);
    let lead_next = engine
        .apply(lead_state, Action::Play { cards: aces })
        .expect("对 A 应合法");
    let mut last = lead_next.last_play.expect("桌面记录");
    last.seat = 1;

    let state = state_with(&engine, big.clone(), Some(last.clone()));
    let legal = engine.legal_actions(&state).expect("合法动作");
    assert!(
        legal
            .iter()
            .any(|a| matches!(a, Action::Play { cards } if cards == &big)),
        "对 A 压场时，两大王对必须出现在合法动作里"
    );
    assert!(
        engine.apply(state, Action::Play { cards: big }).is_ok(),
        "两大王对必须压过对 A"
    );

    let state = state_with(&engine, small.clone(), Some(last));
    assert!(
        engine.apply(state, Action::Play { cards: small }).is_ok(),
        "两小王对也必须压过对 A"
    );
}

#[test]
fn follow_compare_handles_both_directions() {
    let engine = engine();
    let r = &engine.ruleset;

    // 先用真实出牌造「5 的对子」桌面记录（3 是邻位，2 是级牌会被提升，故取 5 为基准）。
    let lead_cards = vec![c(r, 3, 0, 0), c(r, 3, 1, 0)];
    let lead_state = state_with(&engine, lead_cards.clone(), None);
    let lead_next = engine
        .apply(lead_state, Action::Play { cards: lead_cards })
        .expect("5 的对子应合法");
    let last = lead_next.last_play.expect("桌面记录");

    // 跟 3 的对子（rank 1）→ 压不过。
    let lower = vec![c(r, 1, 0, 0), c(r, 1, 1, 0)];
    let state = state_with(&engine, lower.clone(), Some(last.clone()));
    assert!(
        matches!(
            engine.apply(state, Action::Play { cards: lower }),
            Err(EngineError::IllegalAction { .. })
        ),
        "小对子压不过大对子"
    );

    // 跟 6 的对子（rank 4）→ 压得过。
    let higher = vec![c(r, 4, 0, 0), c(r, 4, 1, 0)];
    let state = state_with(&engine, higher.clone(), Some(last));
    let next = engine
        .apply(state, Action::Play { cards: higher })
        .expect("大对子应压过");
    assert_eq!(next.last_play.expect("桌面记录").seat, 0);
}
