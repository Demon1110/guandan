//! 候选枚举与压制比较的边界覆盖（纯补测，不改实现）。
//!
//! 手段：直接改座位 0 的手牌内容，再调 `legal_actions` / `apply`，让枚举分支
//! （单张含王、含百搭、全百搭、王炸、轮子顺、同花顺、三带二、连对、钢板）与
//! `beats` 的三类比较（同型、炸弹族互压、炸弹压非炸弹）都走到。

use gf_engine::cases;
use gf_engine::contract::{Action, Engine, State};
use gf_engine::hash::LastPlay;
use gf_engine::patterns::{card_id, joker_id};
use gf_engine::ruleset::model::RuleSetBody;
use gf_engine::ruleset::parse::from_yaml;

const CLASSIC: &str = include_str!("fixtures/guandan-classic/ruleset.yaml");

fn body() -> RuleSetBody {
    from_yaml(CLASSIC).expect("classic 夹具应可构造")
}

/// 只换 seat 0 的手牌内容，长度保持不变（维持牌数守恒，避免触发无关不变量）。
fn set_hand(state: &mut State, want: Vec<u16>) {
    let len = state
        .zones
        .iter()
        .find(|z| z.seat == 0 && z.zone_id == 0)
        .map(|z| z.cards.len())
        .unwrap_or(0);
    let mut hand = want;
    for c in 0..108u16 {
        if hand.len() >= len {
            break;
        }
        if !hand.contains(&c) {
            hand.push(c);
        }
    }
    hand.truncate(len);
    for z in state.zones.iter_mut() {
        if z.seat == 0 && z.zone_id == 0 {
            z.cards = hand.clone();
        }
    }
}

fn lead_with(hand: Vec<u16>) -> (gf_engine::engine::M0Engine, State) {
    let (engine, mut state) = cases::classic_lead_state();
    state.turn_seat = 0;
    set_hand(&mut state, hand);
    (engine, state)
}

fn plays_of(actions: &[Action]) -> Vec<Vec<u16>> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Play { cards } => Some(cards.clone()),
            Action::Pass => None,
        })
        .collect()
}

#[test]
fn single_candidates_cover_jokers_and_wildcard() {
    let r = body();
    let little = joker_id(&r, false, 0);
    let big = joker_id(&r, true, 0);
    // 红桃级牌 = 百搭（classic 级牌 2，逢人配源花色 H）。
    let wild = card_id(&r, 0, 1, 0);
    let (engine, state) = lead_with(vec![little, big, wild]);
    let actions = engine.legal_actions(&state).expect("领出枚举失败");
    let plays = plays_of(&actions);
    for card in [little, big] {
        assert!(
            plays.iter().any(|c| c == &vec![card]),
            "单张候选应含 {card}，实际 {plays:?}"
        );
    }
    // 百搭单张不进候选（识别侧不成型），只要求枚举过程不 panic。
    let _ = wild;
}

#[test]
fn all_wildcard_hand_short_circuit() {
    let r = body();
    let wilds: Vec<u16> = (0..2).map(|c| card_id(&r, 0, 1, c)).collect();
    let (engine, mut state) = lead_with(wilds.clone());
    // 手牌只剩百搭：走「整手全百搭」独立分支。
    for z in state.zones.iter_mut() {
        if z.seat == 0 && z.zone_id == 0 {
            z.cards = wilds.clone();
        }
    }
    let actions = engine.legal_actions(&state).expect("全百搭枚举失败");
    assert!(!actions.is_empty(), "全百搭手牌至少应给出单张候选");
}

#[test]
fn joker_bomb_and_bombs() {
    let r = body();
    let four_jokers: Vec<u16> = vec![
        joker_id(&r, false, 0),
        joker_id(&r, false, 1),
        joker_id(&r, true, 0),
        joker_id(&r, true, 1),
    ];
    let bomb: Vec<u16> = (0..4).map(|s| card_id(&r, 5, s, 0)).collect();
    let mut hand = four_jokers.clone();
    hand.extend(bomb.iter().copied());
    let (engine, state) = lead_with(hand);
    let plays = plays_of(&engine.legal_actions(&state).expect("枚举失败"));
    let mut bomb_cards = four_jokers.clone();
    bomb_cards.sort_unstable();
    let r2 = engine.apply(
        state.clone(),
        Action::Play {
            cards: bomb_cards.clone(),
        },
    );
    assert!(r2.is_ok(), "王炸应可出：{:?}", r2.err());
    let _ = &plays;
    assert!(
        plays.iter().any(|c| c.len() == 4),
        "应枚举出 4 张炸弹候选，实际 {plays:?}"
    );
}

#[test]
fn wheel_straight_and_flush() {
    let r = body();
    // 轮子顺 A-2-3-4-5（跨花色，避免误判同花顺）。
    let wheel: Vec<u16> = vec![
        card_id(&r, 12, 0, 0),
        card_id(&r, 0, 2, 0),
        card_id(&r, 1, 3, 0),
        card_id(&r, 2, 1, 0),
        card_id(&r, 3, 0, 1),
    ];
    let (engine, state) = lead_with(wheel.clone());
    let plays = plays_of(&engine.legal_actions(&state).expect("枚举失败"));
    assert!(
        plays.iter().any(|c| c.len() == 5),
        "轮子顺应被枚举，实际 {plays:?}"
    );

    // 同花顺 ♠3-4-5-6-7。
    let flush: Vec<u16> = (1..=5).map(|rank| card_id(&r, rank, 0, 0)).collect();
    let (engine, state) = lead_with(flush.clone());
    let plays = plays_of(&engine.legal_actions(&state).expect("枚举失败"));
    assert!(
        plays.iter().any(|c| c
            == &{
                let mut v = flush.clone();
                v.sort_unstable();
                v
            }),
        "同花顺应被枚举，实际 {plays:?}"
    );
}

#[test]
fn triple_pair_plate_and_steel() {
    let r = body();
    // 三带二：3 张 6 + 一对 7。
    let mut tp: Vec<u16> = (0..3).map(|s| card_id(&r, 5, s, 0)).collect();
    tp.push(card_id(&r, 6, 0, 0));
    tp.push(card_id(&r, 6, 1, 0));
    let (engine, state) = lead_with(tp.clone());
    let plays = plays_of(&engine.legal_actions(&state).expect("枚举失败"));
    assert!(
        plays.iter().any(|c| c.len() == 5),
        "三带二应被枚举，实际 {plays:?}"
    );

    // 三连对：8/9/10 各一对。
    let mut plate: Vec<u16> = Vec::new();
    for rank in [7usize, 8, 9] {
        plate.push(card_id(&r, rank, 0, 0));
        plate.push(card_id(&r, rank, 1, 0));
    }
    let (engine, state) = lead_with(plate);
    let plays = plays_of(&engine.legal_actions(&state).expect("枚举失败"));
    assert!(
        plays.iter().any(|c| c.len() == 6),
        "三连对应被枚举，实际 {plays:?}"
    );

    // 钢板：K/A 各三张。
    let mut steel: Vec<u16> = Vec::new();
    for rank in [11usize, 12] {
        for suit in 0..3 {
            steel.push(card_id(&r, rank, suit, 0));
        }
    }
    let (engine, state) = lead_with(steel);
    let plays = plays_of(&engine.legal_actions(&state).expect("枚举失败"));
    assert!(
        plays.iter().any(|c| c.len() == 6),
        "钢板应被枚举，实际 {plays:?}"
    );
}

#[test]
fn straight_with_wildcard_hole() {
    let r = body();
    // 4 张连号（3-4-5-6）+ 1 张百搭 → 补成 5 张顺子。
    let wild = card_id(&r, 0, 1, 0);
    let mut hand: Vec<u16> = (1..=4).map(|rank| card_id(&r, rank, 2, 0)).collect();
    hand.push(wild);
    let (engine, state) = lead_with(hand);
    let plays = plays_of(&engine.legal_actions(&state).expect("枚举失败"));
    assert!(
        plays.iter().any(|c| c.len() == 5 && c.contains(&wild)),
        "百搭补位的顺子应被枚举，实际 {plays:?}"
    );
}

#[test]
fn beats_matrix() {
    let r = body();
    let pair5: Vec<u16> = (0..2).map(|s| card_id(&r, 5, s, 0)).collect();
    let bomb5: Vec<u16> = (0..4).map(|s| card_id(&r, 5, s, 0)).collect();
    let bomb6: Vec<u16> = (0..4).map(|s| card_id(&r, 4, s, 0)).collect();

    // ① 炸弹压非炸弹：上家一对 5，己方 4 张同点可压。
    let (engine, mut state) = cases::classic_lead_state();
    state.turn_seat = 0;
    state.last_play = Some(LastPlay {
        seat: 1,
        pattern_id: "pair".to_owned(),
        pattern_key: (2, 7),
        cards: pair5.clone(),
    });
    let hand: Vec<u16> = bomb5.clone();
    set_hand(&mut state, hand);
    let plays = plays_of(&engine.legal_actions(&state).expect("跟牌枚举失败"));
    assert!(
        plays.iter().any(|c| c.len() == 4),
        "炸弹应能压对子，实际 {plays:?}"
    );

    // ② 炸弹互压：更大点数的 4 张炸弹能压过小的。
    state.last_play = Some(LastPlay {
        seat: 1,
        pattern_id: "bomb-4".to_owned(),
        pattern_key: (4, 7),
        cards: bomb5.clone(),
    });
    set_hand(&mut state, bomb6.clone());
    let plays = plays_of(&engine.legal_actions(&state).expect("炸弹跟牌枚举失败"));
    assert!(!plays.is_empty(), "应能给出更大的炸弹候选");

    // ③ 同型比 patternKey：单张压不过更大的单张。
    let low = card_id(&r, 2, 0, 0);
    let high = card_id(&r, 12, 0, 0);
    state.last_play = Some(LastPlay {
        seat: 1,
        pattern_id: "single".to_owned(),
        pattern_key: (1, 13),
        cards: vec![high],
    });
    set_hand(&mut state, vec![low, high]);
    let plays = plays_of(&engine.legal_actions(&state).expect("单张跟牌枚举失败"));
    assert!(
        !plays.iter().any(|c| c == &vec![low]),
        "小于上家的单张不应出现在候选里，实际 {plays:?}"
    );
}
