//! 牌型识别、候选枚举与压制比较（§5.2 参数表第 513–519 行、§5.10 取值表第 775 行、
//! §5.13 play 段第 890–895 行）。
//!
//! 判定以 `ruleset.patterns[]` 的已声明牌型为驱动：张数 / 长度 / 连对组数 / 钢板组数
//! 全部读夹具参数，代码不写死牌型 id 与取值；未声明的原语不产生候选。
//!
//! # 三条显式 hold（文档缺口待上位裁决，本批不做近似实现）
//!
//! - TODO(M1-HOLD-wildcard)：逢人配替代规则。§5.13 第 893 行仅写「红桃级牌可替代除
//!   大小王外任意一张牌；`inStraightFlush=false` 时不得用于同花顺」，第 891 行写
//!   「代牌后仍须整体连续」；一手可否用两张逢人配、全逢人配牌型点数取什么、含百搭
//!   牌型的 `rankKey` 取哪个解、顺子多孔位多解时取哪个、能否代级牌，文档均未规定。
//!   本批遇到 wildcard 牌一律不产生候选、提交含 wildcard 的出牌判非法。
//! - TODO(M1-HOLD-with-attachment)：三带二（`pattern.with-attachment`）。§5.2 第 495、
//!   547 行定为 v2 预留、引用即 L1 拒；§5.13 第 890 行牌型清单没有它；`guandan-classic`
//!   夹具也未声明。本批不识别。
//! - TODO(M1-HOLD-level-rank)：级牌点力重排。§5.13 第 892 行写「当前 `levels[team]`
//!   对应的牌…按高于 A 参与比较」，但每队一个 `levels`、两队档位可能不同，跨队比较时
//!   同一张牌点力不唯一；本批 `rankKey` 保持 `naturalOrder`，不按级牌高位重排。

use crate::hash::LastPlay;
use crate::ruleset::ladder::synthetic_ladder;
use crate::ruleset::model::{PatternDef, RuleSetBody, WildcardRank};
use crate::ruleset::params::PrimitiveParams;
use std::collections::{BTreeMap, HashSet};

// ---------- 牌 id 几何（与发牌期冻结编码一致，§5.12 第 811 行） ----------

fn rank_count(r: &RuleSetBody) -> usize {
    r.deck.ranks.len()
}

fn suit_count(r: &RuleSetBody) -> usize {
    r.deck.suits.len()
}

fn copies(r: &RuleSetBody) -> usize {
    r.deck.copies as usize
}

fn base_total(r: &RuleSetBody) -> usize {
    rank_count(r) * suit_count(r) * copies(r)
}

/// 基础牌 id：`(suit × rank_count + rank) × copies + copy`（测试侧构造受控手牌用）。
pub fn card_id(r: &RuleSetBody, rank: usize, suit: usize, copy: u8) -> u16 {
    ((suit * rank_count(r) + rank) * copies(r) + copy as usize) as u16
}

/// 王 id：小王在前 `copies` 张、大王在后 `copies` 张（测试侧构造受控手牌用）。
pub fn joker_id(r: &RuleSetBody, big: bool, copy: u8) -> u16 {
    let slot = if big { copies(r) } else { 0 };
    (base_total(r) + slot + copy as usize) as u16
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CardRef {
    Base { rank: usize, suit: usize },
    LittleJoker,
    BigJoker,
}

fn card_ref(r: &RuleSetBody, card: u16) -> CardRef {
    let base = base_total(r);
    if card as usize >= base {
        let slot = (card as usize - base) / copies(r);
        if slot == 0 {
            CardRef::LittleJoker
        } else {
            CardRef::BigJoker
        }
    } else {
        let b = card as usize / copies(r);
        let rank = b % rank_count(r);
        let suit = b / rank_count(r);
        CardRef::Base { rank, suit }
    }
}

/// 点数键：`naturalOrder` 下标（不含级牌高位插入；见模块顶部 level-rank hold）。
fn rank_key_of(r: &RuleSetBody, rank_idx: usize) -> u32 {
    // TODO(M1-HOLD-level-rank)：§5.13 第 892 行「当前 levels[team] 对应的牌…按高于 A
    // 参与比较」未落地——每队一个 levels、两队档位可能不同，跨队比较时同一张牌点力
    // 不唯一，可比性口径待裁决；本批保持 naturalOrder，不按级牌高位重排。
    let rank_str = r.deck.ranks[rank_idx].as_str();
    r.natural_order
        .iter()
        .position(|k| k.as_str() == rank_str)
        .unwrap_or(rank_idx) as u32
}

fn joker_rank_key(r: &RuleSetBody, big: bool) -> u32 {
    let tail = r.natural_order.len();
    (tail - 2 + if big { 1 } else { 0 }) as u32
}

/// 逢人配牌 id 集合（hold 口径：识别出即排除，不做替代）。
///
/// `@level` 占位符在 score-only 局 `levels` 为空，载入期只能按 `levelTable.initial`
/// 解析源点数；这只是「排除哪些牌」的定位用近似，替代语义本身见 hold TODO。
fn wildcard_ids(r: &RuleSetBody) -> HashSet<u16> {
    let mut out = HashSet::new();
    let Some(w) = &r.wildcard else { return out };
    let rank_str = match &w.source.rank {
        WildcardRank::Rank(k) => k.0.clone(),
        WildcardRank::LevelPlaceholder => match &r.level_table {
            Some(t) => t.initial.0.clone(),
            None => return out,
        },
    };
    let Some(rank_idx) = r.deck.ranks.iter().position(|k| k.0 == rank_str) else {
        return out;
    };
    let Some(suit_idx) = r.deck.suits.iter().position(|s| *s == w.source.suit) else {
        return out;
    };
    for copy in 0..copies(r) {
        out.insert(card_id(r, rank_idx, suit_idx, copy as u8));
    }
    out
}

// ---------- 识别结果 ----------

/// 一次识别出的成型牌型（服务端判定结果，§5.9 第 203 行：`patternId` 由服务端判定）。
pub(crate) struct Recognized {
    pub pattern_id: String,
    /// `patternKey = (countKey, rankKey)`（§5.12 第 797、842 行）。
    pub pattern_key: (u32, u32),
    pub cards: Vec<u16>,
    pub primitive: &'static str,
}

fn primitive_of(params: &PrimitiveParams) -> &'static str {
    match params {
        PrimitiveParams::SameRank { .. } => "pattern.same-rank",
        PrimitiveParams::Straight { .. } => "pattern.straight",
        PrimitiveParams::ConsecutivePairs { .. } => "pattern.consecutive-pairs",
        PrimitiveParams::ConsecutiveTriples { .. } => "pattern.consecutive-triples",
        PrimitiveParams::Bomb { .. } => "pattern.bomb",
        PrimitiveParams::StraightFlush { .. } => "pattern.straight-flush",
        PrimitiveParams::JokerBomb => "pattern.joker-bomb",
        _ => "reserved",
    }
}

/// 一札多解时的判定优先级：更具体的牌型优先（同花顺压过普通顺子、炸弹压过同点数散牌）。
fn priority(params: &PrimitiveParams) -> u8 {
    match params {
        PrimitiveParams::JokerBomb => 0,
        PrimitiveParams::Bomb { .. } => 1,
        PrimitiveParams::StraightFlush { .. } => 2,
        PrimitiveParams::ConsecutiveTriples { .. } => 3,
        PrimitiveParams::ConsecutivePairs { .. } => 4,
        PrimitiveParams::Straight { .. } => 5,
        PrimitiveParams::SameRank { .. } => 6,
        _ => 99,
    }
}

fn build_rec(def: &PatternDef, count_key: u32, rank_key: u32, cards: Vec<u16>) -> Recognized {
    let mut cards = cards;
    cards.sort_unstable();
    Recognized {
        pattern_id: def.id.0.clone(),
        pattern_key: (count_key, rank_key),
        cards,
        primitive: primitive_of(&def.params),
    }
}

/// 按点数归组的提交牌 / 手牌。
struct Groups {
    /// 点数 → 该点数牌（升序）。
    base: BTreeMap<usize, Vec<u16>>,
    little: Vec<u16>,
    big: Vec<u16>,
}

impl Groups {
    fn build(r: &RuleSetBody, cards: &[u16]) -> Self {
        let mut base: BTreeMap<usize, Vec<u16>> = BTreeMap::new();
        let mut little = Vec::new();
        let mut big = Vec::new();
        for &card in cards {
            match card_ref(r, card) {
                CardRef::Base { rank, .. } => base.entry(rank).or_default().push(card),
                CardRef::LittleJoker => little.push(card),
                CardRef::BigJoker => big.push(card),
            }
        }
        for v in base.values_mut() {
            v.sort_unstable();
        }
        little.sort_unstable();
        big.sort_unstable();
        Self { base, little, big }
    }

    fn no_jokers(&self) -> bool {
        self.little.is_empty() && self.big.is_empty()
    }
}

/// 连续点数判定；返回高点位下标。`None` 即断档或含王。
///
/// §5.13 第 891 行：相邻按 `naturalOrder` 线性比较；唯一环形特例 `A-2-3-4-5` 合法，
/// `Q-K-A-2-3` 非法，A 后接 2 非法。
fn consecutive_top(rank_count: usize, ranks: &[usize], length: usize) -> Option<usize> {
    if ranks.len() != length {
        return None;
    }
    let mut sorted: Vec<usize> = ranks.to_vec();
    sorted.sort_unstable();
    if sorted.windows(2).all(|w| w[1] == w[0] + 1) {
        return Some(sorted[length - 1]);
    }
    // 轮子顺 A-2-3-4-5：高点按 5 计（标准扑克口径，轮子顺为最小顺子）。
    if length == 5 && sorted == [0, 1, 2, 3, rank_count - 1] {
        return Some(3);
    }
    None
}

fn match_same_rank(r: &RuleSetBody, def: &PatternDef, count: u8, g: &Groups) -> Option<Recognized> {
    let count = count as usize;
    if count == 1 {
        if g.no_jokers() && g.base.len() == 1 {
            let (&rank, cards) = g.base.iter().next().unwrap();
            if cards.len() == 1 {
                return Some(build_rec(def, 1, rank_key_of(r, rank), cards.clone()));
            }
        }
        if g.base.is_empty() {
            if g.little.len() == 1 && g.big.is_empty() {
                return Some(build_rec(
                    def,
                    1,
                    joker_rank_key(r, false),
                    g.little.clone(),
                ));
            }
            if g.big.len() == 1 && g.little.is_empty() {
                return Some(build_rec(def, 1, joker_rank_key(r, true), g.big.clone()));
            }
        }
        return None;
    }
    if !g.no_jokers() || g.base.len() != 1 {
        return None;
    }
    let (&rank, cards) = g.base.iter().next().unwrap();
    if cards.len() != count {
        return None;
    }
    Some(build_rec(
        def,
        count as u32,
        rank_key_of(r, rank),
        cards.clone(),
    ))
}

fn match_bomb(r: &RuleSetBody, def: &PatternDef, count: u8, g: &Groups) -> Option<Recognized> {
    // 炸弹是同点数 4–8 张；结构与 same-rank 同，张数区间由夹具参数 + L1 值域闸门保证。
    match_same_rank(r, def, count, g)
}

fn match_straight(
    r: &RuleSetBody,
    def: &PatternDef,
    length: u8,
    g: &Groups,
    flush: bool,
) -> Option<Recognized> {
    let length = length as usize;
    if !g.no_jokers() || g.base.len() != length {
        return None;
    }
    let ranks: Vec<usize> = g.base.keys().copied().collect();
    let top = consecutive_top(rank_count(r), &ranks, length)?;
    let mut cards = Vec::new();
    let mut suit_seen: Option<usize> = None;
    for bucket in g.base.values() {
        if bucket.len() != 1 {
            return None;
        }
        let card = bucket[0];
        if let CardRef::Base { suit, .. } = card_ref(r, card) {
            if flush {
                match suit_seen {
                    None => suit_seen = Some(suit),
                    Some(s) if s != suit => return None,
                    _ => {}
                }
            }
        }
        cards.push(card);
    }
    Some(build_rec(def, length as u32, rank_key_of(r, top), cards))
}

fn match_consecutive(
    r: &RuleSetBody,
    def: &PatternDef,
    groups: usize,
    per_group: usize,
    g: &Groups,
) -> Option<Recognized> {
    if !g.no_jokers() || g.base.len() != groups {
        return None;
    }
    let ranks: Vec<usize> = g.base.keys().copied().collect();
    let top = consecutive_top(rank_count(r), &ranks, groups)?;
    let mut cards = Vec::new();
    for bucket in g.base.values() {
        if bucket.len() != per_group {
            return None;
        }
        cards.extend_from_slice(bucket);
    }
    Some(build_rec(
        def,
        (groups * per_group) as u32,
        rank_key_of(r, top),
        cards,
    ))
}

fn match_joker_bomb(r: &RuleSetBody, def: &PatternDef, g: &Groups) -> Option<Recognized> {
    if !g.base.is_empty() || g.little.len() != 2 || g.big.len() != 2 {
        return None;
    }
    let mut cards = Vec::new();
    cards.extend_from_slice(&g.little);
    cards.extend_from_slice(&g.big);
    Some(build_rec(def, 4, joker_rank_key(r, true), cards))
}

fn match_def(r: &RuleSetBody, def: &PatternDef, g: &Groups) -> Option<Recognized> {
    match &def.params {
        PrimitiveParams::SameRank { count } => match_same_rank(r, def, *count, g),
        PrimitiveParams::Straight { length, .. } => match_straight(r, def, *length, g, false),
        PrimitiveParams::ConsecutivePairs { pairs } => {
            match_consecutive(r, def, *pairs as usize, 2, g)
        }
        PrimitiveParams::ConsecutiveTriples { triples } => {
            match_consecutive(r, def, *triples as usize, 3, g)
        }
        PrimitiveParams::Bomb { count } => match_bomb(r, def, *count, g),
        PrimitiveParams::StraightFlush { length } => match_straight(r, def, *length, g, true),
        PrimitiveParams::JokerBomb => match_joker_bomb(r, def, g),
        // TODO(M1-HOLD-with-attachment)：v2 预留原语（§5.2 第 495/547 行、§5.13 第 890 行），
        // L1 已拒绝引用；运行期即便进入也不识别为任何牌型。
        PrimitiveParams::Reserved { .. } => None,
        _ => None,
    }
}

/// 按提交牌结构判定牌型；不匹配任何已声明牌型返回 `None`（§5.9：`patternId` 服务端判定）。
pub(crate) fn recognize(r: &RuleSetBody, cards: &[u16]) -> Option<Recognized> {
    if cards.is_empty() {
        return None;
    }
    // TODO(M1-HOLD-wildcard)：含逢人配的出牌本批一律判非法，替代语义待裁决（模块顶部注释）。
    let wild = wildcard_ids(r);
    if cards.iter().any(|c| wild.contains(c)) {
        return None;
    }
    let mut seen = HashSet::new();
    if !cards.iter().all(|c| seen.insert(*c)) {
        return None;
    }
    let groups = Groups::build(r, cards);
    let mut defs: Vec<&PatternDef> = r
        .patterns
        .iter()
        .filter(|p| priority(&p.params) < 99)
        .collect();
    defs.sort_by_key(|p| priority(&p.params));
    for def in defs {
        if let Some(rec) = match_def(r, def, &groups) {
            return Some(rec);
        }
    }
    None
}

// ---------- 候选枚举 ----------

struct HandGroups {
    base: BTreeMap<usize, Vec<u16>>,
    /// 花色 → 点数 → 牌（同花顺枚举用）。
    by_suit: BTreeMap<usize, BTreeMap<usize, Vec<u16>>>,
    little: Vec<u16>,
    big: Vec<u16>,
}

impl HandGroups {
    fn build(r: &RuleSetBody, hand: &[u16]) -> Self {
        let wild = wildcard_ids(r);
        let mut base: BTreeMap<usize, Vec<u16>> = BTreeMap::new();
        let mut by_suit: BTreeMap<usize, BTreeMap<usize, Vec<u16>>> = BTreeMap::new();
        let mut little = Vec::new();
        let mut big = Vec::new();
        for &card in hand {
            // TODO(M1-HOLD-wildcard)：wildcard 牌不进任何候选（模块顶部注释）。
            if wild.contains(&card) {
                continue;
            }
            match card_ref(r, card) {
                CardRef::Base { rank, suit } => {
                    base.entry(rank).or_default().push(card);
                    by_suit
                        .entry(suit)
                        .or_default()
                        .entry(rank)
                        .or_default()
                        .push(card);
                }
                CardRef::LittleJoker => little.push(card),
                CardRef::BigJoker => big.push(card),
            }
        }
        for v in base.values_mut() {
            v.sort_unstable();
        }
        for s in by_suit.values_mut() {
            for v in s.values_mut() {
                v.sort_unstable();
            }
        }
        little.sort_unstable();
        big.sort_unstable();
        Self {
            base,
            by_suit,
            little,
            big,
        }
    }
}

/// 连续点数窗口；返回每个窗口的点数序列（含轮子顺特例）。
fn rank_windows(rank_count: usize, present: &HashSet<usize>, length: usize) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    for start in 0..=rank_count.saturating_sub(length) {
        if (start..start + length).all(|i| present.contains(&i)) {
            out.push((start..start + length).collect());
        }
    }
    if length == 5
        && present.contains(&0)
        && present.contains(&1)
        && present.contains(&2)
        && present.contains(&3)
        && present.contains(&(rank_count - 1))
    {
        out.push(vec![rank_count - 1, 0, 1, 2, 3]);
    }
    out
}

fn pick_one_per_rank(base: &BTreeMap<usize, Vec<u16>>, ranks: &[usize]) -> Vec<u16> {
    pick_n_per_rank(base, ranks, 1)
}

/// 每个点数取前 `n` 张（连对 n=2、钢板 n=3）。
fn pick_n_per_rank(base: &BTreeMap<usize, Vec<u16>>, ranks: &[usize], n: usize) -> Vec<u16> {
    let mut cards = Vec::new();
    for &rank in ranks {
        if let Some(bucket) = base.get(&rank) {
            cards.extend_from_slice(&bucket[..n]);
        }
    }
    cards
}

/// 枚举手牌中所有可成型的牌型候选（领出 / 跟牌的压过过滤在接线层做）。
pub(crate) fn enumerate(r: &RuleSetBody, hand: &[u16]) -> Vec<Recognized> {
    let g = HandGroups::build(r, hand);
    let mut out: Vec<Recognized> = Vec::new();
    let mut emitted: HashSet<Vec<u16>> = HashSet::new();

    let mut push = |rec: Recognized| {
        if emitted.insert(rec.cards.clone()) {
            out.push(rec);
        }
    };

    let present: HashSet<usize> = g.base.keys().copied().collect();

    // 按识别优先级输出：更具体的牌型先占位，同一札多解时不被泛化牌型顶掉。
    let mut defs: Vec<&PatternDef> = r
        .patterns
        .iter()
        .filter(|p| priority(&p.params) < 99)
        .collect();
    defs.sort_by_key(|p| priority(&p.params));
    for def in defs {
        match &def.params {
            PrimitiveParams::SameRank { count } => {
                let count = *count as usize;
                if count == 1 {
                    for bucket in g.base.values() {
                        for &card in bucket {
                            if let Some(rec) = recognize(r, &[card]) {
                                push(rec);
                            }
                        }
                    }
                    for &card in &g.little {
                        if let Some(rec) = recognize(r, &[card]) {
                            push(rec);
                        }
                    }
                    for &card in &g.big {
                        if let Some(rec) = recognize(r, &[card]) {
                            push(rec);
                        }
                    }
                } else {
                    for bucket in g.base.values() {
                        if bucket.len() >= count {
                            let cards = bucket[..count].to_vec();
                            if let Some(rec) = recognize(r, &cards) {
                                push(rec);
                            }
                        }
                    }
                }
            }
            PrimitiveParams::Bomb { count } => {
                let count = *count as usize;
                for bucket in g.base.values() {
                    if bucket.len() >= count {
                        let cards = bucket[..count].to_vec();
                        if let Some(rec) = recognize(r, &cards) {
                            push(rec);
                        }
                    }
                }
            }
            PrimitiveParams::Straight { length, .. } => {
                for ranks in rank_windows(rank_count(r), &present, *length as usize) {
                    let cards = pick_one_per_rank(&g.base, &ranks);
                    if let Some(rec) = recognize(r, &cards) {
                        push(rec);
                    }
                }
            }
            PrimitiveParams::ConsecutivePairs { pairs } => {
                let groups = *pairs as usize;
                let eligible: HashSet<usize> = g
                    .base
                    .iter()
                    .filter(|(_, b)| b.len() >= 2)
                    .map(|(&k, _)| k)
                    .collect();
                for ranks in rank_windows(rank_count(r), &eligible, groups) {
                    let cards = pick_n_per_rank(&g.base, &ranks, 2);
                    if let Some(rec) = recognize(r, &cards) {
                        push(rec);
                    }
                }
            }
            PrimitiveParams::ConsecutiveTriples { triples } => {
                let groups = *triples as usize;
                let eligible: HashSet<usize> = g
                    .base
                    .iter()
                    .filter(|(_, b)| b.len() >= 3)
                    .map(|(&k, _)| k)
                    .collect();
                for ranks in rank_windows(rank_count(r), &eligible, groups) {
                    let cards = pick_n_per_rank(&g.base, &ranks, 3);
                    if let Some(rec) = recognize(r, &cards) {
                        push(rec);
                    }
                }
            }
            PrimitiveParams::StraightFlush { length } => {
                for suit_buckets in g.by_suit.values() {
                    let suit_present: HashSet<usize> = suit_buckets.keys().copied().collect();
                    for ranks in rank_windows(rank_count(r), &suit_present, *length as usize) {
                        let cards = pick_one_per_rank(suit_buckets, &ranks);
                        if let Some(rec) = recognize(r, &cards) {
                            push(rec);
                        }
                    }
                }
            }
            PrimitiveParams::JokerBomb if g.little.len() >= 2 && g.big.len() >= 2 => {
                let mut cards = Vec::new();
                cards.extend_from_slice(&g.little[..2]);
                cards.extend_from_slice(&g.big[..2]);
                if let Some(rec) = recognize(r, &cards) {
                    push(rec);
                }
            }
            PrimitiveParams::JokerBomb => {}
            _ => {}
        }
    }
    out
}

// ---------- 压制比较 ----------

fn primitive_by_id(r: &RuleSetBody, pattern_id: &str) -> Option<&'static str> {
    r.patterns
        .iter()
        .find(|p| p.id.0 == pattern_id)
        .map(|p| primitive_of(&p.params))
}

/// 炸弹族：可压任意非炸弹牌型；族内按 ladder 位次比较（`straight-flush` /
/// `joker-bomb` 与 `bomb` 同属一族，§5.10 第 775 行阶梯）。
fn is_bomb_class(primitive: &str) -> bool {
    matches!(
        primitive,
        "pattern.bomb" | "pattern.straight-flush" | "pattern.joker-bomb"
    )
}

/// `candidate` 能否压过 `last`（§5.13 第 894、895 行）。
///
/// 跟牌只在三种情形合法：
/// 1. 同 `pattern_id`：比 `patternKey = (countKey, rankKey)`，张数优先、再点数；
/// 2. 两者都属炸弹族：按 `synthetic_ladder` 位次比较；`bomb` 对 `bomb` 仍走
///    `bombOrder: count-then-rank`（先张数、同张数比点数）；
/// 3. 候选属炸弹族、上家不属：炸弹压任意非炸弹。
///
/// 其余（两个非同型的非炸弹牌型）一律不能压。`compare.ladder` 只是牌型位次表
/// （AI 排序 + flushRank 锚点插入位），不是跨型可压的合法性表。
pub(crate) fn beats(r: &RuleSetBody, candidate: &Recognized, last: &LastPlay) -> bool {
    let last_primitive = primitive_by_id(r, &last.pattern_id).unwrap_or("");
    let candidate_bomb = is_bomb_class(candidate.primitive);
    let last_bomb = is_bomb_class(last_primitive);

    // 1. 同牌型：比 patternKey。
    if candidate.pattern_id == last.pattern_id {
        return candidate.pattern_key.0 > last.pattern_key.0
            || (candidate.pattern_key.0 == last.pattern_key.0
                && candidate.pattern_key.1 > last.pattern_key.1);
    }

    // 2. 炸弹族对炸弹族：ladder 位次；bomb 对 bomb 走 bombOrder。
    if candidate_bomb && last_bomb {
        if candidate.primitive == "pattern.bomb" && last_primitive == "pattern.bomb" {
            return candidate.pattern_key.0 > last.pattern_key.0
                || (candidate.pattern_key.0 == last.pattern_key.0
                    && candidate.pattern_key.1 > last.pattern_key.1);
        }
        let ladder = synthetic_ladder(r);
        let pos = |id: &str| ladder.iter().position(|p| p.0 == id);
        return match (pos(&candidate.pattern_id), pos(last.pattern_id.as_str())) {
            (Some(a), Some(b)) => a > b,
            _ => false,
        };
    }

    // 3. 炸弹压任意非炸弹。
    if candidate_bomb {
        return true;
    }

    // 4. 非同型非炸弹：不能压。
    false
}
