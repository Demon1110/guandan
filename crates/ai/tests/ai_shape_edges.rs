//! AI 侧「形状判定 / 视图扫描 / L0 直测」的边界补测（纯补测，不改实现）。
//!
//! 覆盖点：炸弹形状的四王/超 8 张/不足 4 张分支、`total_of_rank` 王与普通点、
//! `is_controller` 空牌动作、`bomb_count_in_hand` 空手牌与王计数、
//! `view_scan` 各锚点缺失与数字溢出分支、`L0Bot::decide` 的领出路径。

use gf_ai::shape::{
    bomb_count_in_hand, cards_of, is_bomb_shape, is_controller, rank_of, total_of_rank,
};
use gf_ai::view_scan::{
    discard_cards, hand_cards, last_play_cards, last_play_pattern_id, last_play_seat,
};
use gf_ai::{L0Bot};
use gf_engine::contract::Action;
use gf_table_runtime::room::BotPolicy;

fn play(cards: Vec<u16>) -> Action {
    Action::Play { cards }
}

// ---------- shape ----------

#[test]
fn bomb_shape_four_jokers_is_bomb() {
    // 两小王 + 两大王（id ≥ 104）→ 四王炸弹。
    assert!(is_bomb_shape(&play(vec![104, 105, 106, 107])));
}

#[test]
fn bomb_shape_same_rank_five_and_eight() {
    // rank 0 的全部 8 个 id：suit×26 + copy。
    let rank0: Vec<u16> = (0..4u16).flat_map(|suit| (0..2u16).map(move |copy| suit * 26 + copy)).collect();
    assert_eq!(rank0.len(), 8);
    assert!(is_bomb_shape(&play(rank0[..4].to_vec())));
    assert!(is_bomb_shape(&play(rank0[..5].to_vec())));
    assert!(is_bomb_shape(&play(rank0.clone())));
}

#[test]
fn bomb_shape_rejects_short_long_and_mixed() {
    // < 4 张。
    assert!(!is_bomb_shape(&play(vec![0, 2, 4])));
    // > 8 张（同点只有 8 张，这里用 9 个 id 走「超长」早退）。
    let nine: Vec<u16> = (0..9u16).map(|i| i * 2).collect();
    assert!(!is_bomb_shape(&play(nine)));
    // 4 张但不是同点（rank 0 三张 + rank 1 一张）。
    assert!(!is_bomb_shape(&play(vec![0, 2, 4, 8])));
    // 4 张含王但不全是王。
    assert!(!is_bomb_shape(&play(vec![0, 2, 4, 104])));
    // Pass 不是炸弹。
    assert!(!is_bomb_shape(&Action::Pass));
    assert!(cards_of(&Action::Pass).is_empty());
}

#[test]
fn rank_and_total_accounting() {
    assert_eq!(rank_of(104), 13);
    assert_eq!(rank_of(106), 14);
    assert_eq!(rank_of(0), 0);
    assert_eq!(rank_of(26), 0); // (26/2)%13 = 0 → 第二副同点
    assert_eq!(total_of_rank(13), 2);
    assert_eq!(total_of_rank(14), 2);
    assert_eq!(total_of_rank(0), 8);
}

#[test]
fn controller_and_bomb_count_edges() {
    // 空牌动作（Pass）不可能控场。
    assert!(!is_controller(&[], &[], &Action::Pass));
    // 顶牌点是最大（大王）→ 无人能压。
    assert!(is_controller(&[], &[], &play(vec![106])));
    // A 仍被王压 → 不算控场。
    assert!(!is_controller(&[], &[], &play(vec![24])));
    // 弃牌堆已出光全部王 → A 变成控场。
    assert!(is_controller(&[104, 105, 106, 107], &[], &play(vec![24])));
    // 空手牌没有炸弹。
    assert_eq!(bomb_count_in_hand(&[]), 0);
    // 4 张同点 = 1 个炸弹；4 张王再多算 1。
    assert_eq!(bomb_count_in_hand(&[0, 1, 26, 27]), 1);
    assert_eq!(bomb_count_in_hand(&[104, 105, 106, 107]), 1);
    assert_eq!(bomb_count_in_hand(&[0, 1, 26, 27, 104, 105, 106, 107]), 2);
}

// ---------- view_scan ----------

#[test]
fn hand_cards_without_anchor_or_with_overflow() {
    // 无 `"hand":{"cards":[` 锚点 → 空。
    assert!(hand_cards(br#"{"seat":0}"#).is_empty());
    // 有锚点无 `]` → 读到末尾仍收下数字。
    assert_eq!(hand_cards(br#""hand":{"cards":[3,7"#), vec![3, 7]);
    // 超出 u16 的数字被丢弃，后续数字照收。
    assert_eq!(hand_cards(br#""hand":{"cards":[99999,7]}"#), vec![7]);
}

#[test]
fn discard_without_cards_array_or_overflow() {
    // 有 `"discard":` 但没有 `"cards":[` → 空。
    assert!(discard_cards(br#"{"discard":{"seat":0}}"#).is_empty());
    // 无 `"discard":` → 空。
    assert!(discard_cards(br#"{"seat":0}"#).is_empty());
    // 数字溢出 u16 被跳过。
    assert_eq!(discard_cards(br#"{"discard":{"cards":[99999,4]}}"#), vec![4]);
}

#[test]
fn last_play_pattern_id_edges() {
    // 无 `"lastPlay":` → None。
    assert_eq!(last_play_pattern_id(br#"{"seat":0}"#), None);
    // 有 lastPlay 但无 patternId → None。
    assert_eq!(last_play_pattern_id(br#"{"lastPlay":{"seat":1}}"#), None);
    // 空 patternId → Some("")。
    assert_eq!(
        last_play_pattern_id(br#"{"lastPlay":{"patternId":""}}"#),
        Some(String::new())
    );
    assert_eq!(
        last_play_pattern_id(br#"{"lastPlay":{"patternId":"bomb-4"}}"#),
        Some("bomb-4".to_owned())
    );
}

#[test]
fn last_play_cards_edges() {
    assert!(last_play_cards(br#"{"seat":0}"#).is_empty());
    assert!(last_play_cards(br#"{"lastPlay":{"seat":1}}"#).is_empty());
    assert_eq!(
        last_play_cards(br#"{"hand":{"cards":[1]},"lastPlay":{"cards":[42,43]}}"#),
        vec![42, 43]
    );
    // seat 缺失（锚点不全）→ None。
    assert_eq!(last_play_seat(br#"{"lastPlay":{"seat":null}}"#), None);
}

// ---------- L0 ----------

#[test]
fn l0_lead_plays_smallest_single() {
    let bot = L0Bot;
    // 领出（视图里没有 lastPlay）：出最小单张。
    let lead = br#"{"hand":{"cards":[9,3,7]}}"#;
    assert_eq!(bot.decide(lead, 0), play(vec![3]));
    // 空手牌：过牌。
    assert_eq!(bot.decide(br#"{"hand":{"cards":[]}}"#, 0), Action::Pass);
    // 跟牌（lastPlay 来自他座）：过牌。
    let follow = br#"{"hand":{"cards":[9,3]},"lastPlay":{"seat":2,"patternId":"single","cards":[5]}}"#;
    assert_eq!(bot.decide(follow, 0), Action::Pass);
}
