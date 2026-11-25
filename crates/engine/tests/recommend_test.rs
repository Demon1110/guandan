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

// ---------- 第三十七次变更：黄金用例（两种 mode 各一组） ----------

const CLASSIC_YAML: &str = include_str!("fixtures/guandan-classic/ruleset.yaml");

/// 用 classic 夹具加一段 `ai.recommend` 构造引擎；`mode = None` 即省略 `mode`（默认 weighted）。
fn engine_with_mode(mode: Option<&str>) -> M0Engine {
    let src = match mode {
        None => format!("{CLASSIC_YAML}\nai:\n  recommend: {{ enabled: true }}"),
        Some(m) => format!("{CLASSIC_YAML}\nai:\n  recommend: {{ enabled: true, mode: {m} }}"),
    };
    let body = from_yaml(&src).expect("classic + ai.recommend 夹具应构造成功");
    M0Engine::new(body)
}

/// 把 seat 0 的手牌换成给定牌，并置为领出态（`last_play = None`）。
fn with_hand(engine: &M0Engine, hand: Vec<u16>) -> gf_engine::contract::State {
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
    state
}

/// 第三十七次变更第 8 条的用户手牌（示意手牌，28 张，不按 27 张裁）：
/// 炸弹 2×5、炸弹 5×5、顺子 10JQKA、三带二（三张 6 + 一对 8）、两对（一对 3、一对 7）、
/// 4 单（4、9、小王、大王）。花色刻意打散，避免意外凑出同花顺。
fn user_shape_hand(engine: &M0Engine) -> Vec<u16> {
    use gf_engine::patterns::card_id;
    let r = &engine.ruleset;
    let mut hand: Vec<u16> = Vec::new();
    // 炸弹 2（点数位 0）：避开红桃级牌（百搭），取 5 张真牌。
    for (rank, suit, copy) in [(0, 0, 0), (0, 0, 1), (0, 2, 0), (0, 2, 1), (0, 3, 0)] {
        hand.push(card_id(r, rank, suit, copy));
    }
    // 炸弹 5（点数位 3）：5 张。
    for (rank, suit, copy) in [(3, 0, 0), (3, 1, 0), (3, 2, 0), (3, 3, 0), (3, 0, 1)] {
        hand.push(card_id(r, rank, suit, copy));
    }
    // 顺子 10-J-Q-K-A（点数位 8..12），不同花色。
    for (rank, suit) in [(8, 0), (9, 1), (10, 2), (11, 3), (12, 0)] {
        hand.push(card_id(r, rank, suit, 0));
    }
    // 三带二：三张 6（点数位 4）+ 一对 8（点数位 6）。
    for (rank, suit, copy) in [(4, 1, 0), (4, 2, 0), (4, 3, 0), (6, 0, 0), (6, 1, 0)] {
        hand.push(card_id(r, rank, suit, copy));
    }
    // 两对：一对 3（点数位 1）、一对 7（点数位 5）。
    for (rank, suit, copy) in [(1, 0, 0), (1, 0, 1), (5, 3, 0), (5, 3, 1)] {
        hand.push(card_id(r, rank, suit, copy));
    }
    // 4 单：4（点数位 2）、9（点数位 7）+ 小王 + 大王。
    let base_total = r.deck.ranks.len() * r.deck.suits.len() * r.deck.copies as usize;
    hand.push(card_id(r, 2, 1, 0));
    hand.push(card_id(r, 7, 2, 0));
    hand.push(base_total as u16); // 小王 copy 0
    hand.push((base_total + r.deck.copies as usize) as u16); // 大王 copy 0
    assert_eq!(hand.len(), 28, "用户示意手牌 28 张");
    hand
}

/// 干净黄金手（23 张）：点数位刻意留空，任何 5 张连续窗口都缺一个点数，因此不存在
/// 意外顺子/同花顺/连对/钢板，最优拆分逐组可算（级牌 2、`levelInsert = above-A`，
/// 牌力序 3…A、2，故 2 的 `rankKey = 13`、5 = 3）：
///
/// - 炸弹 5（点数位 0 = 2）→ 50；炸弹 5（点数位 12 = A）→ 50
/// - 三带二（主牌三张 5：点数位 3、`rankKey = 3` → `30 + ⌊20×2/12⌋ = 33`）
/// - 三对（点数位 6、7、9）→ 0 ×3，其中点数位 6 的对接被后处理吸收进三带二
/// - 4 单（点数位 1、11、小王、大王）→ −10 ×4
///
/// 合计 `50 + 50 + 33 − 40 = 93`；手数 `2 炸 + 1 三带二 + 2 对 + 4 散张 = 9`。
fn clean_hand(engine: &M0Engine) -> (Vec<u16>, i64, usize) {
    use gf_engine::patterns::card_id;
    let r = &engine.ruleset;
    let mut hand: Vec<u16> = Vec::new();
    // 炸弹 2（点数位 0）5 张、炸弹 A（点数位 12）5 张。
    for (rank, suit, copy) in [(0, 0, 0), (0, 0, 1), (0, 2, 0), (0, 2, 1), (0, 3, 0)] {
        hand.push(card_id(r, rank, suit, copy));
    }
    for (rank, suit, copy) in [(12, 0, 0), (12, 0, 1), (12, 1, 0), (12, 2, 0), (12, 3, 0)] {
        hand.push(card_id(r, rank, suit, copy));
    }
    // 三带二：三张 5（点数位 3）+ 一对 8（点数位 6）。
    for (rank, suit, copy) in [(3, 0, 0), (3, 1, 0), (3, 3, 0), (6, 0, 0), (6, 2, 0)] {
        hand.push(card_id(r, rank, suit, copy));
    }
    // 余下两对：一对 9（点数位 7）、一对 J（点数位 9）。
    for (rank, suit, copy) in [(7, 0, 0), (7, 3, 0), (9, 1, 0), (9, 2, 0)] {
        hand.push(card_id(r, rank, suit, copy));
    }
    // 4 单：3（点数位 1）、K（点数位 11）+ 小王 + 大王。
    let base_total = r.deck.ranks.len() * r.deck.suits.len() * r.deck.copies as usize;
    hand.push(card_id(r, 1, 0, 0));
    hand.push(card_id(r, 11, 2, 0));
    hand.push(base_total as u16);
    hand.push((base_total + r.deck.copies as usize) as u16);
    assert_eq!(hand.len(), 23, "干净黄金手 23 张");
    (hand, 93, 9)
}

/// 分组身份签名：`(patternId, patternKey, 张数)`，按字典序。
fn layout_signature(layout: &gf_engine::recommend::Layout) -> Vec<(String, (u32, u32), usize)> {
    let mut sig: Vec<(String, (u32, u32), usize)> = layout
        .groups
        .iter()
        .map(|g| (g.pattern_id.clone(), g.pattern_key, g.cards.len()))
        .collect();
    sig.sort();
    sig
}

/// 用户示意手（28 张）的黄金向量。**实现结论与「10 手」口述不一致，属权重表的必然结果**：
/// 默认权重下把 5 张炸拆成「炸 4 + 喂一张给顺子」比留 5 张炸更高分（`50` vs `40 + 46`），
/// 故 DFS 给出的最优是 `bomb-4 ×2 + plate-3 + straight-5 ×2 + 4 散张 = 9 手 / 181 分`。
/// 这条差异已记入交付说明，是否改权重（让 5 张炸压过「炸 4 + 顺子」）需上级裁决，代码不擅自改。
#[test]
fn golden_user_shape_hand() {
    let mut expected = vec![
        ("bomb-4".to_owned(), (4, 3), 4),
        ("bomb-4".to_owned(), (4, 13), 4),
        ("plate-3".to_owned(), (6, 7), 6),
        ("straight-5".to_owned(), (5, 5), 5),
        ("straight-5".to_owned(), (5, 13), 5),
    ];
    expected.sort();

    for mode in [None, Some("least-hands")] {
        let engine = engine_with_mode(mode);
        let hand = user_shape_hand(&engine);
        let state = with_hand(&engine, hand.clone());
        let layout = engine
            .recommend(&state, Seat(0))
            .expect("用户示意手 recommend 不应报错");

        assert_conservation(&layout, &hand);
        assert_groups_sorted(&layout);
        assert_eq!(
            layout_signature(&layout),
            expected,
            "mode={mode:?} 实际：{:?}",
            layout_signature(&layout)
        );
        assert_eq!(layout.single_count, 4);
        assert_eq!(layout.groups.len() + layout.single_count as usize, 9);
        assert_eq!(layout.score, 181, "mode={mode:?}：Σ 分组分 + 散张分");
    }
}

/// 干净黄金手：加权 / 手数两种 mode 各断言一次（这手牌两者同解，
/// 因为权重表里「合并成型组」总是既提分又减手，两套序在此不冲突）。
#[test]
fn golden_clean_hand_both_modes() {
    let mut expected = vec![
        ("bomb-5".to_owned(), (5, 12), 5),
        ("bomb-5".to_owned(), (5, 13), 5),
        ("pair".to_owned(), (2, 7), 2),
        ("pair".to_owned(), (2, 9), 2),
        ("triple-pair".to_owned(), (5, 3), 5),
    ];
    expected.sort();

    for mode in [None, Some("least-hands")] {
        let engine = engine_with_mode(mode);
        let (hand, expected_score, expected_hands) = clean_hand(&engine);
        let state = with_hand(&engine, hand.clone());
        let layout = engine
            .recommend(&state, Seat(0))
            .expect("干净黄金手 recommend 不应报错");

        assert_conservation(&layout, &hand);
        assert_groups_sorted(&layout);
        assert_eq!(
            layout_signature(&layout),
            expected,
            "mode={mode:?} 实际：{:?}",
            layout_signature(&layout)
        );
        assert_eq!(layout.single_count, 4, "mode={mode:?}");
        assert_eq!(
            layout.groups.len() + layout.single_count as usize,
            expected_hands,
            "mode={mode:?}：应为 9 手"
        );
        assert_eq!(
            layout.score, expected_score,
            "mode={mode:?}：Σ 分组分 + 散张分"
        );
        assert!(!layout.has_straight_flush);
    }
}

/// 同花顺后处理必须回原手牌做花色层检测：搜索落地的 5 张可以不是同一花色，
/// 只要手牌里存在「同一点数、同一花色」的另一套选牌（含百搭补孔），该组就该升级。
#[test]
fn straight_flush_upgrades_from_suit_layer() {
    use gf_engine::patterns::card_id;
    let engine = classic_engine().expect("classic 构造失败");
    let r = &engine.ruleset;

    let mut hand: Vec<u16> = Vec::new();
    // 炸弹 4：点数 3（位 1）的 4 张最小副本（♠♠♥♥），把黑桃 3 占掉。
    for (rank, suit, copy) in [(1, 0, 0), (1, 0, 1), (1, 1, 0), (1, 1, 1)] {
        hand.push(card_id(r, rank, suit, copy));
    }
    // 黑桃 4-5-6-7：落地时按牌 id 最小取，必然先取这四张。
    for rank in 2..=5usize {
        hand.push(card_id(r, rank, 0, 0));
    }
    // 方块 3-4-5-6-7：真正能成同花的那一套。
    for rank in 1..=5usize {
        hand.push(card_id(r, rank, 2, 0));
    }

    let state = with_hand(&engine, hand.clone());
    let layout = engine
        .recommend(&state, Seat(0))
        .expect("花色层用例 recommend 不应报错");

    assert_conservation(&layout, &hand);
    let flush = layout
        .groups
        .iter()
        .find(|g| g.pattern_id == "straight-flush-5")
        .unwrap_or_else(|| panic!("应有 3-7 同花顺组，实际：{:?}", layout.groups));
    let expected: Vec<u16> = (1..=5usize).map(|rank| card_id(r, rank, 2, 0)).collect();
    assert_eq!(
        flush.cards, expected,
        "同花顺应落到方块那一套，而不是落地序取到的混花"
    );
    assert!(layout.has_straight_flush);
}

/// `score` 必须是**后处理之后**的分组分数之和（同花顺升级会把 46 抬到 55）。
#[test]
fn score_reflects_postprocessing() {
    use gf_engine::patterns::card_id;
    let engine = classic_engine().expect("classic 构造失败");
    let r = &engine.ruleset;
    // 黑桃 3-4-5-6-7 一手同花顺，另加 2 张散张。
    let mut hand: Vec<u16> = (1..=5usize).map(|rank| card_id(r, rank, 0, 0)).collect();
    hand.push(card_id(r, 8, 2, 0));
    hand.push(card_id(r, 10, 3, 0));

    let state = with_hand(&engine, hand.clone());
    let layout = engine
        .recommend(&state, Seat(0))
        .expect("同花顺计分用例 recommend 不应报错");

    assert_conservation(&layout, &hand);
    assert_eq!(
        layout.groups.len(),
        1,
        "应只保留一组同花顺，实际：{:?}",
        layout.groups
    );
    assert_eq!(layout.groups[0].pattern_id, "straight-flush-5");
    // 1 组同花顺 55 + 2 张散张 (−10 × 2) = 35；若取升级前的杂顺则是 46 − 20 = 26。
    assert_eq!(layout.score, 35);
}

/// 三带二可带**同色王对**（两张小王或两张大王；§5.13「双副牌下两张小王 / 两张大王成对
/// 亦可」）。搜索侧王只作单张 / 王炸，故王对由三带二后处理兜底：吸收后散张减 2。
///
/// 手牌 = 三张 6（点数位 4、`rankKey = 4`）+ 两张小王 + 2 张散张：
/// 三带二分 = `30 + ⌊20×3/12⌋ = 35`，散张 2 张 = −20 → 合计 15；手数 1 组 + 2 散张 = 3。
#[test]
fn triple_pair_absorbs_same_kind_joker_pair() {
    use gf_engine::patterns::{card_id, joker_id};
    for mode in [None, Some("least-hands")] {
        let engine = engine_with_mode(mode);
        let r = &engine.ruleset;
        let mut hand: Vec<u16> = vec![
            card_id(r, 4, 0, 0),
            card_id(r, 4, 1, 0),
            card_id(r, 4, 3, 0),
            joker_id(r, false, 0),
            joker_id(r, false, 1),
            card_id(r, 9, 2, 0),
            card_id(r, 11, 3, 0),
        ];
        hand.sort_unstable();

        let state = with_hand(&engine, hand.clone());
        let layout = engine
            .recommend(&state, Seat(0))
            .expect("王对用例 recommend 不应报错");

        assert_conservation(&layout, &hand);
        assert_groups_sorted(&layout);
        assert_eq!(layout.groups.len(), 1, "mode={mode:?}：{:?}", layout.groups);
        let g = &layout.groups[0];
        assert_eq!(g.pattern_id, "triple-pair", "mode={mode:?}");
        assert_eq!(g.pattern_key, (5, 4), "mode={mode:?}：只按主牌三张的点数");
        let mut expected: Vec<u16> = vec![
            card_id(r, 4, 0, 0),
            card_id(r, 4, 1, 0),
            card_id(r, 4, 3, 0),
            joker_id(r, false, 0),
            joker_id(r, false, 1),
        ];
        expected.sort_unstable();
        assert_eq!(g.cards, expected, "mode={mode:?}：两张小王应进三带二");
        assert_eq!(layout.single_count, 2, "mode={mode:?}：王对不再算散张");
        assert_eq!(layout.score, 15, "mode={mode:?}：35 + 2 × (−10)");
    }
}

/// 一大一小**不成对**：两张不同色的王在双副牌里既不是对子、也不是王炸（王炸要两张小王
/// + 两张大王共 4 张），只能各算一张散张，三张也保持裸三张。
#[test]
fn triple_pair_rejects_mixed_jokers() {
    use gf_engine::patterns::{card_id, joker_id};
    let engine = engine_with_mode(None);
    let r = &engine.ruleset;
    let mut hand: Vec<u16> = vec![
        card_id(r, 4, 0, 0),
        card_id(r, 4, 1, 0),
        card_id(r, 4, 3, 0),
        joker_id(r, false, 0),
        joker_id(r, true, 0),
        card_id(r, 9, 2, 0),
        card_id(r, 11, 3, 0),
    ];
    hand.sort_unstable();

    let state = with_hand(&engine, hand.clone());
    let layout = engine
        .recommend(&state, Seat(0))
        .expect("混王用例 recommend 不应报错");

    assert_conservation(&layout, &hand);
    assert!(
        !layout.groups.iter().any(|g| g.pattern_id == "triple-pair"),
        "一大一小不得进三带二，实际：{:?}",
        layout.groups
    );
    assert_eq!(layout.groups.len(), 1, "实际：{:?}", layout.groups);
    assert_eq!(layout.groups[0].pattern_id, "triple");
    assert_eq!(layout.single_count, 4, "两张异色王只能各算散张");
    assert_eq!(layout.score, -40, "裸三张 0 + 4 张散张 (−10 × 4)");
}
