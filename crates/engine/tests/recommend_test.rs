//! ⑦ 组牌推荐 `recommend` 集成用例（§5.11「组牌推荐」段）。
//!
//! `recommend` 是只读旁路：用例只断言 `Layout` 形状、守恒与确定性，不触碰
//! `stateHash` / `legal_actions` / 判定链。

use gf_engine::cases::{classic_engine, m0_ruleset_yaml, M0_SEED};
use gf_engine::contract::{Engine, Seat};
use gf_engine::engine::M0Engine;
use gf_engine::ruleset::from_yaml;
use gf_engine::ruleset::model::ZoneScope;
use std::collections::BTreeMap;

fn deal_state(engine: &M0Engine) -> gf_engine::contract::State {
    engine.deal(M0_SEED).expect("发牌失败")
}

fn hand_of(state: &gf_engine::contract::State, engine: &M0Engine, seat: u8) -> Vec<u16> {
    let hand_idx = engine
        .ruleset
        .zones
        .iter()
        .position(|z| z.scope == ZoneScope::Private)
        .unwrap_or(0) as u16;
    state
        .zones
        .iter()
        .find(|z| z.seat == seat && z.zone_id == hand_idx)
        .map(|z| z.cards.clone())
        .unwrap_or_default()
}

fn counts(cards: &[u16]) -> BTreeMap<u16, usize> {
    let mut m = BTreeMap::new();
    for &c in cards {
        *m.entry(c).or_default() += 1;
    }
    m
}

/// 守恒断言：分组牌 + 散张 = 原手牌（多重集合相等）；分组内部互不重叠。
fn assert_conservation(layout: &gf_engine::recommend::Layout, hand: &[u16]) {
    let mut used: BTreeMap<u16, usize> = BTreeMap::new();
    for g in &layout.groups {
        for &c in &g.cards {
            *used.entry(c).or_default() += 1;
        }
    }
    let mut leftover = counts(hand);
    for (c, n) in &used {
        let have = leftover.entry(*c).or_default();
        assert!(*have >= *n, "牌 {c} 在分组里超发（手牌 {have} < 分组 {n}）");
        *have -= n;
    }
    let leftover_total: usize = leftover.values().sum();
    assert_eq!(
        leftover_total, layout.single_count as usize,
        "散张数应等于未进分组的剩余手牌数",
    );
    let group_total: usize = used.values().sum();
    assert_eq!(
        group_total + leftover_total,
        hand.len(),
        "分组张数与散张数应覆盖整手",
    );
}

/// `groups` 排序键：`(patternId 字典序升, patternKey 降, cards 升)`。
fn assert_groups_sorted(layout: &gf_engine::recommend::Layout) {
    let key = |g: &gf_engine::recommend::LayoutGroup| {
        (
            g.pattern_id.clone(),
            std::cmp::Reverse(g.pattern_key),
            g.cards.clone(),
        )
    };
    for w in layout.groups.windows(2) {
        assert!(
            key(&w[0]) <= key(&w[1]),
            "groups 未按 (patternId 升, patternKey 降, cards 升) 排序: {:?} vs {:?}",
            w[0],
            w[1]
        );
    }
}

#[test]
fn m0_single_only_layout_is_all_singles() {
    let engine = gf_engine::cases::build_engine().expect("m0 构造失败");
    let state = deal_state(&engine);
    let hand = hand_of(&state, &engine, 0);
    let layout = engine
        .recommend(&state, Seat(0))
        .expect("m0 recommend 不应报错");

    assert!(!layout.disabled);
    assert!(layout.groups.is_empty(), "m0 只声明 single，不应出成型分组");
    assert_eq!(layout.single_count as usize, hand.len());
    // 默认权重 single = -10：全散张 score = 手牌数 × (-10)。
    assert_eq!(layout.score, -(hand.len() as i64 * 10));
    assert!(layout.straight_flush_candidates.is_empty());
    assert!(!layout.has_straight_flush);
    assert_conservation(&layout, &hand);
}

#[test]
fn classic_layout_conservative_and_sorted() {
    let engine = classic_engine().expect("classic 构造失败");
    let state = deal_state(&engine);
    let hand = hand_of(&state, &engine, 0);
    let layout = engine
        .recommend(&state, Seat(0))
        .expect("classic recommend 不应报错");

    assert!(!layout.disabled);
    assert_conservation(&layout, &hand);
    assert_groups_sorted(&layout);

    // 同花顺候选独立于最优拆分：去重、按 cards 升序；hasStraightFlush 即候选非空。
    let mut dedup: Vec<Vec<u16>> = layout.straight_flush_candidates.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(dedup, layout.straight_flush_candidates);
    assert_eq!(layout.has_straight_flush, !dedup.is_empty());
}

#[test]
fn recommend_is_deterministic_read_only() {
    let engine = classic_engine().expect("classic 构造失败");
    let state = deal_state(&engine);
    let first = engine.recommend(&state, Seat(0)).expect("首次 recommend");
    let second = engine.recommend(&state, Seat(0)).expect("二次 recommend");
    assert_eq!(first, second, "同输入必须同输出（无随机）");

    // 只读旁路：两次调用之间状态快照不变（不进 stateHash、不改 legal_actions）。
    let before = gf_engine::hash::state_hash(&gf_engine::hash::CanonicalState::from(&state));
    let _ = engine.recommend(&state, Seat(0)).expect("三次 recommend");
    let after = gf_engine::hash::state_hash(&gf_engine::hash::CanonicalState::from(&state));
    assert_eq!(before, after, "recommend 不得改写 State");
}

#[test]
fn flush_candidates_listed_independent_of_split() {
    use gf_engine::patterns::card_id;
    let engine = classic_engine().expect("classic 构造失败");
    let r = &engine.ruleset;
    // 黑桃 3-4-5-6-7 同花顺（避开红桃级牌百搭），另带两张散张。
    let flush: Vec<u16> = (1..=5u16)
        .map(|rank| card_id(r, rank as usize, 0, 0))
        .collect();
    let fillers = [card_id(r, 8, 1, 0), card_id(r, 9, 2, 0)];
    let hand: Vec<u16> = flush.iter().chain(fillers.iter()).copied().collect();

    let mut state = engine.deal(M0_SEED).expect("发牌失败");
    let hand_idx = engine
        .ruleset
        .zones
        .iter()
        .position(|z| z.scope == ZoneScope::Private)
        .unwrap_or(0) as u16;
    let zone = state
        .zones
        .iter_mut()
        .find(|z| z.seat == 0 && z.zone_id == hand_idx)
        .expect("seat 0 手牌区");
    zone.cards = hand;
    state.turn_seat = 0;
    state.last_play = None;

    let layout = engine
        .recommend(&state, Seat(0))
        .expect("flush 用例 recommend 不应报错");

    assert!(layout.has_straight_flush, "手牌含同花顺，候选不得为空");
    assert!(
        layout.straight_flush_candidates.contains(&flush),
        "同花顺候选应列出该组：{:?}",
        layout.straight_flush_candidates
    );
    // 候选去重：同一组不得出现两次。
    let mut dedup = layout.straight_flush_candidates.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(dedup, layout.straight_flush_candidates);
}

#[test]
fn disabled_recommend_returns_empty_placeholder() {
    let src = format!(
        "{}\nai:\n  recommend: {{ enabled: false }}",
        m0_ruleset_yaml()
    );
    let body = from_yaml(&src).expect("disabled 夹具应构造成功");
    let engine = M0Engine::new(body);
    let state = deal_state(&engine);
    let layout = engine
        .recommend(&state, Seat(0))
        .expect("disabled recommend 不应报错");

    assert!(layout.disabled);
    assert!(layout.groups.is_empty());
    assert_eq!(layout.score, 0);
    assert_eq!(layout.single_count, 0);
    assert!(layout.straight_flush_candidates.is_empty());
    assert!(!layout.has_straight_flush);
}

#[test]
fn custom_weights_override_single_score() {
    // 自定义权重必须给全键；single 改为 0 后，全散张局 score = 0。
    let weights = "weights: { bomb4: 40, bomb5: 50, bomb6: 60, bomb7: 70, bomb8: 80, jokerBomb: 90, straightFlush: 55, straightBase: 40, straightRankBonus: 20, triplePairBase: 30, triplePairRankBonus: 20, plate3Base: 30, plate3RankBonus: 10, steel2Base: 40, steel2RankBonus: 10, pair: 0, single: 0 }";
    let src = format!(
        "{}\nai:\n  recommend: {{ enabled: true, {weights} }}",
        m0_ruleset_yaml()
    );
    let body = from_yaml(&src).expect("自定义权重夹具应构造成功");
    let engine = M0Engine::new(body);
    let state = deal_state(&engine);
    let hand = hand_of(&state, &engine, 0);
    let layout = engine
        .recommend(&state, Seat(0))
        .expect("自定义权重 recommend 不应报错");

    assert!(layout.groups.is_empty());
    assert_eq!(layout.single_count as usize, hand.len());
    assert_eq!(layout.score, 0, "single=0 时全散张局不得扣分");
}
