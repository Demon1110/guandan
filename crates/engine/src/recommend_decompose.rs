//! `recommend` 私有分解器（§5.11「组牌推荐」旁路，④ 性能批）。
//!
//! 边界纪律：
//! - 本模块**只被 `recommend` 束搜索调用**；`legal_actions` / 共享 `enumerate` /
//!   `recognize` 的判定链一行不改，legal 枚举结果不变。
//! - 候选的牌型解析、逢人配选解、`patternKey` / 角色判定仍委托共享
//!   [`crate::patterns::recognize`]；本模块只按 rank 桶 / 合法窗口 / 百搭孔位**直取
//!   候选牌面**，再交给 `recognize` 校验，不复制选解三档序与牌力序。
//! - 两层记忆化：① 剩余手牌多重集签名 → 候选列表；② 候选组合牌面 → `recognize`
//!   结果（同一札在不同剩余集合间复用识别结论）。
//! - 纯函数、只读、确定性；不进 `stateHash`、不进双端判定链。

use crate::contract::PatternId;
use crate::patterns::{recognize, wildcard_ids, RankContext, Recognized};
use crate::ruleset::params::PrimitiveParams;
use crate::ruleset::shape_plan::ShapePlan;
use crate::ruleset::RuleSetBody;
use std::collections::{BTreeMap, HashSet};

/// 一次 `recommend` 调用内的分解器状态。
pub(crate) struct Decompose {
    /// 载入期预编译的牌型计划，按 ladder 位次降序（与共享 `enumerate` 同序）。
    plans: Vec<ShapePlan>,
    /// 逢人配牌 id 集合（只依赖 ruleset/ctx，构造期算一次，不随剩余集合重建）。
    wild: HashSet<u16>,
    /// 多重集签名（升序牌 id 序列）→ 该剩余集合上的非单张候选。
    cache: BTreeMap<Vec<u16>, Vec<Recognized>>,
    /// 候选组合牌面 → 共享 `recognize` 结论（`None` = 该札不成型，同样缓存避免重复识别）。
    combo_cache: BTreeMap<Vec<u16>, Option<Recognized>>,
    /// 跨集合去重的候选组合牌面（识别重复度拆账用）。
    combos: HashSet<Vec<u16>>,
    pub stats: Stats,
}

/// 束内与记忆化统计（只服务性能拆账，不进 `stateHash`、不影响判定）。
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct Stats {
    /// 束内处理的中间态（节点）总数。
    pub beam_nodes: usize,
    /// 签名命中（复用已有候选列表）次数。
    pub cache_hits: usize,
    /// 签名未命中、需要直取候选的剩余集合数（≈ 唯一剩余集合数）。
    pub cache_misses: usize,
    /// 候选组合命中识别缓存的次数。
    pub combo_hits: usize,
    /// 候选组合未命中、调用共享 `recognize` 的次数。
    pub combo_misses: usize,
    /// 缓存条目里累计的候选条数。
    pub candidates: usize,
    /// 跨剩余集合去重后的候选组合数。
    pub unique_combos: usize,
}

impl Decompose {
    pub(crate) fn new(ladder: &[PatternId], r: &RuleSetBody, ctx: &RankContext) -> Self {
        let plans = compile_plans(ladder, r);
        let wild = wildcard_ids(r, ctx);
        Self {
            plans,
            wild,
            cache: BTreeMap::new(),
            combo_cache: BTreeMap::new(),
            combos: HashSet::new(),
            stats: Stats::default(),
        }
    }

    /// 取某剩余手牌上的非单张候选；命中签名缓存则复用，未命中走私有直取 + 共享识别。
    pub(crate) fn candidates(
        &mut self,
        ladder: &[PatternId],
        ruleset: &RuleSetBody,
        ctx: &RankContext,
        hand: &[u16],
    ) -> &[Recognized] {
        self.stats.beam_nodes += 1;

        let mut key: Vec<u16> = hand.to_vec();
        key.sort_unstable();

        if self.cache.contains_key(&key) {
            let v = self.cache.get(&key).expect("contains_key 命中");
            self.stats.cache_hits += 1;
            self.stats.candidates += v.len();
            return v.as_slice();
        }

        let produced = build(self, ladder, ruleset, ctx, hand);
        for rec in &produced {
            self.combos.insert(rec.cards.clone());
        }
        self.stats.cache_misses += 1;
        self.stats.candidates += produced.len();
        self.stats.unique_combos = self.combos.len();
        self.cache.insert(key.clone(), produced);
        self.cache.get(&key).expect("刚写入的签名条目").as_slice()
    }
}

// ---------- 私有桶索引（单遍扫描，不重建两层 BTreeMap） ----------

struct Buckets {
    /// 点数 → 真牌（升序）。
    base: Vec<Vec<u16>>,
    /// 花色 → 点数 → 真牌（同花顺枚举用）。
    by_suit: Vec<Vec<Vec<u16>>>,
    wild: Vec<u16>,
    little: Vec<u16>,
    big: Vec<u16>,
}

impl Buckets {
    fn build(r: &RuleSetBody, wild: &HashSet<u16>, hand: &[u16]) -> Self {
        let copies = r.deck.copies as usize;
        let rank_count = r.deck.ranks.len();
        let suit_count = r.deck.suits.len();
        let base_total = rank_count * suit_count * copies;

        let mut base = vec![Vec::new(); rank_count];
        let mut by_suit = vec![vec![Vec::new(); rank_count]; suit_count];
        let mut wild_cards = Vec::new();
        let mut little = Vec::new();
        let mut big = Vec::new();

        for &card in hand {
            if wild.contains(&card) {
                wild_cards.push(card);
                continue;
            }
            if card as usize >= base_total {
                let slot = (card as usize - base_total) / copies;
                if slot == 0 {
                    little.push(card);
                } else {
                    big.push(card);
                }
                continue;
            }
            let b = card as usize / copies;
            let rank = b % rank_count;
            let suit = b / rank_count;
            base[rank].push(card);
            by_suit[suit][rank].push(card);
        }
        for bucket in &mut base {
            bucket.sort_unstable();
        }
        for suit in &mut by_suit {
            for bucket in suit {
                bucket.sort_unstable();
            }
        }
        wild_cards.sort_unstable();
        little.sort_unstable();
        big.sort_unstable();

        Self {
            base,
            by_suit,
            wild: wild_cards,
            little,
            big,
        }
    }

    fn base_empty(&self) -> bool {
        self.base.iter().all(Vec::is_empty)
    }
}

/// 合法滑动窗口（含轮子顺 A-2-3-4-5 特例），与共享枚举口径一致。
fn all_windows(rank_count: usize, length: usize) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = (0..=rank_count.saturating_sub(length))
        .map(|s| (s..s + length).collect())
        .collect();
    if length == 5 {
        out.push(vec![rank_count - 1, 0, 1, 2, 3]);
    }
    out
}

/// 每个点数取前一张（顺子 / 同花顺）。
fn pick_one(buckets: &[Vec<u16>], window: &[usize]) -> Vec<u16> {
    let mut cards = Vec::new();
    for &rank in window {
        if let Some(card) = buckets[rank].first() {
            cards.push(*card);
        }
    }
    cards
}

/// 每点数取前 `per_group` 张，缺孔用百搭前缀补齐（连对 / 钢板）。
fn consecutive_cards(
    base: &[Vec<u16>],
    wild: &[u16],
    window: &[usize],
    per_group: usize,
) -> Vec<u16> {
    let mut cards = Vec::new();
    let mut used = 0usize;
    for &rank in window {
        let take = per_group.min(base[rank].len());
        cards.extend_from_slice(&base[rank][..take]);
        let need = per_group - take;
        cards.extend_from_slice(&wild[used..used + need]);
        used += need;
    }
    cards
}

/// 窗口内每个点数的缺孔总和 ≤ 百搭数。
fn window_wildcard_fits(
    base: &[Vec<u16>],
    window: &[usize],
    per_group: usize,
    wild: usize,
) -> bool {
    let mut deficit = 0usize;
    for &rank in window {
        let t = base[rank].len();
        deficit += per_group.saturating_sub(t);
    }
    deficit <= wild
}

fn flush_allowed(r: &RuleSetBody) -> bool {
    r.wildcard
        .as_ref()
        .map(|w| w.in_straight_flush)
        .unwrap_or(false)
}

// ---------- 候选直取 ----------

/// 载入期编译一次牌型计划，按 ladder 位次降序（与共享 `enumerate` 同序）。
fn compile_plans(ladder: &[PatternId], r: &RuleSetBody) -> Vec<ShapePlan> {
    let mut defs: Vec<(ShapePlan, usize)> = r
        .patterns
        .iter()
        .filter(|p| !matches!(p.params, PrimitiveParams::Reserved { .. }))
        .filter_map(|p| {
            let pos = ladder.iter().position(|x| *x == p.id).unwrap_or(usize::MAX);
            ShapePlan::compile(&p.params).map(|plan| (plan, pos))
        })
        .collect();
    defs.sort_by_key(|(_, pos)| std::cmp::Reverse(*pos));
    defs.into_iter().map(|(plan, _)| plan).collect()
}

fn build(
    decomp: &mut Decompose,
    ladder: &[PatternId],
    r: &RuleSetBody,
    ctx: &RankContext,
    hand: &[u16],
) -> Vec<Recognized> {
    let buckets = Buckets::build(r, &decomp.wild, hand);
    let mut out: Vec<Recognized> = Vec::new();
    let mut emitted: HashSet<Vec<u16>> = HashSet::new();

    let mut recognize_cached = |cards: &[u16]| -> Option<Recognized> {
        match decomp.combo_cache.get(cards) {
            Some(opt) => {
                decomp.stats.combo_hits += 1;
                opt.clone()
            }
            None => {
                decomp.stats.combo_misses += 1;
                let rec = recognize(ladder, r, ctx, cards);
                decomp.combo_cache.insert(cards.to_vec(), rec.clone());
                rec
            }
        }
    };

    let mut push = |mut cards: Vec<u16>| {
        cards.sort_unstable();
        if emitted.contains(&cards) {
            return;
        }
        match recognize_cached(&cards) {
            Some(rec) => {
                emitted.insert(rec.cards.clone());
                out.push(rec);
            }
            // 不成型也记录牌面，避免后续集合重复识别。
            None => {
                emitted.insert(cards);
            }
        }
    };

    // 整手全百搭：独立分支按张数输出，与共享枚举口径一致。
    if buckets.base_empty()
        && buckets.little.is_empty()
        && buckets.big.is_empty()
        && !buckets.wild.is_empty()
    {
        for n in 1..=buckets.wild.len() {
            push(buckets.wild[..n].to_vec());
        }
        return out;
    }

    let rc = r.deck.ranks.len();

    for plan in &decomp.plans {
        match plan {
            ShapePlan::RankSame {
                count: 1,
                attach: None,
            } => {
                // 单张在束搜索里按散张处理，候选直取跳过（与 `enumerate(include_single=false)` 一致）。
            }
            ShapePlan::RankSame {
                count,
                attach: None,
            } => {
                same_rank(&buckets, *count as usize, rc, &mut push);
            }
            ShapePlan::RankSame {
                count: 3,
                attach: Some(_),
            } => {
                triple_pair(&buckets, rc, &mut push);
            }
            ShapePlan::RankContiguous {
                steps,
                step: 1,
                flush,
                ..
            } => {
                straight(
                    &buckets,
                    *steps as usize,
                    *flush,
                    flush_allowed(r),
                    &mut push,
                );
            }
            ShapePlan::RankContiguous { steps, step, .. } => {
                consecutive(&buckets, *steps as usize, *step as usize, rc, &mut push);
            }
            ShapePlan::JokerOnly { big: 2, small: 2 } => {
                joker_bomb(&buckets, &mut push);
            }
            // `suit.same`、附件非三带二组合、其它 joker.only 张数：v1 未落地，不发候选。
            _ => {}
        }
    }

    out
}

fn same_rank(buckets: &Buckets, count: usize, rc: usize, push: &mut impl FnMut(Vec<u16>)) {
    for rank in 0..rc {
        let bucket = &buckets.base[rank];
        if bucket.is_empty() {
            continue;
        }
        let need = count.saturating_sub(bucket.len());
        if need > buckets.wild.len() {
            continue;
        }
        let take = count.min(bucket.len());
        let mut cards = bucket[..take].to_vec();
        cards.extend_from_slice(&buckets.wild[..need]);
        push(cards);
    }
}

fn straight(
    buckets: &Buckets,
    length: usize,
    flush: bool,
    flush_ok: bool,
    push: &mut impl FnMut(Vec<u16>),
) {
    let rc = buckets.base.len();
    let sc = buckets.by_suit.len();
    if flush {
        for suit in 0..sc {
            let suit_buckets = &buckets.by_suit[suit];
            if suit_buckets.iter().all(Vec::is_empty) {
                continue;
            }
            if !flush_ok && !buckets.wild.is_empty() {
                continue;
            }
            for window in all_windows(rc, length) {
                let missing: Vec<usize> = window
                    .iter()
                    .copied()
                    .filter(|p| suit_buckets[*p].is_empty())
                    .collect();
                if missing.len() > buckets.wild.len() {
                    continue;
                }
                let mut cards = pick_one(suit_buckets, &window);
                cards.extend_from_slice(&buckets.wild[..missing.len()]);
                push(cards);
            }
        }
    } else {
        for window in all_windows(rc, length) {
            let missing: Vec<usize> = window
                .iter()
                .copied()
                .filter(|p| buckets.base[*p].is_empty())
                .collect();
            if missing.len() > buckets.wild.len() {
                continue;
            }
            let mut cards = pick_one(&buckets.base, &window);
            cards.extend_from_slice(&buckets.wild[..missing.len()]);
            push(cards);
        }
    }
}

fn consecutive(
    buckets: &Buckets,
    groups: usize,
    per_group: usize,
    rc: usize,
    push: &mut impl FnMut(Vec<u16>),
) {
    for window in all_windows(rc, groups) {
        if !window_wildcard_fits(&buckets.base, &window, per_group, buckets.wild.len()) {
            continue;
        }
        let cards = consecutive_cards(&buckets.base, &buckets.wild, &window, per_group);
        push(cards);
    }
}

fn triple_pair(buckets: &Buckets, rc: usize, push: &mut impl FnMut(Vec<u16>)) {
    let joker_pairs: Vec<Vec<u16>> = [&buckets.little, &buckets.big]
        .iter()
        .filter(|j| j.len() == 2)
        .map(|j| j[..2].to_vec())
        .collect();

    for main in 0..rc {
        let main_bucket = &buckets.base[main];
        if main_bucket.is_empty() {
            continue;
        }
        let real_main = main_bucket.len().min(3);
        let need_main = 3 - real_main;

        for pair in 0..rc {
            if pair == main {
                continue;
            }
            let pair_bucket = &buckets.base[pair];
            let take_pair = pair_bucket.len().min(2);
            let need_pair = 2 - take_pair;
            if need_main + need_pair > buckets.wild.len() {
                continue;
            }
            let mut cards = main_bucket[..real_main].to_vec();
            cards.extend_from_slice(&buckets.wild[..need_main]);
            cards.extend_from_slice(&pair_bucket[..take_pair]);
            cards.extend_from_slice(&buckets.wild[need_main..need_main + need_pair]);
            push(cards);
        }

        for jp in &joker_pairs {
            if need_main > buckets.wild.len() {
                continue;
            }
            let mut cards = main_bucket[..real_main].to_vec();
            cards.extend_from_slice(&buckets.wild[..need_main]);
            cards.extend_from_slice(jp);
            push(cards);
        }
    }
}

fn joker_bomb(buckets: &Buckets, push: &mut impl FnMut(Vec<u16>)) {
    if buckets.little.len() < 2 || buckets.big.len() < 2 {
        return;
    }
    let mut cards = Vec::new();
    cards.extend_from_slice(&buckets.little[..2]);
    cards.extend_from_slice(&buckets.big[..2]);
    push(cards);
}

#[cfg(test)]
pub(crate) fn debug_list(
    ladder: &[PatternId],
    r: &RuleSetBody,
    ctx: &RankContext,
    hand: &[u16],
) -> Vec<Recognized> {
    let mut decomp = Decompose::new(ladder, r, ctx);
    build(&mut decomp, ladder, r, ctx, hand)
}
