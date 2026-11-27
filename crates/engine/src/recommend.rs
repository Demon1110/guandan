//! 组牌推荐（`recommend`，§5.11「组牌推荐」段、第三十七次变更）。
//!
//! - 纯函数、只读、确定性（同输入同输出）；**不进 `stateHash`、不改 `legal_actions`、
//!   不进双端比对的判定链**；native 与 wasm 必须同解。
//! - 评分全整数（禁浮点）；权重按刻度 10 书写，引擎按原值直接比较、不再换算。
//! - 搜索为计数向量精确 DFS + 调用内局部记忆化 + 最低位锚定；旧束搜索（`beamWidth`）
//!   已作废。搜索闭集 8 类，`triple-pair` / `straight-flush-5` 走两条后处理。
//! - 三带二的附件除真牌对子外，也接受**剩余的两张同色王**（§5.13）；一大一小不成对。
//! - 同花顺候选只跑「花色 × 窗口」这一支（[`crate::patterns::straight_flush_candidates`]），
//!   不再整手 `enumerate`。
//! - `score` 是**后处理之后**的实际分组分数之和（+ 散张），不是搜索中间值。

use crate::patterns::{recognize, RankContext};
use crate::recommend_key::KeyCounts;
use crate::ruleset::model::{AiConfig, RecommendMode, RecommendWeights};
use crate::ruleset::shape_plan::{BombRole, ShapePlan};
use crate::ruleset::RuleSetBody;
use std::collections::{BTreeMap, HashSet};

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

/// 搜索闭集牌型族（§5.11 第 842 行）。`TriplePair` 仅在后处理吸收后出现，不进搜索。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Single,
    Pair,
    Triple,
    Bomb(u8),
    Straight,
    Plate3,
    Steel2,
    JokerBomb,
    TriplePair,
}

/// 载入期从已声明牌型裁出的搜索闭集条目。
#[derive(Clone)]
struct Closure {
    pattern_id: String,
    family: Family,
    /// 同点组张数 / 连续组每点张数。
    count: u8,
    /// 连续组窗口点数个数（同点组为 0）。
    steps: u8,
}

/// 解析 `ai.recommend` 配置：返回 `(enabled, weights, mode)`；段省略 / `weights` 省略 =
/// 默认权重全上；`mode` 省略 = `weighted`（§5.11 第 865、901 行）。
pub fn resolve_config(ai: Option<&AiConfig>) -> (bool, RecommendWeights, RecommendMode) {
    match ai.and_then(|a| a.recommend.as_ref()) {
        None => (true, RecommendWeights::default(), RecommendMode::Weighted),
        Some(rec) => (rec.enabled, rec.weights.unwrap_or_default(), rec.mode),
    }
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

/// 从规则集声明牌型裁出搜索闭集。`triple-pair`（`attach`）与 `straight-flush-5`
/// （`flush: true`）不进搜索，留给后处理。
fn build_closure(r: &RuleSetBody) -> Vec<Closure> {
    let mut out = Vec::new();
    for p in &r.patterns {
        let Some(plan) = ShapePlan::compile(&p.params) else {
            continue;
        };
        match plan {
            ShapePlan::RankSame {
                count,
                attach: None,
            } => {
                let family = match count {
                    1 => Family::Single,
                    2 => Family::Pair,
                    3 => Family::Triple,
                    c @ 4..=8 => Family::Bomb(c),
                    _ => continue,
                };
                out.push(Closure {
                    pattern_id: p.id.0.clone(),
                    family,
                    count,
                    steps: 0,
                });
            }
            ShapePlan::RankSame {
                attach: Some(_), ..
            } => continue,
            ShapePlan::RankContiguous {
                steps, step, flush, ..
            } if !flush => {
                let family = match step {
                    1 => Family::Straight,
                    2 => Family::Plate3,
                    3 => Family::Steel2,
                    _ => continue,
                };
                out.push(Closure {
                    pattern_id: p.id.0.clone(),
                    family,
                    count: step,
                    steps,
                });
            }
            ShapePlan::RankContiguous { flush: true, .. } => continue,
            ShapePlan::JokerOnly { big: 2, small: 2 } => out.push(Closure {
                pattern_id: p.id.0.clone(),
                family: Family::JokerBomb,
                count: 4,
                steps: 0,
            }),
            _ => continue,
        }
    }
    out
}

/// 三带二在规则集中的 patternId（后处理吸收用）。
fn triple_pair_id(r: &RuleSetBody) -> String {
    r.patterns
        .iter()
        .find(|p| {
            matches!(
                ShapePlan::compile(&p.params),
                Some(ShapePlan::RankSame {
                    count: 3,
                    attach: Some(_)
                })
            )
        })
        .map(|p| p.id.0.clone())
        .unwrap_or_else(|| "triple-pair".to_owned())
}

/// 记忆化首组选择：只存标量与首组描述，不存整条 `groups` 序列。
#[derive(Clone, Debug)]
struct Choice {
    family: Family,
    pattern_id: String,
    count_key: u32,
    rank_key: u32,
    ranks: Vec<u8>,
    score: i64,
}

/// 一个剩余状态的最优子结构：累计分、手数、首组选择。
#[derive(Clone, Debug)]
struct Opt {
    score: i64,
    hands: u32,
    first: Option<Box<Choice>>,
}

/// 开放寻址记忆化表（u64 键 → 标量 + 首组索引），线性探测、按需扩容；不用 `HashMap`。
struct Memo {
    keys: Vec<u64>,
    vals: Vec<Opt>,
    used: Vec<bool>,
    cap: usize,
    len: usize,
}

impl Memo {
    fn new(cap: usize) -> Self {
        let cap = cap.max(16).next_power_of_two();
        Self {
            keys: vec![0; cap],
            vals: vec![
                Opt {
                    score: 0,
                    hands: 0,
                    first: None,
                };
                cap
            ],
            used: vec![false; cap],
            cap,
            len: 0,
        }
    }

    fn hash(key: u64) -> u64 {
        // splitmix64 低位混淆，开放寻址探测用。
        let mut z = key.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn get(&self, key: u64) -> Option<&Opt> {
        let mut idx = (Self::hash(key) as usize) & (self.cap - 1);
        for _ in 0..self.cap {
            if !self.used[idx] {
                return None;
            }
            if self.keys[idx] == key {
                return Some(&self.vals[idx]);
            }
            idx = (idx + 1) & (self.cap - 1);
        }
        None
    }

    fn insert(&mut self, key: u64, opt: Opt) {
        // 载荷超过 1/2 先扩容，避免探测表塞满后退化成 O(cap) 并静默丢条目。
        if (self.len + 1) * 2 > self.cap {
            self.grow();
        }
        self.insert_raw(key, opt);
    }

    fn insert_raw(&mut self, key: u64, opt: Opt) {
        let mut idx = (Self::hash(key) as usize) & (self.cap - 1);
        for _ in 0..self.cap {
            if !self.used[idx] {
                self.used[idx] = true;
                self.keys[idx] = key;
                self.vals[idx] = opt;
                self.len += 1;
                return;
            }
            if self.keys[idx] == key {
                self.vals[idx] = opt;
                return;
            }
            idx = (idx + 1) & (self.cap - 1);
        }
        // 理论不可达（载荷 < cap 且已扩容）；真到这里说明扩容逻辑被改坏。
        debug_assert!(false, "记忆化表探测满且插入失败");
    }

    fn grow(&mut self) {
        let old: Vec<(u64, Opt)> = self
            .used
            .iter()
            .enumerate()
            .filter(|(_, used)| **used)
            .map(|(i, _)| (self.keys[i], self.vals[i].clone()))
            .collect();
        self.cap *= 2;
        self.keys = vec![0; self.cap];
        self.vals = vec![
            Opt {
                score: 0,
                hands: 0,
                first: None,
            };
            self.cap
        ];
        self.used = vec![false; self.cap];
        self.len = 0;
        for (k, v) in old {
            self.insert_raw(k, v);
        }
    }
}

/// 搜索的不变参数（与记忆化表分开持有，避免递归时借用冲突、也不必每层克隆闭集）。
struct SearchParams<'a> {
    ruleset: &'a RuleSetBody,
    ctx: &'a RankContext,
    weights: &'a RecommendWeights,
    mode: RecommendMode,
    closure: &'a [Closure],
    n_non_joker: usize,
}

impl SearchParams<'_> {
    fn group_score(&self, family: Family, rank_key: u32) -> i64 {
        let w = self.weights;
        match family {
            Family::Bomb(4) => w.bomb4 as i64,
            Family::Bomb(5) => w.bomb5 as i64,
            Family::Bomb(6) => w.bomb6 as i64,
            Family::Bomb(7) => w.bomb7 as i64,
            Family::Bomb(8) => w.bomb8 as i64,
            Family::Bomb(_) => 0,
            Family::JokerBomb => w.joker_bomb as i64,
            Family::Straight => {
                w.straight_base as i64
                    + rank_bonus(w.straight_rank_bonus, rank_key, self.n_non_joker)
            }
            Family::Plate3 => {
                w.plate3_base as i64 + rank_bonus(w.plate3_rank_bonus, rank_key, self.n_non_joker)
            }
            Family::Steel2 => {
                w.steel2_base as i64 + rank_bonus(w.steel2_rank_bonus, rank_key, self.n_non_joker)
            }
            // 裸三张同点：权重表未单列键（§5.11 无 `triple` 档），暂按 `pair` 兜底。
            // **权重键归属待上级裁决**，裁决前不自造权重键、不改评分口径。
            Family::Triple | Family::Pair => w.pair as i64,
            Family::TriplePair => {
                w.triple_pair_base as i64
                    + rank_bonus(w.triple_pair_rank_bonus, rank_key, self.n_non_joker)
            }
            Family::Single => w.single as i64,
        }
    }

    /// `a` 是否严格优于 `b`（tie-break 三级；相等不替换，即取枚举顺序最先者）。
    fn strictly_better(&self, a: &Opt, b: &Opt) -> bool {
        match self.mode {
            RecommendMode::Weighted => {
                if a.score != b.score {
                    return a.score > b.score;
                }
                if a.hands != b.hands {
                    return a.hands < b.hands;
                }
                false
            }
            RecommendMode::LeastHands => {
                if a.hands != b.hands {
                    return a.hands < b.hands;
                }
                if a.score != b.score {
                    return a.score > b.score;
                }
                false
            }
        }
    }

    fn rank_key_same(&self, rank: usize) -> u32 {
        self.ctx.rank_key(self.ruleset, rank)
    }

    fn rank_key_natural(&self, top: usize) -> u32 {
        self.ctx.natural_rank_key(self.ruleset, top)
    }

    fn keep_best(&self, best: &mut Option<Opt>, cand: Opt) {
        match best {
            Some(cur) if !self.strictly_better(&cand, cur) => {}
            _ => *best = Some(cand),
        }
    }
}

/// 计数向量精确 DFS：每步只枚举「包含最小非空点数」的组，同状态只算一次。
fn solve(params: &SearchParams, memo: &mut Memo, counts: &KeyCounts) -> Opt {
    if counts.is_empty() {
        return Opt {
            score: 0,
            hands: 0,
            first: None,
        };
    }
    let key = counts.pack();
    if let Some(hit) = memo.get(key).cloned() {
        return hit;
    }

    let mut best: Option<Opt> = None;
    let anchor = counts.anchor_base();
    let single_score = params.weights.single as i64;

    // 散张候选：锚点一张单张（不与其他牌成型）。仍记为 `Single` 首组选择，
    // 供还原链穿过散张继续到达其后的成型组。
    if let Some(a) = anchor {
        let mut rest = counts.clone();
        if rest.consume_single_base(a) {
            let tail = solve(params, memo, &rest);
            let choice = Choice {
                family: Family::Single,
                pattern_id: String::new(),
                count_key: 1,
                rank_key: 0,
                ranks: vec![a as u8],
                score: single_score,
            };
            let cand = Opt {
                score: tail.score + single_score,
                hands: tail.hands + 1,
                first: Some(Box::new(choice)),
            };
            params.keep_best(&mut best, cand);
        }
    } else if counts.little() + counts.big() > 0 {
        // 只剩王：取一张小王 / 大王作散张（王不成对，只能单出或组王炸）。
        let mut rest = counts.clone();
        if rest.little() > 0 {
            rest.take_little(1);
        } else {
            rest.take_big(1);
        }
        let tail = solve(params, memo, &rest);
        let choice = Choice {
            family: Family::Single,
            pattern_id: String::new(),
            count_key: 1,
            rank_key: 0,
            ranks: Vec::new(),
            score: single_score,
        };
        let cand = Opt {
            score: tail.score + single_score,
            hands: tail.hands + 1,
            first: Some(Box::new(choice)),
        };
        params.keep_best(&mut best, cand);
    }

    // 成型组候选：每个闭集条目在锚点上展开同点组 / 合法窗口。
    for closure in params.closure {
        match closure.family {
            Family::Single => continue,
            Family::Pair | Family::Triple | Family::Bomb(_) => {
                let Some(a) = anchor else { continue };
                if counts.real(a) + counts.wild() < u16::from(closure.count) {
                    continue;
                }
                let mut rest = counts.clone();
                if !rest.consume_same(a, closure.count) {
                    continue;
                }
                let tail = solve(params, memo, &rest);
                let rank_key = params.rank_key_same(a);
                let score = params.group_score(closure.family, rank_key);
                let choice = Choice {
                    family: closure.family,
                    pattern_id: closure.pattern_id.clone(),
                    count_key: closure.count as u32,
                    rank_key,
                    ranks: vec![a as u8],
                    score,
                };
                let cand = Opt {
                    score: tail.score + score,
                    hands: tail.hands + 1,
                    first: Some(Box::new(choice)),
                };
                params.keep_best(&mut best, cand);
            }
            Family::Straight | Family::Plate3 | Family::Steel2 => {
                let Some(a) = anchor else { continue };
                let steps = closure.steps as usize;
                let per = closure.count as usize;
                let n = counts.n();
                if steps == 0 || n < steps {
                    continue;
                }
                let mut windows: Vec<Vec<usize>> = Vec::new();
                for start in 0..=n - steps {
                    let w: Vec<usize> = (start..start + steps).collect();
                    if w.contains(&a) {
                        windows.push(w);
                    }
                }
                // 轮子顺 A-2-3-4-5（仅 `straight-5`，第十七次变更）。
                if closure.family == Family::Straight && steps == 5 {
                    let wheel = vec![n - 1, 0, 1, 2, 3];
                    if wheel.contains(&a) && !windows.contains(&wheel) {
                        windows.push(wheel);
                    }
                }
                for window in windows {
                    let mut rest = counts.clone();
                    if !rest.consume_window(&window, per as u8) {
                        continue;
                    }
                    let tail = solve(params, memo, &rest);
                    let mut sorted = window.clone();
                    sorted.sort_unstable();
                    let is_wheel = closure.family == Family::Straight
                        && steps == 5
                        && sorted == vec![0, 1, 2, 3, n - 1];
                    let top = *sorted.last().unwrap_or(&0);
                    let rank_key = if is_wheel {
                        0
                    } else {
                        params.rank_key_natural(top)
                    };
                    let count_key = (steps * per) as u32;
                    let score = params.group_score(closure.family, rank_key);
                    let choice = Choice {
                        family: closure.family,
                        pattern_id: closure.pattern_id.clone(),
                        count_key,
                        rank_key,
                        ranks: window.iter().map(|&r| r as u8).collect(),
                        score,
                    };
                    let cand = Opt {
                        score: tail.score + score,
                        hands: tail.hands + 1,
                        first: Some(Box::new(choice)),
                    };
                    params.keep_best(&mut best, cand);
                }
            }
            Family::JokerBomb => {
                if anchor.is_some() {
                    continue;
                }
                let mut rest = counts.clone();
                if !rest.consume_joker_bomb() {
                    continue;
                }
                let tail = solve(params, memo, &rest);
                let rank_key = params.ctx.joker_key(params.ruleset, true);
                let score = params.group_score(Family::JokerBomb, rank_key);
                let choice = Choice {
                    family: Family::JokerBomb,
                    pattern_id: closure.pattern_id.clone(),
                    count_key: 4,
                    rank_key,
                    ranks: Vec::new(),
                    score,
                };
                let cand = Opt {
                    score: tail.score + score,
                    hands: tail.hands + 1,
                    first: Some(Box::new(choice)),
                };
                params.keep_best(&mut best, cand);
            }
            Family::TriplePair => continue,
        }
    }

    let result = best.unwrap_or(Opt {
        score: 0,
        hands: 0,
        first: None,
    });
    memo.insert(key, result.clone());
    result
}

/// 手牌分桶（选牌落地用）：真牌按点数升序、百搭 / 小王 / 大王各自成桶。
struct HandBuckets {
    real: BTreeMap<usize, Vec<u16>>,
    wild: Vec<u16>,
    little: Vec<u16>,
    big: Vec<u16>,
}

impl HandBuckets {
    fn build(r: &RuleSetBody, wild_ids: &HashSet<u16>, hand: &[u16]) -> Self {
        let rank_count = r.deck.ranks.len();
        let suit_count = r.deck.suits.len();
        let copies = r.deck.copies as usize;
        let base_total = rank_count * suit_count * copies;
        let mut real: BTreeMap<usize, Vec<u16>> = BTreeMap::new();
        let mut wild = Vec::new();
        let mut little = Vec::new();
        let mut big = Vec::new();
        for &card in hand {
            let c = card as usize;
            if wild_ids.contains(&card) {
                wild.push(card);
                continue;
            }
            if c >= base_total {
                let slot = (c - base_total) / copies;
                if slot == 0 {
                    little.push(card);
                } else {
                    big.push(card);
                }
                continue;
            }
            let b = c / copies;
            let rank_idx = b % rank_count;
            real.entry(rank_idx).or_default().push(card);
        }
        for v in real.values_mut() {
            v.sort_unstable();
        }
        wild.sort_unstable();
        little.sort_unstable();
        big.sort_unstable();
        Self {
            real,
            wild,
            little,
            big,
        }
    }

    /// 取某点数前 `want` 张真牌，不足从百搭补；返回凑齐的牌（真牌优先，§5.11「落地」）。
    fn take_same(&mut self, rank: usize, want: usize) -> Vec<u16> {
        let mut cards = Vec::new();
        let bucket = self.real.entry(rank).or_default();
        let take = want.min(bucket.len());
        cards.extend_from_slice(&bucket[..take]);
        bucket.drain(..take);
        let need = want - cards.len();
        let wild_take = need.min(self.wild.len());
        cards.extend_from_slice(&self.wild[..wild_take]);
        self.wild.drain(..wild_take);
        cards
    }

    /// 连续组：窗口每点 `per` 张，真牌优先、百搭补孔。
    fn take_window(&mut self, window: &[usize], per: usize) -> Vec<u16> {
        let mut cards = Vec::new();
        for &r in window {
            cards.extend(self.take_same(r, per));
        }
        cards
    }

    fn take_joker_bomb(&mut self) -> Vec<u16> {
        let mut cards = Vec::new();
        let lt = 2.min(self.little.len());
        cards.extend_from_slice(&self.little[..lt]);
        self.little.drain(..lt);
        let bg = 2.min(self.big.len());
        cards.extend_from_slice(&self.big[..bg]);
        self.big.drain(..bg);
        cards
    }
}

/// 内部成型分组（携带族 / 点数 / 窗口供后处理判定）。
#[derive(Clone)]
struct Built {
    layout: LayoutGroup,
    family: Family,
    rank: usize,
    ranks: Vec<usize>,
    rank_key: u32,
    score: i64,
}

fn apply_choice(counts: &mut KeyCounts, choice: &Choice) {
    match choice.family {
        Family::Pair | Family::Triple | Family::Bomb(_) => {
            if let Some(&r) = choice.ranks.first() {
                counts.consume_same(r as usize, choice.count_key as u8);
            }
        }
        Family::Straight | Family::Plate3 | Family::Steel2 => {
            let window: Vec<usize> = choice.ranks.iter().map(|&r| r as usize).collect();
            let per = match choice.family {
                Family::Straight => 1,
                Family::Plate3 => 2,
                Family::Steel2 => 3,
                _ => 1,
            };
            counts.consume_window(&window, per);
        }
        Family::JokerBomb => {
            counts.consume_joker_bomb();
        }
        Family::Single => {
            if let Some(&r) = choice.ranks.first() {
                counts.consume_single_base(r as usize);
            } else if counts.little() > 0 {
                counts.take_little(1);
            } else if counts.big() > 0 {
                counts.take_big(1);
            }
        }
        Family::TriplePair => {}
    }
}

/// 推荐主入口：计数向量精确 DFS + 两条后处理。纯函数、只读、不回写状态。
pub(crate) fn recommend_layout(
    ruleset: &RuleSetBody,
    ladder: &[crate::contract::PatternId],
    ctx: &RankContext,
    hand: &[u16],
) -> Layout {
    let (enabled, weights, mode) = resolve_config(ruleset.ai.as_ref());
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

    // 同花顺候选独立于最优拆分（§5.11 第 835–836 行），含百搭方案、去重、按 cards 升序。
    // 只跑同花顺这一支（按花色 × 窗口构造），不整手枚举全部牌型：与 `enumerate` 的同花顺
    // 子集逐条等价，差分用例见 `engine.rs` 的 `straight_flush_candidates_match_enumerate`。
    let flush_candidates = crate::patterns::straight_flush_candidates(ladder, ruleset, ctx, hand);
    let has_straight_flush = !flush_candidates.is_empty();

    let wild_ids = crate::patterns::wildcard_ids(ruleset, ctx);
    let counts = KeyCounts::build(ruleset, ctx, hand);
    let closure = build_closure(ruleset);
    let params = SearchParams {
        ruleset,
        ctx,
        weights: &weights,
        mode,
        closure: &closure,
        n_non_joker: ruleset.deck.ranks.len(),
    };
    let mut memo = Memo::new(1024);
    let root = solve(&params, &mut memo, &counts);

    // 沿首组选择链还原成型分组描述（不克隆整条序列，只走记忆化首组指针）。
    let mut cur = counts.clone();
    let mut choices: Vec<Choice> = Vec::new();
    let mut opt = root.clone();
    while let Some(choice) = opt.first.take() {
        choices.push((*choice).clone());
        apply_choice(&mut cur, choices.last().expect("刚压入"));
        if cur.is_empty() {
            break;
        }
        match memo.get(cur.pack()).cloned() {
            Some(next) => opt = next,
            None => {
                debug_assert!(false, "还原链在记忆化表里断了");
                break;
            }
        }
    }

    let mut buckets = HandBuckets::build(ruleset, &wild_ids, hand);
    let mut built: Vec<Built> = Vec::new();
    for choice in &choices {
        if choice.family == Family::Single {
            continue;
        }
        let cards = match choice.family {
            Family::Pair | Family::Triple | Family::Bomb(_) => {
                let r = choice.ranks.first().copied().unwrap_or(0) as usize;
                buckets.take_same(r, choice.count_key as usize)
            }
            Family::Straight | Family::Plate3 | Family::Steel2 => {
                let window: Vec<usize> = choice.ranks.iter().map(|&r| r as usize).collect();
                let per = match choice.family {
                    Family::Straight => 1,
                    Family::Plate3 => 2,
                    Family::Steel2 => 3,
                    _ => 1,
                };
                buckets.take_window(&window, per)
            }
            Family::JokerBomb => buckets.take_joker_bomb(),
            Family::Single | Family::TriplePair => Vec::new(),
        };
        let ranks: Vec<usize> = choice.ranks.iter().map(|&r| r as usize).collect();
        let rank = ranks.first().copied().unwrap_or(0);
        built.push(Built {
            layout: LayoutGroup {
                pattern_id: choice.pattern_id.clone(),
                pattern_key: (choice.count_key, choice.rank_key),
                cards,
            },
            family: choice.family,
            rank,
            ranks,
            rank_key: choice.rank_key,
            score: choice.score,
        });
    }

    // 散张数按**后处理之前**的搜索手数计（后处理只合并成型组，不改散张）；王对被
    // 三带二吸收时由后处理回吐 2 张（那两张原本按散张算）。
    let mut single_count = root.hands as usize - built.len();

    postprocess_triple_pair(
        ruleset,
        &weights,
        params.n_non_joker,
        hand,
        &mut built,
        &mut single_count,
    );
    postprocess_straight_flush(ladder, ruleset, ctx, &weights, hand, &wild_ids, &mut built);

    // `score` 必须是后处理之后的实际分组分数之和（§5.11「score = Σ groupScore」）。
    let mut score: i64 = single_count as i64 * weights.single as i64;
    let mut groups: Vec<LayoutGroup> = Vec::with_capacity(built.len());
    for b in built {
        score += b.score;
        let mut group = b.layout;
        group.cards.sort_unstable();
        groups.push(group);
    }
    groups.sort_by(|a, b| {
        (&a.pattern_id, std::cmp::Reverse(a.pattern_key), &a.cards).cmp(&(
            &b.pattern_id,
            std::cmp::Reverse(b.pattern_key),
            &b.cards,
        ))
    });

    Layout {
        groups,
        score,
        single_count: single_count as u8,
        straight_flush_candidates: flush_candidates,
        has_straight_flush,
        disabled: false,
    }
}

/// 后处理 ①：三带二贪心吸收（§5.11 第 853–856 行）。
///
/// 附件（「对子」）来源两类：
/// - 搜索产出的真牌对子（`Family::Pair`），点数按搜索侧口径；
/// - **剩余的两张同色王**（两张小王或两张大王，§5.13 第 930 行「双副牌下两张小王 / 两张
///   大王成对亦可」；一大一小不成对、也就走不了这条），搜索侧王只作单张 / 王炸，故在此
///   兜底。两个王原先各算一张散张，吸收后 `single_count` 相应减 2。
///
/// 顺序固定：**点数最小的对子**优先（真牌对子按点数升序在前，小王对、大王对排在最后，
/// 因此只在没有更小真牌对子可试时才动用王对），吸收进**点数最小的三张**；接受条件为
/// 「手数减 1（合并必然满足）且 `score` 不降」，不满足则回退该次吸收、换下一个对子。
/// 无随机。
fn postprocess_triple_pair(
    ruleset: &RuleSetBody,
    weights: &RecommendWeights,
    n_non_joker: usize,
    hand: &[u16],
    built: &mut Vec<Built>,
    single_count: &mut usize,
) {
    let tp_id = triple_pair_id(ruleset);
    let copies = ruleset.deck.copies as usize;
    let base_total = n_non_joker * ruleset.deck.suits.len() * copies;
    // 王对候选的点数键：排在所有真牌对子之后（小王 < 大王）。
    let little_rank = n_non_joker;
    let big_rank = n_non_joker + 1;
    // 被否决的对子（按落地牌 id 序列记录），不再重复尝试。
    let mut rejected: Vec<Vec<u16>> = Vec::new();
    loop {
        // 真牌对子（当前仍未被吸收的）+ 剩余同色王对。
        let mut attachments: Vec<(usize, Vec<u16>, i64, Option<usize>)> = built
            .iter()
            .enumerate()
            .filter(|(_, b)| b.family == Family::Pair)
            .map(|(i, b)| (b.rank, b.layout.cards.clone(), b.score, Some(i)))
            .collect();
        {
            let occupied: HashSet<u16> = built
                .iter()
                .flat_map(|b| b.layout.cards.iter().copied())
                .collect();
            let mut jokers: Vec<u16> = hand
                .iter()
                .copied()
                .filter(|&c| (c as usize) >= base_total && !occupied.contains(&c))
                .collect();
            jokers.sort_unstable();
            for (rank, lo, hi) in [
                (little_rank, base_total, base_total + copies),
                (big_rank, base_total + copies, base_total + 2 * copies),
            ] {
                let kind: Vec<u16> = jokers
                    .iter()
                    .copied()
                    .filter(|&c| (c as usize) >= lo && (c as usize) < hi)
                    .collect();
                if kind.len() == 2 {
                    attachments.push((rank, kind, 2 * weights.single as i64, None));
                }
            }
        }
        let Some((_, pair_cards, pair_score, pair_group)) = attachments
            .into_iter()
            .filter(|(_, cards, _, _)| !rejected.contains(cards))
            .min_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)))
        else {
            break;
        };

        let triple_idx = built
            .iter()
            .enumerate()
            .filter(|(_, b)| b.family == Family::Triple)
            .min_by(|a, b| {
                a.1.rank
                    .cmp(&b.1.rank)
                    .then_with(|| a.1.layout.cards.cmp(&b.1.layout.cards))
            })
            .map(|(i, _)| i);
        let Some(ti) = triple_idx else { break };

        let triple_rank_key = built[ti].rank_key;
        let new_score = weights.triple_pair_base as i64
            + rank_bonus(weights.triple_pair_rank_bonus, triple_rank_key, n_non_joker);
        let old_score = pair_score + built[ti].score;
        if new_score < old_score {
            rejected.push(pair_cards);
            continue;
        }

        let mut cards = built[ti].layout.cards.clone();
        cards.extend_from_slice(&pair_cards);
        cards.sort_unstable();
        let triple = built[ti].clone();
        built[ti] = Built {
            layout: LayoutGroup {
                pattern_id: tp_id.clone(),
                pattern_key: (5, triple.rank_key),
                cards,
            },
            family: Family::TriplePair,
            rank: triple.rank,
            ranks: triple.ranks.clone(),
            rank_key: triple.rank_key,
            score: new_score,
        };
        match pair_group {
            Some(pi) => {
                built.remove(pi);
            }
            // 王对原本是两张散张：并进组后散张数减 2。
            None => *single_count = single_count.saturating_sub(2),
        }
    }
}

/// 后处理 ②：同花顺升级（§5.11 第 857–861 行）。
///
/// 计数向量层不含花色，故对每个 5 张顺子**回原手牌做花色层检测**：按「同一点数 × 同一
/// 花色」找真牌（跳过其它分组已占用的牌），不足的孔位由百搭补；找到即把该组的牌换成同花
/// 那一套（同点数、不同副本，张数不变，守恒不受影响），再用 `recognize` 确认身份并按
/// `straightFlush` 计分。找不到就保持杂顺，不降级、不改别的组。
fn postprocess_straight_flush(
    ladder: &[crate::contract::PatternId],
    ruleset: &RuleSetBody,
    ctx: &RankContext,
    weights: &RecommendWeights,
    hand: &[u16],
    wild_ids: &HashSet<u16>,
    built: &mut [Built],
) {
    let suit_count = ruleset.deck.suits.len();
    for i in 0..built.len() {
        if built[i].family != Family::Straight || built[i].layout.cards.len() != 5 {
            continue;
        }
        let mut window = built[i].ranks.clone();
        window.sort_unstable();
        if window.len() != 5 {
            continue;
        }

        // 其它分组已占用的牌（本组自身除外）与仍可用的百搭。
        let mut occupied: HashSet<u16> = HashSet::new();
        for (j, b) in built.iter().enumerate() {
            if j == i {
                continue;
            }
            occupied.extend(b.layout.cards.iter().copied());
        }
        let mut wild_free: Vec<u16> = hand
            .iter()
            .filter(|c| wild_ids.contains(c) && !occupied.contains(c))
            .copied()
            .collect();
        wild_free.sort_unstable();

        // (rank, suit) → 最小可用真牌 id。
        let mut avail: BTreeMap<(usize, usize), u16> = BTreeMap::new();
        let rank_count = ruleset.deck.ranks.len();
        let copies = ruleset.deck.copies as usize;
        let base_total = rank_count * suit_count * copies;
        for &card in hand {
            let c = card as usize;
            if wild_ids.contains(&card) || occupied.contains(&card) || c >= base_total {
                continue;
            }
            let b = c / copies;
            let entry = avail
                .entry((b % rank_count, b / rank_count))
                .or_insert(card);
            if *entry > card {
                *entry = card;
            }
        }

        // 逐花色试：真牌优先（百搭用得越少越好）、同分取牌 id 序列字典序小者。
        let mut best: Option<(usize, Vec<u16>)> = None;
        for suit in 0..suit_count {
            let mut cards = Vec::with_capacity(5);
            let mut need_wild = 0usize;
            for &r in &window {
                match avail.get(&(r, suit)) {
                    Some(&id) => cards.push(id),
                    None => need_wild += 1,
                }
            }
            if need_wild > wild_free.len() {
                continue;
            }
            cards.extend_from_slice(&wild_free[..need_wild]);
            cards.sort_unstable();
            let better = match &best {
                None => true,
                Some((w, current)) => (need_wild, &cards) < (*w, current),
            };
            if better {
                best = Some((need_wild, cards));
            }
        }

        let Some((_, cards)) = best else { continue };
        let Some(rec) = recognize(ladder, ruleset, ctx, &cards) else {
            continue;
        };
        // 身份确认：必须真的判成同花顺，否则不动（不改搜索树、不猜牌型）。
        if rec.role != BombRole::Flush {
            continue;
        }
        built[i].layout.pattern_id = rec.pattern_id;
        built[i].layout.pattern_key = rec.pattern_key;
        built[i].layout.cards = cards;
        built[i].score = weights.straight_flush as i64;
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

    #[test]
    fn memo_grows_and_keeps_entries() {
        let mut memo = Memo::new(16);
        for key in 0u64..200 {
            memo.insert(
                key,
                Opt {
                    score: key as i64,
                    hands: 1,
                    first: None,
                },
            );
        }
        for key in 0u64..200 {
            let hit = memo.get(key).expect("扩容后条目不得丢失");
            assert_eq!(hit.score, key as i64);
        }
        assert!(memo.cap >= 512, "载荷破 1/2 必须扩容：cap={}", memo.cap);
    }
}
