//! M1 牌型判定全表 + 黄金用例（§5.2 第 513–519 行、§5.10 第 775 行、§5.13 第 890–895 行）。
//!
//! 夹具：`guandan-classic`（2 副 108 张，`copies=2`，花色 S/H/D/C，点数
//! 2..A 下标 0..12，小王 13 / 大王 14）。牌 id 几何：
//! `id = (suit × 13 + rank) × 2 + copy`；王在序尾 104–107。
//!
//! 每个牌型至少正常 / 边界 / 非法三例；合法例断言 `pattern_id` 与 `pattern_key`
//! 字面值，非法例断言 `EngineError::IllegalAction`。

use gf_engine::cases::M0_SEED;
use gf_engine::contract::{Action, Engine, EngineError, State};
use gf_engine::engine::M0Engine;
use gf_engine::hash::LastPlay;
use gf_engine::patterns::{card_id, joker_id};
use gf_engine::ruleset::{from_yaml, RuleSetBody};
use std::fs;
use std::path::PathBuf;

fn fixture_body() -> RuleSetBody {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/guandan-classic/ruleset.yaml");
    let src = fs::read_to_string(p).expect("夹具读取失败");
    from_yaml(&src).expect("guandan-classic 应构造成功")
}

fn engine() -> M0Engine {
    M0Engine::new(fixture_body())
}

/// 变体夹具：调整 `deck.copies`（凑齐多张百搭），并同步 `perSeat` / `cardCount`
/// 保持发牌守恒；`strip_bombs` 去掉 bomb-5..8 行，用于「未声明该张数炸弹」反例。
/// 仅构造期用，不走 L1 值域校验。
fn engine_with_copies(copies: u8, strip_bombs: bool) -> M0Engine {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/guandan-classic/ruleset.yaml");
    let src = fs::read_to_string(p).expect("夹具读取失败");
    let mut lines: Vec<&str> = src.lines().collect();
    if strip_bombs {
        lines.retain(|l| !(l.contains("pattern.bomb") && !l.contains("bomb-4")));
    }
    let total = (13 * 4 + 2) * copies as usize;
    let per_seat = total / 4;
    let src = lines
        .join("\n")
        .replace("copies: 2", &format!("copies: {copies}"))
        .replace("perSeat: 27", &format!("perSeat: {per_seat}"))
        .replace("cardCount: 108", &format!("cardCount: {total}"));
    let body = from_yaml(&src).expect("变体夹具应构造成功");
    M0Engine::new(body)
}

/// 基础牌便捷构造：`rank` 0..12、`suit` 0..3、`copy` 0/1。
fn c(r: &RuleSetBody, rank: usize, suit: usize, copy: u8) -> u16 {
    card_id(r, rank, suit, copy)
}

/// 造 seat 0 受控手牌的领出态（其余座位保留发牌结果，不影响判定）。
fn state_with(engine: &M0Engine, hand: Vec<u16>) -> State {
    let mut state = engine.deal(M0_SEED).expect("发牌失败");
    let hand_idx = engine
        .ruleset
        .zones
        .iter()
        .position(|z| z.id.0 == "hand")
        .unwrap() as u16;
    let zone = state
        .zones
        .iter_mut()
        .find(|z| z.seat == 0 && z.zone_id == hand_idx)
        .expect("seat 0 手牌区");
    zone.cards = hand;
    state.turn_seat = 0;
    state.last_play = None;
    state
}

/// 跟牌态：上家 `last_id` / `last_key` 已压桌。
fn follow_state(
    engine: &M0Engine,
    hand: Vec<u16>,
    last_id: &str,
    last_key: (u32, u32),
    last_cards: Vec<u16>,
) -> State {
    let mut state = state_with(engine, hand);
    state.last_play = Some(LastPlay {
        seat: 1,
        pattern_id: last_id.to_owned(),
        pattern_key: last_key,
        cards: last_cards,
    });
    state
}

fn apply_play(engine: &M0Engine, hand: Vec<u16>, cards: Vec<u16>) -> Result<State, EngineError> {
    let state = state_with(engine, hand);
    engine.apply(state, Action::Play { cards })
}

fn assert_illegal(result: Result<State, EngineError>, what: &str) {
    match result {
        Err(EngineError::IllegalAction { .. }) => {}
        other => panic!("{what} 应判 IllegalAction，实际 {other:?}"),
    }
}

fn assert_played(
    result: Result<State, EngineError>,
    expected_id: &str,
    expected_key: (u32, u32),
    what: &str,
) -> State {
    let state = result.unwrap_or_else(|e| panic!("{what} 应落子成功: {e:?}"));
    let last = state
        .last_play
        .as_ref()
        .unwrap_or_else(|| panic!("{what} 应有 last_play"));
    assert_eq!(last.pattern_id, expected_id, "{what}: pattern_id 不符");
    assert_eq!(last.pattern_key, expected_key, "{what}: pattern_key 不符");
    state
}

// ---------- 单张 / 对子 / 三张 ----------

#[test]
fn single_recognized() {
    let r = fixture_body();
    // 2♠ copy 0；级牌 2 在 above-A 合成序中位于 A 后，rankKey 13。
    let card = c(&r, 0, 0, 0);
    let engine = engine();
    assert_played(
        apply_play(&engine, vec![card], vec![card]),
        "single",
        (1, 13),
        "单张 2",
    );
}

#[test]
fn single_joker_recognized() {
    let engine = engine();
    let little = joker_id(&engine.ruleset, false, 0);
    let big = joker_id(&engine.ruleset, true, 0);
    // 小王 rankKey 14、大王 15（rankKey 整体 +1，王占序尾）。
    assert_played(
        apply_play(&engine, vec![little], vec![little]),
        "single",
        (1, 14),
        "小王单张",
    );
    assert_played(
        apply_play(&engine, vec![big], vec![big]),
        "single",
        (1, 15),
        "大王单张",
    );
}

#[test]
fn pair_recognized() {
    let r = fixture_body();
    // 2♠ + 2♦ 同点数 2。
    let cards = vec![c(&r, 0, 0, 0), c(&r, 0, 2, 0)];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "pair",
        (2, 13),
        "对子 2",
    );
}

#[test]
fn triple_recognized_boundary_three() {
    let r = fixture_body();
    // 2♠×2 + 2♦×1 共三张同点 2。
    let cards = vec![c(&r, 0, 0, 0), c(&r, 0, 0, 1), c(&r, 0, 2, 0)];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "triple",
        (3, 13),
        "三张 2（边界 3 张）",
    );
}

#[test]
fn pair_rejects_two_different_ranks() {
    let r = fixture_body();
    let cards = vec![c(&r, 0, 0, 0), c(&r, 1, 0, 0)];
    let engine = engine();
    assert_illegal(
        apply_play(&engine, cards.clone(), cards),
        "2♠3♠ 不同点数当对子",
    );
}

#[test]
fn triple_rejects_mixed_ranks() {
    let r = fixture_body();
    let cards = vec![c(&r, 0, 0, 0), c(&r, 0, 0, 1), c(&r, 1, 0, 0)];
    let engine = engine();
    assert_illegal(
        apply_play(&engine, cards.clone(), cards),
        "两张 2 加一张 3 当三张",
    );
}

// ---------- 三带二（triple-pair） ----------

#[test]
fn triple_pair_recognized() {
    let r = fixture_body();
    // 主牌三张 3 + 对子 5；rankKey 只取主牌 3（合成序 key 1），对子不提升。
    let cards = vec![
        c(&r, 1, 0, 0),
        c(&r, 1, 0, 1),
        c(&r, 1, 2, 0),
        c(&r, 3, 0, 0),
        c(&r, 3, 2, 0),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "triple-pair",
        (5, 1),
        "三带二 333+55",
    );
}

#[test]
fn triple_pair_rank_key_ignores_pair() {
    let r = fixture_body();
    // 主牌三张 A + 对子 2：rankKey 仍取 A（key 12），带级牌对子不抬高。
    let cards = vec![
        c(&r, 12, 0, 0),
        c(&r, 12, 0, 1),
        c(&r, 12, 2, 0),
        c(&r, 0, 0, 0),
        c(&r, 0, 2, 0),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "triple-pair",
        (5, 12),
        "三带二 AAA+22 对子不进 rankKey",
    );
}

#[test]
fn triple_pair_accepts_joker_pair() {
    let r = fixture_body();
    // 主牌三张 3 + 两张小王成对亦可。
    let cards = vec![
        c(&r, 1, 0, 0),
        c(&r, 1, 0, 1),
        c(&r, 1, 2, 0),
        joker_id(&r, false, 0),
        joker_id(&r, false, 1),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "triple-pair",
        (5, 1),
        "三带二 333+小王对",
    );
}

#[test]
fn triple_pair_wildcard_fills_main() {
    let r = fixture_body();
    // 真牌两张 3 + 百搭补主牌三张，对子为小王对（主牌点数无歧义，不取最大解分支）。
    let cards = vec![
        c(&r, 1, 0, 0),
        c(&r, 1, 2, 0),
        w(&r, 0),
        joker_id(&r, false, 0),
        joker_id(&r, false, 1),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "triple-pair",
        (5, 1),
        "百搭补主牌的三带二",
    );
}

#[test]
fn triple_pair_wildcard_fills_pair() {
    let r = fixture_body();
    // 主牌三张 3 + 真牌一张 5 + 百搭补对子 55。
    let cards = vec![
        c(&r, 1, 0, 0),
        c(&r, 1, 0, 1),
        c(&r, 1, 2, 0),
        c(&r, 3, 0, 0),
        w(&r, 0),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "triple-pair",
        (5, 1),
        "百搭补对子的三带二",
    );
}

#[test]
fn triple_pair_rejects_main_jokers() {
    let r = fixture_body();
    // 主牌三张不得是王：两小王 + 大王 + 一对 2 不构成三带二。
    let cards = vec![
        joker_id(&r, false, 0),
        joker_id(&r, false, 1),
        joker_id(&r, true, 0),
        c(&r, 0, 0, 0),
        c(&r, 0, 2, 0),
    ];
    let engine = engine();
    assert_illegal(
        apply_play(&engine, cards.clone(), cards),
        "王不能当三带二主牌",
    );
}

#[test]
fn triple_pair_plain_cannot_beat_plain() {
    let r = fixture_body();
    let engine = engine();
    let last = vec![
        c(&r, 1, 0, 0),
        c(&r, 1, 0, 1),
        c(&r, 1, 2, 0),
        c(&r, 3, 0, 0),
        c(&r, 3, 2, 0),
    ];
    // 另一手普通顺子：非炸弹跨型不可压三带二。
    let hand = vec![
        c(&r, 1, 0, 0),
        c(&r, 2, 1, 0),
        c(&r, 3, 2, 0),
        c(&r, 4, 3, 0),
        c(&r, 5, 0, 0),
    ];
    let state = follow_state(&engine, hand, "triple-pair", (5, 1), last);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        !legal.iter().any(|a| matches!(a, Action::Play { .. })),
        "非炸弹跨型不可压三带二: {legal:?}"
    );
}

#[test]
fn bomb_beats_triple_pair() {
    let r = fixture_body();
    let engine = engine();
    let last = vec![
        c(&r, 1, 0, 0),
        c(&r, 1, 0, 1),
        c(&r, 1, 2, 0),
        c(&r, 3, 0, 0),
        c(&r, 3, 2, 0),
    ];
    let bomb2 = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 0, 1),
        c(&r, 0, 2, 0),
        c(&r, 0, 2, 1),
    ];
    let state = follow_state(&engine, bomb2.clone(), "triple-pair", (5, 1), last);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(plays_contain(&legal, &bomb2), "炸弹应能压三带二: {legal:?}");
}

// ---------- 顺子 ----------

#[test]
fn straight_23456_recognized() {
    let r = fixture_body();
    // 2-3-4-5-6 杂花色（同花情形由同花顺用例覆盖），高点 6（下标 4）。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 1, 1, 0),
        c(&r, 2, 2, 0),
        c(&r, 3, 3, 0),
        c(&r, 4, 0, 0),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "straight-5",
        (5, 5),
        "顺子 2-3-4-5-6",
    );
}

#[test]
fn straight_wheel_a2345_is_legal_boundary() {
    let r = fixture_body();
    // A-2-3-4-5：唯一环形特例合法（§5.13 第 891 行）；非同花轮子 rankKey 恒取保留值 0。
    let cards = vec![
        c(&r, 12, 0, 0),
        c(&r, 0, 0, 0),
        c(&r, 1, 2, 0),
        c(&r, 2, 3, 0),
        c(&r, 3, 1, 0),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "straight-5",
        (5, 0),
        "轮子顺 A-2-3-4-5",
    );
}

#[test]
fn wheel_filled_by_wildcard_still_zero_rank_key() {
    let r = fixture_body();
    // A-2-3-4 + 百搭补 5：非同花轮子，含百搭仍恒取保留值 0。
    let cards = vec![
        c(&r, 12, 0, 0),
        c(&r, 0, 0, 0),
        c(&r, 1, 2, 0),
        c(&r, 2, 3, 0),
        w(&r, 0),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "straight-5",
        (5, 0),
        "百搭补轮子仍 rankKey 0",
    );
}

#[test]
fn flush_wheel_follows_top_not_zero() {
    let r = fixture_body();
    // 同花 A-2-3-4-5：轮子特例只豁免非同花；同花顺跟随顶牌 5，按自然序 key 4。
    let cards = vec![
        c(&r, 12, 0, 0),
        c(&r, 0, 0, 0),
        c(&r, 1, 0, 0),
        c(&r, 2, 0, 0),
        c(&r, 3, 0, 0),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "straight-flush-5",
        (5, 4),
        "同花顺轮子不取 0",
    );
}

#[test]
fn level_single_beats_a() {
    let r = fixture_body();
    let engine = engine();
    // 级牌 2 above-A：单张 2（key 13）可压单张 A（key 12）。
    let hand = vec![c(&r, 0, 0, 0)];
    let state = follow_state(&engine, hand, "single", (1, 12), vec![c(&r, 12, 0, 0)]);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        plays_contain(&legal, &[c(&r, 0, 0, 0)]),
        "级牌单张应压过 A: {legal:?}"
    );
}

#[test]
fn level_bomb_beats_a_bomb() {
    let r = fixture_body();
    let engine = engine();
    let bomb2 = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 0, 1),
        c(&r, 0, 2, 0),
        c(&r, 0, 2, 1),
    ];
    let bomb_a = vec![
        c(&r, 12, 0, 0),
        c(&r, 12, 0, 1),
        c(&r, 12, 2, 0),
        c(&r, 12, 2, 1),
    ];
    let state = follow_state(&engine, bomb2.clone(), "bomb-4", (4, 12), bomb_a);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        plays_contain(&legal, &bomb2),
        "级牌炸弹应压过 A 炸弹: {legal:?}"
    );
}

#[test]
fn high_straight_beats_wheel() {
    let r = fixture_body();
    let engine = engine();
    // 上家轮子 (5,0)，10-J-Q-K-A 顺子（顶 A，key 12）可压。
    let hand = vec![
        c(&r, 8, 0, 0),
        c(&r, 9, 1, 0),
        c(&r, 10, 2, 0),
        c(&r, 11, 3, 0),
        c(&r, 12, 0, 1),
    ];
    let wheel = vec![
        c(&r, 12, 0, 0),
        c(&r, 0, 2, 0),
        c(&r, 1, 2, 0),
        c(&r, 2, 2, 0),
        c(&r, 3, 2, 0),
    ];
    let state = follow_state(&engine, hand.clone(), "straight-5", (5, 0), wheel);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(plays_contain(&legal, &hand), "高顺子应压轮子: {legal:?}");
}

#[test]
fn synth_rank_key_table_level_above_a() {
    let r = fixture_body();
    // 打级 2、above-A：合成序 3,4,5,6,7,8,9,10,J,Q,K,A,2,little,big；rankKey=位序+1。
    let table: [(usize, u32); 13] = [
        (0, 13),
        (1, 1),
        (2, 2),
        (3, 3),
        (4, 4),
        (5, 5),
        (6, 6),
        (7, 7),
        (8, 8),
        (9, 9),
        (10, 10),
        (11, 11),
        (12, 12),
    ];
    let engine = engine();
    for &(rank, key) in &table {
        let card = c(&r, rank, 0, 0);
        let after = apply_play(&engine, vec![card], vec![card])
            .unwrap_or_else(|e| panic!("单张 {rank} 应合法: {e:?}"));
        let last = after.last_play.as_ref().unwrap();
        assert_eq!(
            last.pattern_key,
            (1, key),
            "rank {rank} 合成序 rankKey 不符"
        );
    }
    let little = joker_id(&r, false, 0);
    let big = joker_id(&r, true, 0);
    let after = apply_play(&engine, vec![little], vec![little]).unwrap();
    assert_eq!(after.last_play.as_ref().unwrap().pattern_key, (1, 14));
    let after = apply_play(&engine, vec![big], vec![big]).unwrap();
    assert_eq!(after.last_play.as_ref().unwrap().pattern_key, (1, 15));
}

#[test]
fn straight_10jqka_top_a() {
    let r = fixture_body();
    // 10-J-Q-K-A 杂花色，高点 A（下标 12）；组牌型按自然序，A 取自然位 13。
    let cards = vec![
        c(&r, 8, 0, 0),
        c(&r, 9, 1, 0),
        c(&r, 10, 2, 0),
        c(&r, 11, 3, 0),
        c(&r, 12, 0, 0),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "straight-5",
        (5, 13),
        "顺子 10-J-Q-K-A",
    );
}

#[test]
fn straight_wrap_qka23_is_illegal() {
    let r = fixture_body();
    // Q-K-A-2-3：A 后接 2 非法（§5.13 第 891 行）。
    let cards = vec![
        c(&r, 10, 0, 0),
        c(&r, 11, 0, 0),
        c(&r, 12, 0, 0),
        c(&r, 0, 0, 0),
        c(&r, 1, 0, 0),
    ];
    let engine = engine();
    assert_illegal(
        apply_play(&engine, cards.clone(), cards),
        "Q-K-A-2-3 绕回顺子",
    );
}

#[test]
fn straight_rejects_joker() {
    let r = fixture_body();
    let little = joker_id(&r, false, 0);
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 1, 0, 0),
        c(&r, 2, 0, 0),
        c(&r, 3, 0, 0),
        little,
    ];
    let engine = engine();
    assert_illegal(apply_play(&engine, cards.clone(), cards), "带小王的顺子");
}

#[test]
fn straight_rejects_gap() {
    let r = fixture_body();
    // 2-3-4-6-7：5 断档。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 1, 0, 0),
        c(&r, 2, 0, 0),
        c(&r, 4, 0, 0),
        c(&r, 5, 0, 0),
    ];
    let engine = engine();
    assert_illegal(
        apply_play(&engine, cards.clone(), cards),
        "断档顺子 2-3-4-6-7",
    );
}

// ---------- 三连对（plate-3） ----------

#[test]
fn plate_234_recognized() {
    let r = fixture_body();
    // 2/3/4 各一对，高点 4（下标 2），共 6 张；自然序 key 3。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 2, 0),
        c(&r, 1, 0, 0),
        c(&r, 1, 2, 0),
        c(&r, 2, 0, 0),
        c(&r, 2, 2, 0),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "plate-3",
        (6, 3),
        "三连对 2-3-4",
    );
}

#[test]
fn plate_qka_boundary_endpoint() {
    let r = fixture_body();
    // Q-K-A 端点三连对，高点 A（下标 12）；自然序 key 13。
    let cards = vec![
        c(&r, 10, 0, 0),
        c(&r, 10, 2, 0),
        c(&r, 11, 0, 0),
        c(&r, 11, 2, 0),
        c(&r, 12, 0, 0),
        c(&r, 12, 2, 0),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "plate-3",
        (6, 13),
        "端点三连对 Q-K-A",
    );
}

#[test]
fn plate_rejects_non_consecutive() {
    let r = fixture_body();
    // 2-3-5 断档。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 2, 0),
        c(&r, 1, 0, 0),
        c(&r, 1, 2, 0),
        c(&r, 3, 0, 0),
        c(&r, 3, 2, 0),
    ];
    let engine = engine();
    assert_illegal(
        apply_play(&engine, cards.clone(), cards),
        "不连续三连对 2-3-5",
    );
}

#[test]
fn plate_rejects_wrong_card_count() {
    let r = fixture_body();
    // 只有两对（4 张），不是三连对。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 2, 0),
        c(&r, 1, 0, 0),
        c(&r, 1, 2, 0),
    ];
    let engine = engine();
    assert_illegal(apply_play(&engine, cards.clone(), cards), "两对冒充三连对");
}

// ---------- 钢板（steel-2） ----------

#[test]
fn steel_23_recognized() {
    let r = fixture_body();
    // 2/3 各三张，高点 3（下标 1），共 6 张；自然序 key 2。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 0, 1),
        c(&r, 0, 2, 0),
        c(&r, 1, 0, 0),
        c(&r, 1, 0, 1),
        c(&r, 1, 2, 0),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "steel-2",
        (6, 2),
        "钢板 2-3",
    );
}

#[test]
fn steel_qk_boundary_endpoint() {
    let r = fixture_body();
    // Q-K 端点两组三张，高点 K（下标 11）；自然序 key 12。
    let cards = vec![
        c(&r, 10, 0, 0),
        c(&r, 10, 0, 1),
        c(&r, 10, 2, 0),
        c(&r, 11, 0, 0),
        c(&r, 11, 0, 1),
        c(&r, 11, 2, 0),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "steel-2",
        (6, 12),
        "端点钢板 Q-K",
    );
}

#[test]
fn steel_rejects_non_consecutive() {
    let r = fixture_body();
    // 2-4 断档。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 0, 1),
        c(&r, 0, 2, 0),
        c(&r, 2, 0, 0),
        c(&r, 2, 0, 1),
        c(&r, 2, 2, 0),
    ];
    let engine = engine();
    assert_illegal(apply_play(&engine, cards.clone(), cards), "不连续钢板 2-4");
}

#[test]
fn steel_rejects_wrong_grouping() {
    let r = fixture_body();
    // 一组三张 2 + 两张不同点数单张 3/4（5 张），既不构成两组三张连，也不构成对子。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 0, 1),
        c(&r, 0, 2, 0),
        c(&r, 1, 0, 0),
        c(&r, 2, 0, 0),
    ];
    let engine = engine();
    assert_illegal(
        apply_play(&engine, cards.clone(), cards),
        "三张加两个单张冒充钢板/三带二",
    );
}

// ---------- 炸弹 4–8 ----------

#[test]
fn bomb4_recognized_boundary_four() {
    let r = fixture_body();
    // 2 四张（黑桃×2 + 方块×2）；级牌 2 above-A，rankKey 13。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 0, 1),
        c(&r, 0, 2, 0),
        c(&r, 0, 2, 1),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "bomb-4",
        (4, 13),
        "炸弹 4 张 2（边界 4）",
    );
}

#[test]
fn bomb8_recognized_boundary_eight() {
    let r = fixture_body();
    // 7（下标 5）全花色全副本共 8 张，rankKey 5。
    let mut cards = Vec::new();
    for suit in 0..4 {
        for copy in 0..2 {
            cards.push(c(&r, 5, suit, copy));
        }
    }
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "bomb-8",
        (8, 5),
        "炸弹 8 张 7（边界 8）",
    );
}

#[test]
fn bomb_rejects_cross_rank() {
    let r = fixture_body();
    // 三张 2 加一张 3，跨点数不成炸弹。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 0, 1),
        c(&r, 0, 2, 0),
        c(&r, 1, 0, 0),
    ];
    let engine = engine();
    assert_illegal(
        apply_play(&engine, cards.clone(), cards),
        "跨点数四张冒充炸弹",
    );
}

// ---------- 同花顺 ----------

#[test]
fn straight_flush_23456_recognized() {
    let r = fixture_body();
    // 同黑桃 2-3-4-5-6，高点 6（下标 4）；自然序 key 5。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 1, 0, 0),
        c(&r, 2, 0, 0),
        c(&r, 3, 0, 0),
        c(&r, 4, 0, 0),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "straight-flush-5",
        (5, 5),
        "同花顺 2-3-4-5-6",
    );
}

#[test]
fn flush_mixed_suit_falls_back_to_plain_straight() {
    let r = fixture_body();
    // 2-3-4-5 黑桃 + 6 红桃：花色混杂，应判普通顺子而不是同花顺；自然序 key 5。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 1, 0, 0),
        c(&r, 2, 0, 0),
        c(&r, 3, 0, 0),
        c(&r, 4, 1, 0),
    ];
    let engine = engine();
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "straight-5",
        (5, 5),
        "花色混杂退回普通顺子",
    );
}

#[test]
fn flush_rejects_joker() {
    let r = fixture_body();
    let little = joker_id(&r, false, 0);
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 1, 0, 0),
        c(&r, 2, 0, 0),
        c(&r, 3, 0, 0),
        little,
    ];
    let engine = engine();
    assert_illegal(apply_play(&engine, cards.clone(), cards), "带王的同花顺");
}

#[test]
fn flush_beats_bomb5_and_loses_to_bomb6() {
    let r = fixture_body();
    let engine = engine();
    // 同花顺 2-3-4-5-6（黑桃）压过 bomb-5。
    let flush = vec![
        c(&r, 0, 0, 0),
        c(&r, 1, 0, 0),
        c(&r, 2, 0, 0),
        c(&r, 3, 0, 0),
        c(&r, 4, 0, 0),
    ];
    let last_bomb5 = vec![
        c(&r, 5, 0, 0),
        c(&r, 5, 0, 1),
        c(&r, 5, 2, 0),
        c(&r, 5, 2, 1),
        c(&r, 5, 3, 0),
    ];
    let state = follow_state(&engine, flush.clone(), "bomb-5", (5, 5), last_bomb5);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        legal.contains(&Action::Play {
            cards: flush.clone()
        }),
        "同花顺应能压过 bomb-5: {legal:?}"
    );

    // bomb-6 压过同花顺（阶梯 bomb-5 < straight-flush-5 < bomb-6）。
    let bomb6 = vec![
        c(&r, 5, 0, 0),
        c(&r, 5, 0, 1),
        c(&r, 5, 1, 0),
        c(&r, 5, 1, 1),
        c(&r, 5, 2, 0),
        c(&r, 5, 2, 1),
    ];
    let state = follow_state(
        &engine,
        bomb6.clone(),
        "straight-flush-5",
        (5, 5),
        flush.clone(),
    );
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        legal.contains(&Action::Play { cards: bomb6 }),
        "bomb-6 应能压过同花顺: {legal:?}"
    );
}

// ---------- 四王炸 ----------

#[test]
fn joker_bomb_four_jokers() {
    let engine = engine();
    let little0 = joker_id(&engine.ruleset, false, 0);
    let little1 = joker_id(&engine.ruleset, false, 1);
    let big0 = joker_id(&engine.ruleset, true, 0);
    let big1 = joker_id(&engine.ruleset, true, 1);
    let cards = vec![little0, little1, big0, big1];
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "joker-bomb",
        (4, 15),
        "四王炸",
    );
}

#[test]
fn joker_bomb_beats_bomb8() {
    let r = fixture_body();
    let engine = engine();
    let little0 = joker_id(&engine.ruleset, false, 0);
    let little1 = joker_id(&engine.ruleset, false, 1);
    let big0 = joker_id(&engine.ruleset, true, 0);
    let big1 = joker_id(&engine.ruleset, true, 1);
    let joker = vec![little0, little1, big0, big1];
    let mut bomb8 = Vec::new();
    for suit in 0..4 {
        for copy in 0..2 {
            bomb8.push(c(&r, 5, suit, copy));
        }
    }
    let state = follow_state(&engine, joker.clone(), "bomb-8", (8, 5), bomb8);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        legal.contains(&Action::Play { cards: joker }),
        "四王炸应压过 bomb-8: {legal:?}"
    );
}

#[test]
fn joker_bomb_is_unbeatable() {
    let r = fixture_body();
    let engine = engine();
    let little0 = joker_id(&engine.ruleset, false, 0);
    let little1 = joker_id(&engine.ruleset, false, 1);
    let big0 = joker_id(&engine.ruleset, true, 0);
    let big1 = joker_id(&engine.ruleset, true, 1);
    let joker = vec![little0, little1, big0, big1];
    let mut bomb8 = Vec::new();
    for suit in 0..4 {
        for copy in 0..2 {
            bomb8.push(c(&r, 5, suit, copy));
        }
    }
    // 上家四王炸，bomb-8 也压不过。
    let state = follow_state(&engine, bomb8.clone(), "joker-bomb", (4, 14), joker);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        !legal.iter().any(|a| matches!(a, Action::Play { .. })),
        "四王炸之上不应有可压的 Play: {legal:?}"
    );
    assert!(legal.contains(&Action::Pass), "只能过牌");
}

#[test]
fn joker_bomb_rejects_three_jokers_plus_plain() {
    let r = fixture_body();
    let engine = engine();
    let little0 = joker_id(&engine.ruleset, false, 0);
    let little1 = joker_id(&engine.ruleset, false, 1);
    let big0 = joker_id(&engine.ruleset, true, 0);
    let cards = vec![little0, little1, big0, c(&r, 0, 0, 0)];
    assert_illegal(
        apply_play(&engine, cards.clone(), cards),
        "三王一普通牌冒充王炸",
    );
}

#[test]
fn joker_bomb_rejects_two_big_only() {
    let engine = engine();
    let big0 = joker_id(&engine.ruleset, true, 0);
    let big1 = joker_id(&engine.ruleset, true, 1);
    let cards = vec![big0, big1];
    assert_illegal(apply_play(&engine, cards.clone(), cards), "双大王不带小王");
}

// ---------- 比较 ----------

#[test]
fn bomb4_loses_to_bomb5_count_first() {
    let r = fixture_body();
    let engine = engine();
    // bomb-5（5 点，下标 5）压过 bomb-4（A，下标 12）：张数优先，点数让位。
    let bomb5 = vec![
        c(&r, 5, 0, 0),
        c(&r, 5, 0, 1),
        c(&r, 5, 2, 0),
        c(&r, 5, 2, 1),
        c(&r, 5, 3, 0),
    ];
    let bomb4_a = vec![
        c(&r, 12, 0, 0),
        c(&r, 12, 0, 1),
        c(&r, 12, 2, 0),
        c(&r, 12, 2, 1),
    ];
    let state = follow_state(&engine, bomb5.clone(), "bomb-4", (4, 12), bomb4_a);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        legal.contains(&Action::Play { cards: bomb5 }),
        "5 张炸弹应压过 4 张炸弹（张数优先）: {legal:?}"
    );
}

#[test]
fn same_count_bomb_compares_rank() {
    let r = fixture_body();
    let engine = engine();
    // 同 4 张：A 炸（12）压过 K 炸（11）。
    let bomb_a = vec![
        c(&r, 12, 0, 0),
        c(&r, 12, 0, 1),
        c(&r, 12, 2, 0),
        c(&r, 12, 2, 1),
    ];
    let bomb_k = vec![
        c(&r, 11, 0, 0),
        c(&r, 11, 0, 1),
        c(&r, 11, 2, 0),
        c(&r, 11, 2, 1),
    ];
    let state = follow_state(&engine, bomb_a.clone(), "bomb-4", (4, 11), bomb_k.clone());
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        legal.contains(&Action::Play {
            cards: bomb_a.clone()
        }),
        "同张数 A 炸应压过 K 炸: {legal:?}"
    );

    // 反向：K 炸压不过 A 炸。
    let state = follow_state(&engine, bomb_k.clone(), "bomb-4", (4, 12), bomb_a);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        !legal.iter().any(|a| matches!(a, Action::Play { .. })),
        "K 炸不应压过 A 炸: {legal:?}"
    );
}

#[test]
fn pair_compares_rank_same_pattern() {
    let r = fixture_body();
    let engine = engine();
    // 对子 3（1）压过对子 2（0）。
    let pair3 = vec![c(&r, 1, 0, 0), c(&r, 1, 2, 0)];
    let pair2 = vec![c(&r, 0, 0, 0), c(&r, 0, 2, 0)];
    let state = follow_state(&engine, pair3.clone(), "pair", (2, 0), pair2);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        legal.contains(&Action::Play { cards: pair3 }),
        "对子 3 应压过对子 2: {legal:?}"
    );
}

// ---------- 枚举与 hold 口径 ----------

#[test]
fn lead_enumerates_each_single() {
    let r = fixture_body();
    let engine = engine();
    let hand = vec![c(&r, 0, 0, 0), c(&r, 1, 0, 0)];
    let state = state_with(&engine, hand);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    let plays: Vec<&Action> = legal
        .iter()
        .filter(|a| matches!(a, Action::Play { .. }))
        .collect();
    assert_eq!(plays.len(), 2, "领出两张单张都应枚举: {legal:?}");
    assert!(!legal.contains(&Action::Pass), "领出阶段不能过牌");
}

#[test]
fn follow_filters_unbeatable_and_offers_pass() {
    let r = fixture_body();
    let engine = engine();
    // 上家单张 A（rankKey 12），手牌 3/4 均低于 A，无可压单张，只能 pass。
    let hand = vec![c(&r, 1, 0, 0), c(&r, 2, 0, 0)];
    let state = follow_state(&engine, hand, "single", (1, 12), vec![c(&r, 12, 0, 0)]);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        !legal.iter().any(|a| matches!(a, Action::Play { .. })),
        "小牌压不过上家 A: {legal:?}"
    );
    assert!(legal.contains(&Action::Pass), "跟牌阶段应提供 pass");
}

#[test]
fn enumerates_plate_steel_bomb_candidates() {
    let r = fixture_body();
    let engine = engine();
    // 2 四张、3 三张、4 两张：可成型 bomb-4(2)、steel-2(2-3)、plate-3(2-3-4)。
    let hand = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 0, 1),
        c(&r, 0, 2, 0),
        c(&r, 0, 2, 1),
        c(&r, 1, 0, 0),
        c(&r, 1, 0, 1),
        c(&r, 1, 2, 0),
        c(&r, 2, 0, 0),
        c(&r, 2, 2, 0),
    ];
    let state = state_with(&engine, hand);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    let mut plays: Vec<Vec<u16>> = legal
        .iter()
        .filter_map(|a| match a {
            Action::Play { cards } => Some(cards.clone()),
            Action::Pass => None,
        })
        .collect();
    for p in plays.iter_mut() {
        p.sort_unstable();
    }
    let expect = |cards: &[u16]| {
        let mut sorted = cards.to_vec();
        sorted.sort_unstable();
        plays.contains(&sorted)
    };
    // bomb-4：2♠×2 + 2♦×2。
    assert!(
        expect(&[
            c(&r, 0, 0, 0),
            c(&r, 0, 0, 1),
            c(&r, 0, 2, 0),
            c(&r, 0, 2, 1)
        ]),
        "应枚举 bomb-4 候选: {plays:?}"
    );
    // steel-2：2 三张 + 3 三张。
    assert!(
        expect(&[
            c(&r, 0, 0, 0),
            c(&r, 0, 0, 1),
            c(&r, 0, 2, 0),
            c(&r, 1, 0, 0),
            c(&r, 1, 0, 1),
            c(&r, 1, 2, 0)
        ]),
        "应枚举 steel-2 候选: {plays:?}"
    );
    // plate-3：2/3/4 各一对（枚举取每点数前两张副本）。
    assert!(
        expect(&[
            c(&r, 0, 0, 0),
            c(&r, 0, 0, 1),
            c(&r, 1, 0, 0),
            c(&r, 1, 0, 1),
            c(&r, 2, 0, 0),
            c(&r, 2, 2, 0)
        ]),
        "应枚举 plate-3 候选: {plays:?}"
    );
}

#[test]
fn legal_actions_prefers_real_pair_over_wildcard() {
    let r = fixture_body();
    let engine = engine();
    // 两张真牌 3 + 一张百搭：对子 3 的规范代表取纯真牌解（第二档：百搭用量最少），
    // 不输出带百搭的等价组合。
    let hand = vec![c(&r, 1, 0, 0), c(&r, 1, 2, 0), w(&r, 0)];
    let state = state_with(&engine, hand);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    let wild = w(&r, 0);
    let pair_plays: Vec<&Vec<u16>> = legal
        .iter()
        .filter_map(|a| match a {
            Action::Play { cards } if cards.len() == 2 => Some(cards),
            _ => None,
        })
        .collect();
    assert!(
        !pair_plays.iter().any(|p| p.contains(&wild)),
        "对子规范代表不得带百搭: {pair_plays:?}"
    );
    assert!(
        plays_contain(&legal, &[c(&r, 1, 0, 0), c(&r, 1, 2, 0)]),
        "应输出纯真牌对子: {legal:?}"
    );
}

#[test]
fn legal_actions_pair_lex_min_by_card_id() {
    let r = fixture_body();
    let engine = engine();
    // 三张 3（S copy0 / S copy1 / D copy0）：成对解按真牌牌 id 升序字典序取最小 [S0,S1]。
    let hand = vec![c(&r, 1, 0, 0), c(&r, 1, 0, 1), c(&r, 1, 2, 0)];
    let state = state_with(&engine, hand);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    let mut plays: Vec<Vec<u16>> = legal
        .iter()
        .filter_map(|a| match a {
            Action::Play { cards } => Some(cards.clone()),
            Action::Pass => None,
        })
        .collect();
    for p in plays.iter_mut() {
        p.sort_unstable();
    }
    let pair = plays.iter().find(|p| p.len() == 2).expect("应有对子候选");
    let mut expected = vec![c(&r, 1, 0, 0), c(&r, 1, 0, 1)];
    expected.sort_unstable();
    assert_eq!(
        *pair, expected,
        "真牌牌 id 字典序最小者应作为规范代表: {plays:?}"
    );
}

// ---------- 逢人配（§5.13 第 893 行、§5.10 第 773 行） ----------

/// 逢人配源牌：红桃级牌（H 花色、`@level` → 初始级 2），即 rank 0 / suit 1 两张副本。
fn w(r: &RuleSetBody, copy: u8) -> u16 {
    c(r, 0, 1, copy)
}

#[test]
fn wildcard_single_own_rank() {
    let r = fixture_body();
    let engine = engine();
    // 整手全百搭：按自身点数（级牌 2，above-A 合成序 rankKey 13）用，不走最大解。
    let wild = w(&r, 0);
    assert_played(
        apply_play(&engine, vec![wild], vec![wild]),
        "single",
        (1, 13),
        "百搭单独出按自身点数",
    );
    let state = state_with(&engine, vec![wild]);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        plays_contain(&legal, &[wild]),
        "单张百搭应进枚举: {legal:?}"
    );
}

#[test]
fn wildcard_pair_mixed_fills_slot() {
    let r = fixture_body();
    let engine = engine();
    // 真牌 3（下标 1）一张 + 百搭补成对子 3。
    let cards = vec![c(&r, 1, 0, 0), w(&r, 0)];
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "pair",
        (2, 1),
        "百搭补成对子 3",
    );
}

#[test]
fn wildcard_pair_all_wildcards() {
    let r = fixture_body();
    let engine = engine();
    // 整手全百搭两张：按自身点数成对，级牌 2 above-A，rankKey 13。
    let cards = vec![w(&r, 0), w(&r, 1)];
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "pair",
        (2, 13),
        "两张百搭成对按自身点数",
    );
}

#[test]
fn all_wildcard_count_decides_pattern_1_to_8() {
    let engine = engine_with_copies(8, false);
    let r = &engine.ruleset;
    // 整手全百搭：张数直接定牌型，同一 rankKey（级牌 2，above-A 取 13）；
    // 不得判成顺子 / 三连对 / 钢板 / 三带二 / 同花顺。
    for n in 1u32..=8 {
        let hand: Vec<u16> = (0..n).map(|copy| w(r, copy as u8)).collect();
        let expected_id = match n {
            1 => "single".to_owned(),
            2 => "pair".to_owned(),
            3 => "triple".to_owned(),
            _ => format!("bomb-{n}"),
        };
        let after = apply_play(&engine, hand.clone(), hand)
            .unwrap_or_else(|e| panic!("{n} 张百搭应合法: {e:?}"));
        let last = after.last_play.as_ref().unwrap();
        assert_eq!(last.pattern_id, expected_id, "{n} 张百搭牌型");
        assert_eq!(last.pattern_key, (n, 13), "{n} 张百搭 rankKey");
    }
}

#[test]
fn all_wildcard_over_eight_rejected() {
    let engine = engine_with_copies(10, false);
    let r = &engine.ruleset;
    let hand: Vec<u16> = (0..9).map(|copy| w(r, copy)).collect();
    assert_illegal(
        apply_play(&engine, hand.clone(), hand),
        "9 张百搭超出已声明值域",
    );
}

#[test]
fn all_wildcard_undeclared_bomb_rejected() {
    let engine = engine_with_copies(8, true);
    let r = &engine.ruleset;
    let hand: Vec<u16> = (0..5).map(|copy| w(r, copy)).collect();
    assert_illegal(
        apply_play(&engine, hand.clone(), hand),
        "未声明 bomb-5，全百搭 5 张应按 IllegalAction 拒",
    );
}

#[test]
fn wildcard_triple_fills_slot() {
    let r = fixture_body();
    let engine = engine();
    // 真牌 3 两张 + 百搭补成三张 3。
    let cards = vec![c(&r, 1, 0, 0), c(&r, 1, 2, 0), w(&r, 0)];
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "triple",
        (3, 1),
        "百搭补成三张 3",
    );
}

#[test]
fn wildcard_straight_single_gap() {
    let r = fixture_body();
    let engine = engine();
    // 真牌 2-4-5-6（下标 0/2/3/4）+ 百搭补 3 → 顺子 2-3-4-5-6，高点 6 下标 4。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 2, 2, 0),
        c(&r, 3, 3, 0),
        c(&r, 4, 0, 0),
        w(&r, 0),
    ];
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "straight-5",
        (5, 5),
        "百搭补顺子单孔",
    );
}

#[test]
fn wildcard_straight_multi_gap_picks_top_solution() {
    let r = fixture_body();
    let engine = engine();
    // 真牌 3-4-5-6（下标 1/2/3/4）+ 百搭：可达解 2-3-4-5-6（高点 4）与 3-4-5-6-7
    // （高点 5）；按「rankKey 最大者」取 3-4-5-6-7，高点 7 下标 5，自然序 key 6。
    let cards = vec![
        c(&r, 1, 0, 0),
        c(&r, 2, 1, 0),
        c(&r, 3, 2, 0),
        c(&r, 4, 3, 0),
        w(&r, 0),
    ];
    assert_played(
        apply_play(&engine, cards.clone(), cards.clone()),
        "straight-5",
        (5, 6),
        "多孔顺子取最大解 3-4-5-6-7",
    );
    let state = state_with(&engine, cards.clone());
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        plays_contain(&legal, &cards),
        "最大解顺子应进枚举: {legal:?}"
    );
}

#[test]
fn wildcard_plate_fills_slot() {
    let r = fixture_body();
    let engine = engine();
    // 2 一对、3 一对、4 一张 + 百搭补成三连对 2-3-4，高点 4 下标 2，自然序 key 3。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 2, 0),
        c(&r, 1, 0, 0),
        c(&r, 1, 2, 0),
        c(&r, 2, 0, 0),
        w(&r, 0),
    ];
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "plate-3",
        (6, 3),
        "百搭补三连对",
    );
}

#[test]
fn wildcard_steel_fills_slot() {
    let r = fixture_body();
    let engine = engine();
    // 2 三张、3 两张 + 百搭补成钢板 2-3，高点 3 下标 1，自然序 key 2。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 0, 1),
        c(&r, 0, 2, 0),
        c(&r, 1, 0, 0),
        c(&r, 1, 0, 1),
        w(&r, 0),
    ];
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "steel-2",
        (6, 2),
        "百搭补钢板",
    );
}

#[test]
fn wildcard_bomb_fills_slot() {
    let r = fixture_body();
    let engine = engine();
    // 真牌 2 三张 + 百搭补成 bomb-4；级牌 2 above-A，rankKey 13。
    let cards = vec![c(&r, 0, 0, 0), c(&r, 0, 0, 1), c(&r, 0, 2, 0), w(&r, 0)];
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "bomb-4",
        (4, 13),
        "百搭补炸弹",
    );
}

#[test]
fn wildcard_cannot_substitute_joker() {
    let r = fixture_body();
    let engine = engine();
    let little = joker_id(&r, false, 0);
    // 百搭不得替代大小王：百搭 + 小王不成任何牌型。
    let cards = vec![little, w(&r, 0)];
    assert_illegal(apply_play(&engine, cards.clone(), cards), "百搭代小王");
}

#[test]
fn joker_bomb_rejects_wildcard() {
    let r = fixture_body();
    let engine = engine();
    let little0 = joker_id(&r, false, 0);
    let little1 = joker_id(&r, false, 1);
    let big0 = joker_id(&r, true, 0);
    let big1 = joker_id(&r, true, 1);
    // 4 王 + 百搭：王炸不接受百搭。
    let five = vec![little0, little1, big0, big1, w(&r, 0)];
    assert_illegal(apply_play(&engine, five.clone(), five), "4 王 + 百搭");
    // 3 真王 + 百搭：不成王炸。
    let four = vec![little0, little1, big0, w(&r, 0)];
    assert_illegal(apply_play(&engine, four.clone(), four), "3 王 + 百搭");
}

#[test]
fn wildcard_straight_flush_makes_flush() {
    let r = fixture_body();
    let engine = engine();
    // 黑桃 2-3-4-5（下标 0/1/2/3）+ 百搭补 6 → 同花顺 2-3-4-5-6，高点 6 下标 4，
    // 自然序 key 5。`inStraightFlush=true`（§5.10 第 773 行）后百搭可入同花顺，不得退回普通顺子。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 1, 0, 0),
        c(&r, 2, 0, 0),
        c(&r, 3, 0, 0),
        w(&r, 0),
    ];
    assert_played(
        apply_play(&engine, cards.clone(), cards.clone()),
        "straight-flush-5",
        (5, 5),
        "百搭补同花顺",
    );
    let state = state_with(&engine, cards.clone());
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        plays_contain(&legal, &cards),
        "百搭同花顺应进枚举: {legal:?}"
    );
}

// ---------- 跨型压制边界（§5.13 第 894/895 行：非同型非炸弹不能压） ----------

fn plays_contain(legal: &[Action], cards: &[u16]) -> bool {
    let mut target = cards.to_vec();
    target.sort_unstable();
    legal.iter().any(|a| match a {
        Action::Play { cards: cc } => {
            let mut cc = cc.clone();
            cc.sort_unstable();
            cc == target
        }
        Action::Pass => false,
    })
}

#[test]
fn pair_cannot_beat_single() {
    let r = fixture_body();
    let engine = engine();
    let pair = vec![c(&r, 0, 0, 0), c(&r, 0, 2, 0)];
    let state = follow_state(
        &engine,
        pair.clone(),
        "single",
        (1, 0),
        vec![c(&r, 0, 0, 1)],
    );
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(!plays_contain(&legal, &pair), "对子不得压单张: {legal:?}");
    assert_illegal(
        engine.apply(state, Action::Play { cards: pair }),
        "对子压单张",
    );
}

#[test]
fn triple_cannot_beat_pair() {
    let r = fixture_body();
    let engine = engine();
    let triple = vec![c(&r, 0, 0, 0), c(&r, 0, 0, 1), c(&r, 0, 2, 0)];
    let pair = vec![c(&r, 0, 0, 0), c(&r, 0, 2, 0)];
    let state = follow_state(&engine, triple.clone(), "pair", (2, 0), pair);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(!plays_contain(&legal, &triple), "三张不得压对子: {legal:?}");
    assert_illegal(
        engine.apply(state, Action::Play { cards: triple }),
        "三张压对子",
    );
}

#[test]
fn plate_cannot_beat_straight() {
    let r = fixture_body();
    let engine = engine();
    let plate = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 2, 0),
        c(&r, 1, 0, 0),
        c(&r, 1, 2, 0),
        c(&r, 2, 0, 0),
        c(&r, 2, 2, 0),
    ];
    let straight = vec![
        c(&r, 0, 0, 0),
        c(&r, 1, 1, 0),
        c(&r, 2, 2, 0),
        c(&r, 3, 3, 0),
        c(&r, 4, 0, 0),
    ];
    let state = follow_state(&engine, plate.clone(), "straight-5", (5, 5), straight);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        !plays_contain(&legal, &plate),
        "3 连对不得压顺子: {legal:?}"
    );
    assert_illegal(
        engine.apply(state, Action::Play { cards: plate }),
        "3 连对压顺子",
    );
}

#[test]
fn steel_cannot_beat_plate() {
    let r = fixture_body();
    let engine = engine();
    let steel = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 0, 1),
        c(&r, 0, 2, 0),
        c(&r, 1, 0, 0),
        c(&r, 1, 0, 1),
        c(&r, 1, 2, 0),
    ];
    let plate = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 2, 0),
        c(&r, 1, 0, 0),
        c(&r, 1, 2, 0),
        c(&r, 2, 0, 0),
        c(&r, 2, 2, 0),
    ];
    let state = follow_state(&engine, steel.clone(), "plate-3", (6, 3), plate);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        !plays_contain(&legal, &steel),
        "钢板不得压 3 连对: {legal:?}"
    );
    assert_illegal(
        engine.apply(state, Action::Play { cards: steel }),
        "钢板压 3 连对",
    );
}

#[test]
fn bomb4_beats_straight() {
    let r = fixture_body();
    let engine = engine();
    let bomb = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 0, 1),
        c(&r, 0, 2, 0),
        c(&r, 0, 2, 1),
    ];
    let straight = vec![
        c(&r, 0, 0, 0),
        c(&r, 1, 1, 0),
        c(&r, 2, 2, 0),
        c(&r, 3, 3, 0),
        c(&r, 4, 0, 0),
    ];
    let state = follow_state(&engine, bomb.clone(), "straight-5", (5, 5), straight);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(plays_contain(&legal, &bomb), "bomb-4 应能压顺子: {legal:?}");
    let after = engine
        .apply(
            state,
            Action::Play {
                cards: bomb.clone(),
            },
        )
        .expect("炸弹压顺子应落子成功");
    let last = after.last_play.as_ref().expect("应有 last_play");
    assert_eq!(last.pattern_id, "bomb-4");
    assert_eq!(last.pattern_key, (4, 13));
}

#[test]
fn bomb4_beats_pair() {
    let r = fixture_body();
    let engine = engine();
    let bomb = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 0, 1),
        c(&r, 0, 2, 0),
        c(&r, 0, 2, 1),
    ];
    let pair = vec![c(&r, 1, 0, 0), c(&r, 1, 2, 0)];
    let state = follow_state(&engine, bomb.clone(), "pair", (2, 1), pair);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(plays_contain(&legal, &bomb), "bomb-4 应能压对子: {legal:?}");
    let after = engine
        .apply(state, Action::Play { cards: bomb })
        .expect("炸弹压对子应落子成功");
    let last = after.last_play.as_ref().expect("应有 last_play");
    assert_eq!(last.pattern_id, "bomb-4");
    assert_eq!(last.pattern_key, (4, 13));
}

#[test]
fn straight_flush_beats_non_bomb() {
    let r = fixture_body();
    let engine = engine();
    let flush = vec![
        c(&r, 0, 0, 0),
        c(&r, 1, 0, 0),
        c(&r, 2, 0, 0),
        c(&r, 3, 0, 0),
        c(&r, 4, 0, 0),
    ];
    let single = vec![c(&r, 0, 0, 1)];
    let state = follow_state(&engine, flush.clone(), "single", (1, 12), single);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(plays_contain(&legal, &flush), "同花顺应能压单张: {legal:?}");
    let after = engine
        .apply(state, Action::Play { cards: flush })
        .expect("同花顺压单张应落子成功");
    let last = after.last_play.as_ref().expect("应有 last_play");
    assert_eq!(last.pattern_id, "straight-flush-5");
    assert_eq!(last.pattern_key, (5, 5));
}

#[test]
fn same_pattern_follow_compares_key() {
    let r = fixture_body();
    let engine = engine();
    // 对子：对子 3（rankKey 1）压对子 2（rankKey 0）。
    let pair_low = vec![c(&r, 0, 0, 0), c(&r, 0, 2, 0)];
    let pair_high = vec![c(&r, 1, 0, 0), c(&r, 1, 2, 0)];
    let state = follow_state(&engine, pair_high.clone(), "pair", (2, 0), pair_low);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        plays_contain(&legal, &pair_high),
        "对子 3 应压对子 2: {legal:?}"
    );

    // 顺子：3-4-5-6-7（高点 7 下标 5）压 2-3-4-5-6（高点 6 下标 4）；反向不成立。
    let straight_low = vec![
        c(&r, 0, 0, 0),
        c(&r, 1, 1, 0),
        c(&r, 2, 2, 0),
        c(&r, 3, 3, 0),
        c(&r, 4, 0, 0),
    ];
    let straight_high = vec![
        c(&r, 1, 0, 0),
        c(&r, 2, 1, 0),
        c(&r, 3, 2, 0),
        c(&r, 4, 3, 0),
        c(&r, 5, 0, 0),
    ];
    let state = follow_state(
        &engine,
        straight_high.clone(),
        "straight-5",
        (5, 5),
        straight_low.clone(),
    );
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(
        plays_contain(&legal, &straight_high),
        "高顺子应压低顺子: {legal:?}"
    );
    let after = engine
        .apply(
            state,
            Action::Play {
                cards: straight_high.clone(),
            },
        )
        .expect("高顺子应落子成功");
    let last = after.last_play.as_ref().expect("应有 last_play");
    assert_eq!(last.pattern_id, "straight-5");
    assert_eq!(last.pattern_key, (5, 6));

    let state = follow_state(
        &engine,
        straight_low.clone(),
        "straight-5",
        (5, 6),
        straight_high.clone(),
    );
    assert_illegal(
        engine.apply(
            state,
            Action::Play {
                cards: straight_low,
            },
        ),
        "低顺子压高顺子",
    );
}

// ---------- B10：级牌组牌型自然位口径（第二十五次变更） ----------

#[test]
fn b10_level_two_in_straight_uses_natural_top() {
    let r = fixture_body();
    let engine = engine();
    // 级牌 = 2 时 `2-3-4-5-6` 合法；组牌型只按自然序顶牌 6 取 key 5，级牌 2 不升
    // above-A，也不存在「级牌当 A」的第二解，不额外加权。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 1, 1, 0),
        c(&r, 2, 2, 0),
        c(&r, 3, 3, 0),
        c(&r, 4, 0, 1),
    ];
    assert_played(
        apply_play(&engine, cards.clone(), cards),
        "straight-5",
        (5, 5),
        "级牌 2 入顺子按自然序顶牌",
    );
}

#[test]
fn b10_five_pairs_rejected_as_illegal() {
    let r = fixture_body();
    let engine = engine();
    // 五连对共 10 张：组牌型张数上限 6（plate-3 固定 6 张），超出一律 IllegalAction；
    // 炸弹 / 王炸 / 三带二不受此条约束。
    let mut cards = Vec::new();
    for rank in 0..5 {
        cards.push(c(&r, rank, 0, 0));
        cards.push(c(&r, rank, 2, 0));
    }
    assert_illegal(
        apply_play(&engine, cards.clone(), cards),
        "五连对 10 张超出组牌型上限",
    );
}

// ---------- B3：局级打级队 + levels 初始化 + 首局抽牌前置 ----------

#[test]
fn classic_deal_inits_levels_and_attacking_team() {
    let engine = engine();
    let state = engine.deal(M0_SEED).expect("发牌失败");
    // levelTable.initial = "2"，ranks ["2".."A"] 中 "2" 下标 0；两队都从 initial 起。
    assert_eq!(state.levels, vec![0, 0]);
    // 首局抽牌定先手：先手座位所属队即 attacking_team（§5.12 第 822 行）。
    let team = engine
        .ruleset
        .seats
        .teams
        .iter()
        .position(|t| t.contains(&state.turn_seat))
        .expect("先手座位应归属某一队");
    assert_eq!(state.attacking_team, team as u8);
    // 先手手牌非空、弃牌区为空；抽出的牌不剔出牌堆，正常参与本局。
    let hand_idx = engine
        .ruleset
        .zones
        .iter()
        .position(|z| z.id.0 == "hand")
        .unwrap() as u16;
    let hand = state
        .zones
        .iter()
        .find(|z| z.seat == state.turn_seat && z.zone_id == hand_idx)
        .expect("先手手牌区");
    assert!(!hand.cards.is_empty());
    let discard_idx = engine
        .ruleset
        .zones
        .iter()
        .position(|z| z.id.0 == "discard")
        .map(|i| i as u16);
    if let Some(idx) = discard_idx {
        let discard = state
            .zones
            .iter()
            .find(|z| z.zone_id == idx)
            .expect("弃牌区");
        assert!(discard.cards.is_empty());
    }
}
