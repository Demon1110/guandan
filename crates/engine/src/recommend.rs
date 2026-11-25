//! 组牌推荐（`recommend`，§5.11「组牌推荐」段、第二十/二十八/二十九次变更）。
//!
//! - 纯函数、只读、确定性（同输入同输出）；**不进 `stateHash`、不改 `legal_actions`、
//!   不进双端比对的判定链**；native 与 wasm 必须同解。
//! - 评分全整数（禁浮点）；权重按刻度 10 书写，引擎按原值直接比较、不再换算。
//! - 搜索为确定性束搜索，`beamWidth = 64` 写死在引擎、不进 RuleSet。

use crate::patterns::{enumerate, RankContext, Recognized};
use crate::ruleset::model::{AiConfig, RecommendMode, RecommendWeights};
use crate::ruleset::shape_plan::{BombRole, ShapePlan};
use crate::ruleset::RuleSetBody;
use std::collections::{BTreeMap, HashSet};

/// 束宽（§5.11 第 858 行）：固定常数，写死引擎、不进 RuleSet；束宽变更只影响推荐
/// 向量，不影响 `stateHash`。
pub const BEAM_WIDTH: usize = 64;

/// 推荐出的一个分组（§5.11 第 833–834 行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutGroup {
    pub pattern_id: String,
    pub pattern_key: (u32, u32),
    pub cards: Vec<u16>,
}

/// 整手拆分结果（§5.11 第 833–836 行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub groups: Vec<LayoutGroup>,
    pub score: i64,
    pub single_count: u8,
    pub straight_flush_candidates: Vec<Vec<u16>>,
    pub has_straight_flush: bool,
    pub disabled: bool,
}

/// 分组牌型族：评分键分类用，对应默认权重表的各档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupKind {
    /// 普通炸弹，张数 4–8。
    Bomb(u8),
    JokerBomb,
    StraightFlush,
    Straight,
    TriplePair,
    Plate3,
    Steel2,
    Pair,
    Single,
    /// 裸三张同点（`rank.same{3}`、无附件）：§5.11 权重表未单列键，暂按 `pair`
    /// 权重兜底；**权重键归属待上级裁决**（维持 pair=0 还是归 Unknown=0），裁决前
    /// 不自造权重键、不改评分口径。
    BareTriple,
    /// 未在权重表覆盖的形状（`suit.same` 等 v2 预留）：按 0 分兜底。
    Unknown,
}

/// 解析 `ai.recommend` 配置：返回 `(enabled, weights, mode)`；段省略 / `weights` 省略 =
/// 默认权重全上；`mode` 省略 = `weighted`（§5.11 第 865、901 行）。
pub fn resolve_config(ai: Option<&AiConfig>) -> (bool, RecommendWeights, RecommendMode) {
    match ai.and_then(|a| a.recommend.as_ref()) {
        None => (true, RecommendWeights::default(), RecommendMode::Weighted),
        Some(rec) => (rec.enabled, rec.weights.unwrap_or_default(), rec.mode),
    }
}

/// 载入期按 `patternId` 索引的形状计划（评分族分类用）。
fn plan_index(r: &RuleSetBody) -> BTreeMap<String, ShapePlan> {
    r.patterns
        .iter()
        .filter_map(|p| ShapePlan::compile(&p.params).map(|plan| (p.id.0.clone(), plan)))
        .collect()
}

fn classify(plan: &ShapePlan, role: BombRole, count_key: u32) -> GroupKind {
    match plan {
        ShapePlan::JokerOnly { .. } => GroupKind::JokerBomb,
        ShapePlan::RankContiguous { flush: true, .. } => GroupKind::StraightFlush,
        ShapePlan::RankContiguous { step: 1, .. } => GroupKind::Straight,
        ShapePlan::RankContiguous { step: 2, .. } => GroupKind::Plate3,
        ShapePlan::RankContiguous { step: 3, .. } => GroupKind::Steel2,
        ShapePlan::RankSame {
            attach: Some(_), ..
        } => GroupKind::TriplePair,
        ShapePlan::RankSame {
            count,
            attach: None,
        } => match (*count, role) {
            (1, _) => GroupKind::Single,
            (2, _) => GroupKind::Pair,
            (3, _) => GroupKind::BareTriple,
            (c @ 4..=8, _) => GroupKind::Bomb(c),
            _ => GroupKind::Unknown,
        },
        ShapePlan::SuitSame => GroupKind::Unknown,
        _ => match count_key {
            4..=8 => GroupKind::Bomb(count_key as u8),
            _ => GroupKind::Unknown,
        },
    }
}

fn is_single(kind: GroupKind) -> bool {
    matches!(kind, GroupKind::Single)
}

fn is_bomb_family(role: BombRole) -> bool {
    matches!(
        role,
        BombRole::SameRankBomb | BombRole::JokerBomb | BombRole::Flush
    )
}

/// 位次加分（§5.11 第 843–844、854–855 行）：
/// `⌊ bonus × (rankKey − 1) / (N − 1) ⌋`，`N` = 非王牌点数个数；轮子顺 `rankKey = 0`
/// 按 0 分处理（不拿加分）。全整数下取整。
fn rank_bonus(bonus: i16, rank_key: u32, n_non_joker: usize) -> i64 {
    if rank_key == 0 || n_non_joker <= 1 {
        return 0;
    }
    bonus as i64 * (rank_key as i64 - 1) / (n_non_joker as i64 - 1)
}

/// 单个分组的整数评分（§5.11 第 837–844 行）。
fn group_score(
    plan: Option<&ShapePlan>,
    rec: &Recognized,
    w: &RecommendWeights,
    n_non_joker: usize,
) -> i64 {
    let kind = match plan {
        Some(plan) => classify(plan, rec.role, rec.pattern_key.0),
        None => GroupKind::Unknown,
    };
    let rank_key = rec.pattern_key.1;
    match kind {
        GroupKind::Bomb(4) => w.bomb4 as i64,
        GroupKind::Bomb(5) => w.bomb5 as i64,
        GroupKind::Bomb(6) => w.bomb6 as i64,
        GroupKind::Bomb(7) => w.bomb7 as i64,
        GroupKind::Bomb(8) => w.bomb8 as i64,
        GroupKind::Bomb(_) => 0,
        GroupKind::JokerBomb => w.joker_bomb as i64,
        GroupKind::StraightFlush => w.straight_flush as i64,
        GroupKind::Straight => {
            w.straight_base as i64 + rank_bonus(w.straight_rank_bonus, rank_key, n_non_joker)
        }
        GroupKind::TriplePair => {
            w.triple_pair_base as i64 + rank_bonus(w.triple_pair_rank_bonus, rank_key, n_non_joker)
        }
        GroupKind::Plate3 => {
            w.plate3_base as i64 + rank_bonus(w.plate3_rank_bonus, rank_key, n_non_joker)
        }
        GroupKind::Steel2 => {
            w.steel2_base as i64 + rank_bonus(w.steel2_rank_bonus, rank_key, n_non_joker)
        }
        GroupKind::Pair | GroupKind::BareTriple => w.pair as i64,
        GroupKind::Single => w.single as i64,
        GroupKind::Unknown => 0,
    }
}

impl From<&Recognized> for LayoutGroup {
    fn from(rec: &Recognized) -> Self {
        Self {
            pattern_id: rec.pattern_id.clone(),
            pattern_key: rec.pattern_key,
            cards: rec.cards.clone(),
        }
    }
}

/// 束搜索中的中间拆分状态。
#[derive(Clone, Debug)]
struct Partial {
    groups: Vec<Recognized>,
    remaining: Vec<u16>,
    /// 已选分组累计评分（不含散张单张扣分）。
    group_score: i64,
    /// 炸弹族（含王炸、同花顺）累计张数。
    bomb_cards: usize,
    /// 枚举序兜底：同级 tie-break 时取最先枚举到的一条（§5.11 第 857 行）。
    enum_order: usize,
}

/// 分组牌面从 `remaining` 中按多重集移除（双指针；两序列均已升序）；任一牌不在
/// 剩余集合则返回 `None`。
fn remove_cards(remaining: &[u16], cards: &[u16]) -> Option<Vec<u16>> {
    let mut rem: Vec<u16> = Vec::with_capacity(remaining.len());
    let mut j = 0usize;
    for &card in remaining {
        if j < cards.len() && cards[j] == card {
            j += 1;
        } else {
            rem.push(card);
        }
    }
    (j == cards.len()).then_some(rem)
}

/// 分组序列的字典序键：`(ladder 位次降, patternKey 降, cards 升)`（tie-break 第 ④ 级）。
fn group_tuple(
    ladder: &[crate::contract::PatternId],
    rec: &Recognized,
) -> (
    std::cmp::Reverse<usize>,
    std::cmp::Reverse<u32>,
    std::cmp::Reverse<u32>,
    Vec<u16>,
) {
    let pos = ladder
        .iter()
        .position(|p| *p == crate::contract::PatternId(rec.pattern_id.clone()))
        .unwrap_or(usize::MAX);
    (
        std::cmp::Reverse(pos),
        std::cmp::Reverse(rec.pattern_key.0),
        std::cmp::Reverse(rec.pattern_key.1),
        rec.cards.clone(),
    )
}

struct Finalist<'a> {
    partial: Partial,
    ladder: &'a [crate::contract::PatternId],
    single_count: usize,
    total_score: i64,
}

impl<'a> Finalist<'a> {
    fn new(partial: Partial, ladder: &'a [crate::contract::PatternId], single_weight: i16) -> Self {
        let single_count = partial.remaining.len();
        let total_score = partial.group_score + single_count as i64 * single_weight as i64;
        Self {
            partial,
            ladder,
            single_count,
            total_score,
        }
    }
}

/// tie-break（§5.11 第 856–857 行，固定双端同序）：
/// 分数高 → 散张少 → 炸弹族总张数多 → 分组序列字典序小 → 枚举序最先。
fn finalist_better(a: &Finalist, b: &Finalist) -> bool {
    if a.total_score != b.total_score {
        return a.total_score > b.total_score;
    }
    if a.single_count != b.single_count {
        return a.single_count < b.single_count;
    }
    if a.partial.bomb_cards != b.partial.bomb_cards {
        return a.partial.bomb_cards > b.partial.bomb_cards;
    }
    let cmp = a
        .partial
        .groups
        .iter()
        .map(|g| group_tuple(a.ladder, g))
        .cmp(b.partial.groups.iter().map(|g| group_tuple(b.ladder, g)));
    match cmp {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Greater => false,
        std::cmp::Ordering::Equal => a.partial.enum_order < b.partial.enum_order,
    }
}

/// 束内剪枝序：中间态只看已选分组累计分与炸弹张数；散张数要到终态才定。
fn beam_cmp(a: &Partial, b: &Partial) -> std::cmp::Ordering {
    match a.group_score.cmp(&b.group_score) {
        std::cmp::Ordering::Equal => {}
        other => return other.reverse(),
    }
    match a.bomb_cards.cmp(&b.bomb_cards) {
        std::cmp::Ordering::Equal => {}
        other => return other.reverse(),
    }
    a.enum_order.cmp(&b.enum_order)
}

fn partial_better(a: &Partial, b: &Partial) -> bool {
    beam_cmp(a, b) == std::cmp::Ordering::Less
}

/// 推荐主入口：对给定手牌做确定性束搜索拆分。纯函数、只读、不回写状态。
pub(crate) fn recommend_layout(
    ruleset: &RuleSetBody,
    ladder: &[crate::contract::PatternId],
    ctx: &RankContext,
    hand: &[u16],
) -> Layout {
    let (enabled, weights, _mode) = resolve_config(ruleset.ai.as_ref());
    if !enabled {
        return Layout {
            groups: Vec::new(),
            score: 0,
            single_count: 0,
            straight_flush_candidates: Vec::new(),
            has_straight_flush: false,
            disabled: true,
        };
    }

    let plans = plan_index(ruleset);
    let n_non_joker = ruleset.deck.ranks.len();

    // 整手枚举一次：同花顺候选独立于最优拆分（§5.11 第 835–836 行），含百搭方案、
    // 按 cards 升序去重；最优拆分没走同花顺也要列出。
    let all: Vec<Recognized> = enumerate(ladder, ruleset, ctx, hand, false);
    let mut flush_seen: HashSet<Vec<u16>> = HashSet::new();
    let mut flush_candidates: Vec<Vec<u16>> = Vec::new();
    for rec in &all {
        if rec.role == BombRole::Flush && flush_seen.insert(rec.cards.clone()) {
            flush_candidates.push(rec.cards.clone());
        }
    }
    flush_candidates.sort();
    let has_straight_flush = !flush_candidates.is_empty();

    // 拆分按「剩余手牌」枚举：三张同点可降成对子 + 散张，百搭也会随已消耗组重新配
    // 位；整手候选池会丢这两类组合，属候选生成缺陷，不允许池化近似。剩余集合签名
    // 记忆化由私有分解器 `Decompose` 持有（§5.11 第 859–863 行预算 ≤2ms）；候选解析
    // 仍走共享 `enumerate`，legal 枚举路径不变。
    let mut decompose = crate::recommend_decompose::Decompose::new(ladder, ruleset, ctx);

    let initial = Partial {
        groups: Vec::new(),
        remaining: {
            let mut h: Vec<u16> = hand.to_vec();
            h.sort_unstable();
            h
        },
        group_score: 0,
        bomb_cards: 0,
        enum_order: 0,
    };

    let mut beam: Vec<Partial> = vec![initial];
    let mut best: Option<Finalist> = None;

    while !beam.is_empty() {
        // 当前束内每个中间态本身就是一条终态拆分（剩余全算散张单张）。
        for partial in &beam {
            let cand = Finalist::new(partial.clone(), ladder, weights.single);
            match &best {
                Some(cur) if !finalist_better(&cand, cur) => {}
                _ => best = Some(cand),
            }
        }

        let mut next: Vec<Partial> = Vec::new();
        for (pi, partial) in beam.iter().enumerate() {
            let candidates = decompose.candidates(ladder, ruleset, ctx, &partial.remaining);
            for (ci, rec) in candidates.iter().enumerate() {
                let plan = plans.get(&rec.pattern_id);
                let kind = plan
                    .map(|p| classify(p, rec.role, rec.pattern_key.0))
                    .unwrap_or(GroupKind::Unknown);
                if is_single(kind) {
                    continue;
                }
                let Some(remaining) = remove_cards(&partial.remaining, &rec.cards) else {
                    continue;
                };
                let gscore = group_score(plan, rec, &weights, n_non_joker);
                let bomb_cards = partial.bomb_cards
                    + if is_bomb_family(rec.role) {
                        rec.cards.len()
                    } else {
                        0
                    };
                let groups = {
                    let mut g = partial.groups.clone();
                    g.push(rec.clone());
                    g
                };
                next.push(Partial {
                    groups,
                    remaining,
                    group_score: partial.group_score + gscore,
                    bomb_cards,
                    enum_order: pi * candidates.len() + ci,
                });
            }
        }

        // 同一剩余牌集合的不同拆法只留 tie-break 最优一条（§5.11 第 859 行剪枝），
        // 既收敛束内重复，也减少按剩余手牌重复枚举的次数。
        let mut seen: BTreeMap<Vec<u16>, Partial> = BTreeMap::new();
        for p in next {
            match seen.get(&p.remaining) {
                Some(cur) if !partial_better(&p, cur) => {}
                _ => {
                    seen.insert(p.remaining.clone(), p);
                }
            }
        }
        let mut next: Vec<Partial> = seen.into_values().collect();
        next.sort_by(beam_cmp);
        next.truncate(BEAM_WIDTH);
        beam = next;
    }

    let best = match best {
        Some(b) => b,
        None => {
            return Layout {
                groups: Vec::new(),
                score: 0,
                single_count: 0,
                straight_flush_candidates: flush_candidates,
                has_straight_flush,
                disabled: false,
            };
        }
    };

    let mut groups: Vec<LayoutGroup> = best.partial.groups.iter().map(LayoutGroup::from).collect();
    // `groups` 排序键：`(patternId 字典序升, patternKey 降, cards 升)`。
    groups.sort_by(|a, b| {
        (&a.pattern_id, std::cmp::Reverse(a.pattern_key), &a.cards).cmp(&(
            &b.pattern_id,
            std::cmp::Reverse(b.pattern_key),
            &b.cards,
        ))
    });

    Layout {
        groups,
        score: best.total_score,
        single_count: best.single_count as u8,
        straight_flush_candidates: flush_candidates,
        has_straight_flush,
        disabled: false,
    }
}

/// 组牌推荐向量的确定性摘要（native/wasm 同解 CI 用）：对 `Layout` 全字段按固定
/// 顺序做 FNV-1a 折叠；序列固定、无 `HashMap` 迭代序依赖，双端必须同值。
pub(crate) fn layout_digest(layout: &Layout) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x1000_0000_01b3;
    let mut h = FNV_OFFSET;
    let mut fold = |byte: u8| {
        h ^= byte as u64;
        h = h.wrapping_mul(FNV_PRIME);
    };

    fold(layout.disabled as u8);
    fold(layout.has_straight_flush as u8);
    fold(layout.single_count);
    for b in layout.score.to_le_bytes() {
        fold(b);
    }
    for b in (layout.groups.len() as u32).to_le_bytes() {
        fold(b);
    }
    for g in &layout.groups {
        for &b in g.pattern_id.as_bytes() {
            fold(b);
        }
        for b in g.pattern_key.0.to_le_bytes() {
            fold(b);
        }
        for b in g.pattern_key.1.to_le_bytes() {
            fold(b);
        }
        for &c in &g.cards {
            for b in c.to_le_bytes() {
                fold(b);
            }
        }
    }
    for b in (layout.straight_flush_candidates.len() as u32).to_le_bytes() {
        fold(b);
    }
    for cand in &layout.straight_flush_candidates {
        for &c in cand {
            for b in c.to_le_bytes() {
                fold(b);
            }
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ruleset::model::RecommendWeights;

    fn weights() -> RecommendWeights {
        RecommendWeights::default()
    }

    #[test]
    fn rank_bonus_wheel_zero() {
        // 轮子顺 rankKey=0：不拿加分（§5.11 第 844、855 行）。
        assert_eq!(rank_bonus(20, 0, 13), 0);
    }

    #[test]
    fn rank_bonus_scales_with_rank_key() {
        // N=13：rankKey=1 加分 0；rankKey=13 加分 ⌊20×12/12⌋=20。
        let w = weights();
        assert_eq!(rank_bonus(w.straight_rank_bonus, 1, 13), 0);
        assert_eq!(rank_bonus(w.straight_rank_bonus, 13, 13), 20);
        // 中间档下取整：rankKey=7 → ⌊20×6/12⌋=10。
        assert_eq!(rank_bonus(w.straight_rank_bonus, 7, 13), 10);
    }

    #[test]
    fn flush_between_bomb5_and_bomb6() {
        // 第二十八次变更 ③：50 < 55 < 60，同花顺严格落在 5 炸与 6 炸之间。
        let w = weights();
        let b5 = w.bomb5 as i64;
        let sf = w.straight_flush as i64;
        let b6 = w.bomb6 as i64;
        assert!(b5 < sf);
        assert!(sf < b6);
        assert_eq!((w.bomb5, w.straight_flush, w.bomb6), (50, 55, 60));
    }

    #[test]
    fn default_directions() {
        let w = weights();
        // 单张负分、对子零分、炸弹越大越高、王炸最高。
        assert!(w.single < 0);
        assert_eq!(w.pair, 0);
        assert!(w.bomb4 < w.bomb5 && w.bomb5 < w.bomb6 && w.bomb6 < w.bomb7 && w.bomb7 < w.bomb8);
        assert!(w.joker_bomb > w.bomb8);
    }

    #[test]
    fn resolve_config_defaults_when_omitted() {
        let (enabled, w, mode) = resolve_config(None);
        assert!(enabled);
        assert_eq!(w, RecommendWeights::default());
        assert_eq!(mode, RecommendMode::Weighted);
    }
}
