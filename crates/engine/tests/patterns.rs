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
    M0Engine {
        ruleset: fixture_body(),
    }
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
    // 2♠ copy 0 → rankKey 0。
    let card = c(&r, 0, 0, 0);
    let engine = engine();
    assert_played(
        apply_play(&engine, vec![card], vec![card]),
        "single",
        (1, 0),
        "单张 2",
    );
}

#[test]
fn single_joker_recognized() {
    let engine = engine();
    let little = joker_id(&engine.ruleset, false, 0);
    let big = joker_id(&engine.ruleset, true, 0);
    // 小王 rankKey 13、大王 14。
    assert_played(
        apply_play(&engine, vec![little], vec![little]),
        "single",
        (1, 13),
        "小王单张",
    );
    assert_played(
        apply_play(&engine, vec![big], vec![big]),
        "single",
        (1, 14),
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
        (2, 0),
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
        (3, 0),
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
        (5, 4),
        "顺子 2-3-4-5-6",
    );
}

#[test]
fn straight_wheel_a2345_is_legal_boundary() {
    let r = fixture_body();
    // A-2-3-4-5：唯一环形特例合法（§5.13 第 891 行）；高点按 5（下标 3）。
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
        (5, 3),
        "轮子顺 A-2-3-4-5",
    );
}

#[test]
fn straight_10jqka_top_a() {
    let r = fixture_body();
    // 10-J-Q-K-A 杂花色，高点 A（下标 12）。
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
        (5, 12),
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
    // 2/3/4 各一对，高点 4（下标 2），共 6 张。
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
        (6, 2),
        "三连对 2-3-4",
    );
}

#[test]
fn plate_qka_boundary_endpoint() {
    let r = fixture_body();
    // Q-K-A 端点三连对，高点 A（下标 12）。
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
        (6, 12),
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
    // 2/3 各三张，高点 3（下标 1），共 6 张。
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
        (6, 1),
        "钢板 2-3",
    );
}

#[test]
fn steel_qk_boundary_endpoint() {
    let r = fixture_body();
    // Q-K 端点两组三张，高点 K（下标 11）。
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
        (6, 11),
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
    // 一组三张 2 + 一对 3（5 张），不构成两组三张连。
    let cards = vec![
        c(&r, 0, 0, 0),
        c(&r, 0, 0, 1),
        c(&r, 0, 2, 0),
        c(&r, 1, 0, 0),
        c(&r, 1, 2, 0),
    ];
    let engine = engine();
    assert_illegal(
        apply_play(&engine, cards.clone(), cards),
        "三带两杂牌形冒充钢板",
    );
}

// ---------- 炸弹 4–8 ----------

#[test]
fn bomb4_recognized_boundary_four() {
    let r = fixture_body();
    // 2 四张（黑桃×2 + 方块×2），rankKey 0。
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
        (4, 0),
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
    // 同黑桃 2-3-4-5-6，高点 6（下标 4）。
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
        (5, 4),
        "同花顺 2-3-4-5-6",
    );
}

#[test]
fn flush_mixed_suit_falls_back_to_plain_straight() {
    let r = fixture_body();
    // 2-3-4-5 黑桃 + 6 红桃：花色混杂，应判普通顺子而不是同花顺。
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
        (5, 4),
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
        (5, 4),
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
        (4, 14),
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
    // 上家单张 A（rankKey 12），手牌两张小牌，无可压单张，只能 pass。
    let hand = vec![c(&r, 0, 0, 0), c(&r, 1, 0, 0)];
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
fn wildcard_cards_produce_no_candidate() {
    // TODO(M1-HOLD-wildcard)：逢人配源为红桃级牌（H 花色、初始级 2），即 id 26/27。
    // 本批 wildcard 牌不进任何候选；提交含 wildcard 的出牌判非法。
    let r = fixture_body();
    let engine = engine();
    let wildcard = c(&r, 0, 1, 0); // 红桃 2
    let other = c(&r, 1, 0, 0);
    let state = state_with(&engine, vec![wildcard, other]);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    for action in &legal {
        if let Action::Play { cards } = action {
            assert!(
                !cards.contains(&wildcard),
                "wildcard 牌不得进候选: {cards:?}"
            );
        }
    }
    assert_illegal(
        apply_play(&engine, vec![wildcard], vec![wildcard]),
        "单独提交 wildcard 牌",
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
    let state = follow_state(&engine, plate.clone(), "straight-5", (5, 4), straight);
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
    let state = follow_state(&engine, steel.clone(), "plate-3", (6, 2), plate);
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
    let state = follow_state(&engine, bomb.clone(), "straight-5", (5, 4), straight);
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
    assert_eq!(last.pattern_key, (4, 0));
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
    assert_eq!(last.pattern_key, (4, 0));
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
    let state = follow_state(&engine, flush.clone(), "single", (1, 0), single);
    let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
    assert!(plays_contain(&legal, &flush), "同花顺应能压单张: {legal:?}");
    let after = engine
        .apply(state, Action::Play { cards: flush })
        .expect("同花顺压单张应落子成功");
    let last = after.last_play.as_ref().expect("应有 last_play");
    assert_eq!(last.pattern_id, "straight-flush-5");
    assert_eq!(last.pattern_key, (5, 4));
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
        (5, 4),
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
    assert_eq!(last.pattern_key, (5, 5));

    let state = follow_state(
        &engine,
        straight_low.clone(),
        "straight-5",
        (5, 5),
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
