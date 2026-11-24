//! 牌型识别、候选枚举与压制比较（§5.2 参数表第 513–519 行、§5.10 取值表第 775 行、
//! §5.13 play 段第 890–895 行）。
//!
//! 判定以 `ruleset.patterns[]` 的已声明牌型为驱动：张数 / 长度 / 连对组数 / 钢板组数
//! 全部读夹具参数，代码不写死牌型 id 与取值；未声明的原语不产生候选。
//!
//! # 逢人配（§5.13 第 893 行、§5.10 第 773 行）
//!
//! - 红桃级牌（`wildcard.source`）可替代除大小王外任意一张牌，也可按自身点数
//!   （该队当前级牌）使用；一手内百搭张数不限（含整手全为百搭）。
//! - 含百搭时按「可达解中 `rankKey` 最大者」取解；`rankKey` 相同再按「真牌点数
//!   组合 `naturalOrder` 字典序最小」定唯一解；顺子多孔多解同此条。
//! - **整手全百搭**（提交牌全部是百搭）按自身点数用，不走最大解。
//! - 百搭不得替代大小王；`joker-bomb` 不接受百搭。
//! - `wildcard.inStraightFlush=true` 时百搭可入同花顺，`false` 时不得参与。
//!
//! # 仍未落地的 hold / 缺口
//!
//! - TODO(M1-HOLD-with-attachment)：三带二之外的 `pattern.with-attachment`。
//!   §5.2 第 547 行定为 v2 预留、引用即 L1 拒；本批不识别。

use crate::contract::PatternId;
use crate::hash::LastPlay;
use crate::ruleset::model::{LevelInsert, PatternDef, RankKey, RuleSetBody, WildcardRank};
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

/// 当前局牌力序上下文：打级队的级牌（`deck.ranks` 下标）。
///
/// 牌力序全场唯一一份，由 `levels[attackingTeam]` 的级牌 + `levelInsert` 合成
/// （§5.13 第 911、932 行）；`None` 表示 score-only 局无 `levelTable`，合成序
/// 退回 `naturalOrder`。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RankContext {
    level_rank: Option<usize>,
    /// 合成序位置表：`deck.ranks` 下标 → `synth_order` 下标。载入一次，避免每次
    /// `rankKey` 查询都重建 `naturalOrder + levelInsert` 序列（§5.13 第 911、932 行）。
    synth_pos: Vec<usize>,
}

impl RankContext {
    pub(crate) fn new(r: &RuleSetBody, level_rank: Option<usize>) -> Self {
        let order = Self::synth_order(r, level_rank);
        let synth_pos = r
            .deck
            .ranks
            .iter()
            .enumerate()
            .map(|(idx, key)| order.iter().position(|k| **k == *key).unwrap_or(idx))
            .collect();
        Self {
            level_rank,
            synth_pos,
        }
    }

    /// 级牌在 `deck.ranks` 中的下标（逢人配按自身点数使用时取这一份）。
    pub(crate) fn own_rank(&self) -> Option<usize> {
        self.level_rank
    }

    /// 载入期合成牌力序：`naturalOrder` 去掉级牌，按 `levelInsert` 插回
    /// `above-A`（A 之后、小王之前）或 `below-2`（2 之前）。
    fn synth_order(r: &RuleSetBody, level_rank: Option<usize>) -> Vec<&RankKey> {
        let mut order: Vec<&RankKey> = r.natural_order.iter().collect();
        let Some(lr) = level_rank else {
            return order;
        };
        let level_key = &r.deck.ranks[lr];
        let Some(pos) = order.iter().position(|k| **k == *level_key) else {
            return order;
        };
        order.remove(pos);
        let insert_at = match r.level_insert {
            LevelInsert::AboveA => {
                let a = r.deck.ranks.last().expect("deck.ranks 非空");
                order
                    .iter()
                    .position(|k| **k == *a)
                    .map(|p| p + 1)
                    .unwrap_or(order.len())
            }
            LevelInsert::Below2 => 0,
        };
        order.insert(insert_at, level_key);
        order
    }

    /// 真实点数的 `rankKey`：合成序下标 +1；`0` 是保留值（轮子特例），真实点数与
    /// 大小王不得映射到 `0`（§5.13 第 931 行）。散牌型（单张 / 对 / 三张 / 三带二主牌）
    /// 与炸弹走这套合成序。
    fn rank_key(&self, _r: &RuleSetBody, rank_idx: usize) -> u32 {
        let k = self.synth_pos[rank_idx] as u32 + 1;
        debug_assert!(k >= 1, "真实点数 rankKey 不得取保留值 0");
        k
    }

    /// 自然序 `rankKey`：组牌型（顺子 / 三连对 / 钢板 / 同花顺）只按自身自然点数位参与，
    /// 不经过 `levelInsert` 提升（第二十五次变更）；级牌 = 2 时 2 就是自然序最小位，
    /// 不存在「级牌当 A」的第二解，也不额外加权。两套序各在本 `patternId` 内可比，
    /// 跨型只按 `compare.ladder`，不做数值比较，两套键不得在同一比较里混用。
    fn natural_rank_key(&self, r: &RuleSetBody, rank_idx: usize) -> u32 {
        let key = &r.deck.ranks[rank_idx];
        let pos = r
            .natural_order
            .iter()
            .position(|k| k.0 == key.0)
            .unwrap_or(rank_idx);
        let k = pos as u32 + 1;
        debug_assert!(k >= 1, "真实点数 rankKey 不得取保留值 0");
        k
    }

    /// 大小王 `rankKey`：序尾两位，同样 +1 偏移（小王 < 大王）。
    fn joker_key(&self, r: &RuleSetBody, big: bool) -> u32 {
        let tail = r.natural_order.len();
        (tail - 2 + if big { 1 } else { 0 }) as u32 + 1
    }
}

// ---------- 逢人配定位 ----------

/// 逢人配牌 id 集合（源花色 + 源点数）。
pub(crate) fn wildcard_ids(r: &RuleSetBody, ctx: &RankContext) -> HashSet<u16> {
    let mut out = HashSet::new();
    let Some(w) = &r.wildcard else { return out };
    let Some(rank_idx) = wildcard_own_rank(r, ctx) else {
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

/// 百搭自身点数的下标：优先取 `levels[attackingTeam]` 的局级牌（唯一那份 ladder）；
/// 上下文未给级牌（score-only）时退回 `levelTable.initial` 占位。
fn wildcard_own_rank(r: &RuleSetBody, ctx: &RankContext) -> Option<usize> {
    if let Some(lr) = ctx.own_rank() {
        return Some(lr);
    }
    let w = r.wildcard.as_ref()?;
    let rank_str = match &w.source.rank {
        WildcardRank::Rank(k) => k.0.clone(),
        WildcardRank::LevelPlaceholder => r.level_table.as_ref()?.initial.0.clone(),
    };
    r.deck.ranks.iter().position(|k| k.0 == rank_str)
}

/// 百搭可替代的点数下标集；无百搭规则集或夹具未列 `substitute` 时按全部基础点数处理。
fn substitute_set(r: &RuleSetBody) -> HashSet<usize> {
    let Some(w) = &r.wildcard else {
        return (0..rank_count(r)).collect();
    };
    if w.substitute.is_empty() {
        return (0..rank_count(r)).collect();
    }
    w.substitute
        .iter()
        .filter_map(|k| r.deck.ranks.iter().position(|d| d.0 == k.0))
        .collect()
}

/// 百搭能否参与同花顺（`wildcard.inStraightFlush`，§5.10 第 773 行）。
fn wildcard_flush_allowed(r: &RuleSetBody) -> bool {
    r.wildcard
        .as_ref()
        .map(|w| w.in_straight_flush)
        .unwrap_or(false)
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
        PrimitiveParams::TriplePair => "pattern.triple-pair",
        _ => "reserved",
    }
}

/// 同手多解消歧（§5.13，`fa13b2a` 追认裁定，非推导）：同一手牌可判成多个牌型时，
/// 取 `compare.ladder`（含 straight-flush 合成位次）中位次最高者。
/// 边界：不引入客户端显式声明牌型；也不取位次最低者。现状覆盖顺子 / 连对 / 钢板 / 三带二
/// 之间的同手多解。
fn ladder_position(ladder: &[PatternId], def: &PatternDef) -> usize {
    ladder
        .iter()
        .position(|p| *p == def.id)
        .unwrap_or(usize::MAX)
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

/// 一个可行解：解析后的高点位 `rank_key`、真牌点数组合（tiebreak）、提交牌原 id。
struct Sol {
    rank_key: u32,
    /// 解中百搭张数（第二档：越少越规范）。
    wild_count: usize,
    /// 真牌牌 id 升序序列（第三档：字典序最小）。
    true_cards: Vec<u16>,
    cards: Vec<u16>,
}

fn make_sol(r: &RuleSetBody, ctx: &RankContext, rank_key: u32, cards: Vec<u16>) -> Sol {
    let wild = wildcard_ids(r, ctx);
    let mut true_cards: Vec<u16> = cards
        .iter()
        .copied()
        .filter(|c| !wild.contains(c))
        .collect();
    true_cards.sort_unstable();
    let wild_count = cards.len() - true_cards.len();
    Sol {
        rank_key,
        wild_count,
        true_cards,
        cards,
    }
}

/// 选解三档序（§5.13 第 899 行）：① `rankKey` 最大 ② 百搭用量最少 ③ 真牌牌
/// id 升序字典序最小（按牌 id 序列比，不按点数组合比）。
fn replace_best(best: &mut Option<Sol>, candidate: Sol) {
    let take = match best.as_ref() {
        None => true,
        Some(cur) => {
            candidate.rank_key > cur.rank_key
                || (candidate.rank_key == cur.rank_key
                    && (candidate.wild_count < cur.wild_count
                        || (candidate.wild_count == cur.wild_count
                            && candidate.true_cards < cur.true_cards)))
        }
    };
    if take {
        *best = Some(candidate);
    }
}

/// 按点数归组的提交牌 / 手牌。
struct Groups {
    /// 点数 → 该点数真牌（升序）。
    base: BTreeMap<usize, Vec<u16>>,
    /// 百搭牌（孔位填充用）。
    wild: Vec<u16>,
    little: Vec<u16>,
    big: Vec<u16>,
}

impl Groups {
    fn build(r: &RuleSetBody, ctx: &RankContext, cards: &[u16]) -> Self {
        let wild_ids = wildcard_ids(r, ctx);
        let mut base: BTreeMap<usize, Vec<u16>> = BTreeMap::new();
        let mut wild = Vec::new();
        let mut little = Vec::new();
        let mut big = Vec::new();
        for &card in cards {
            if wild_ids.contains(&card) {
                wild.push(card);
                continue;
            }
            match card_ref(r, card) {
                CardRef::Base { rank, .. } => base.entry(rank).or_default().push(card),
                CardRef::LittleJoker => little.push(card),
                CardRef::BigJoker => big.push(card),
            }
        }
        for v in base.values_mut() {
            v.sort_unstable();
        }
        wild.sort_unstable();
        little.sort_unstable();
        big.sort_unstable();
        Self {
            base,
            wild,
            little,
            big,
        }
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

/// 所有可容纳 `present` 真牌的连续窗口；缺孔数 ≤ `wild`。返回 (窗口点数序列, 缺孔点数)。
fn feasible_windows(
    rank_count: usize,
    present: &HashSet<usize>,
    length: usize,
    wild: usize,
) -> Vec<(Vec<usize>, Vec<usize>)> {
    let mut out = Vec::new();
    for start in 0..=rank_count.saturating_sub(length) {
        let window: Vec<usize> = (start..start + length).collect();
        if !present.iter().all(|p| window.contains(p)) {
            continue;
        }
        let missing: Vec<usize> = window
            .iter()
            .copied()
            .filter(|p| !present.contains(p))
            .collect();
        if missing.len() <= wild {
            out.push((window, missing));
        }
    }
    // 轮子顺 A-2-3-4-5 特例。
    if length == 5
        && present
            .iter()
            .all(|p| [0, 1, 2, 3, rank_count - 1].contains(p))
    {
        let window = vec![rank_count - 1, 0, 1, 2, 3];
        let missing: Vec<usize> = window
            .iter()
            .copied()
            .filter(|p| !present.contains(p))
            .collect();
        if missing.len() <= wild {
            out.push((window, missing));
        }
    }
    out
}

fn match_same_rank(
    r: &RuleSetBody,
    ctx: &RankContext,
    def: &PatternDef,
    count: u8,
    g: &Groups,
) -> Option<Recognized> {
    let k = count as usize;

    // 王只在单张牌型里出现；百搭不能代王。
    if !g.little.is_empty() || !g.big.is_empty() {
        if k == 1 && g.base.is_empty() && g.wild.is_empty() {
            if g.little.len() == 1 && g.big.is_empty() {
                return Some(build_rec(def, 1, ctx.joker_key(r, false), g.little.clone()));
            }
            if g.big.len() == 1 && g.little.is_empty() {
                return Some(build_rec(def, 1, ctx.joker_key(r, true), g.big.clone()));
            }
        }
        return None;
    }

    let mut best: Option<Sol> = None;
    let substitute = substitute_set(r);

    // 混合：真牌集中在一个点数，百搭补到该点数；其余真牌点数无法容纳则不成型。
    if g.base.len() == 1 {
        for (&rank, bucket) in &g.base {
            let t = bucket.len();
            if t > k || !substitute.contains(&rank) {
                continue;
            }
            let need = k - t;
            if need > g.wild.len() {
                continue;
            }
            let mut cards = bucket.clone();
            cards.extend_from_slice(&g.wild[..need]);
            let sol = make_sol(r, ctx, ctx.rank_key(r, rank), cards);
            replace_best(&mut best, sol);
        }
    }

    best.map(|sol| build_rec(def, k as u32, sol.rank_key, sol.cards))
}

fn match_bomb(
    r: &RuleSetBody,
    ctx: &RankContext,
    def: &PatternDef,
    count: u8,
    g: &Groups,
) -> Option<Recognized> {
    // 炸弹是同点数 4–8 张；结构与 same-rank 同，张数区间由夹具参数 + L1 值域闸门保证。
    match_same_rank(r, ctx, def, count, g)
}

fn match_straight(
    r: &RuleSetBody,
    ctx: &RankContext,
    def: &PatternDef,
    length: u8,
    g: &Groups,
    flush: bool,
) -> Option<Recognized> {
    let length = length as usize;
    if !g.no_jokers() {
        return None;
    }
    if flush && !wildcard_flush_allowed(r) && !g.wild.is_empty() {
        return None;
    }
    // 顺子每个点数至多一张真牌。
    if g.base.values().any(|b| b.len() > 1) {
        return None;
    }
    let present: HashSet<usize> = g.base.keys().copied().collect();
    let substitute = substitute_set(r);
    let mut best: Option<Sol> = None;
    let rc = rank_count(r);

    for (window, missing) in feasible_windows(rc, &present, length, g.wild.len()) {
        if !window.iter().all(|p| substitute.contains(p)) {
            continue;
        }
        // 同花顺：真牌必须同花色；全百搭无花色可定，不成同花顺。
        let mut suit_seen: Option<usize> = None;
        let mut same_suit = true;
        if flush {
            for bucket in g.base.values() {
                if let CardRef::Base { suit, .. } = card_ref(r, bucket[0]) {
                    match suit_seen {
                        None => suit_seen = Some(suit),
                        Some(s) if s != suit => {
                            same_suit = false;
                            break;
                        }
                        _ => {}
                    }
                }
            }
            if !same_suit || suit_seen.is_none() {
                continue;
            }
        }
        let Some(top) = consecutive_top(rc, &window, length) else {
            continue;
        };
        // 轮子特例（第十七次变更）：非同花 `A-2-3-4-5` 的 rankKey 恒取保留值 0，
        // 不论含不含级牌、是否百搭补位，不受 levelInsert 提升；同花顺不适用本特例。
        // 组牌型其余情形只按自然序顶牌取 rankKey（第二十五次变更），不走合成序。
        let mut window_sorted = window.clone();
        window_sorted.sort_unstable();
        let wheel = vec![0, 1, 2, 3, rc - 1];
        let is_wheel = length == 5 && window_sorted == wheel;
        let rank_key = if !flush && is_wheel {
            0
        } else {
            ctx.natural_rank_key(r, top)
        };
        let mut cards = Vec::new();
        for &rank in &window {
            if let Some(bucket) = g.base.get(&rank) {
                cards.push(bucket[0]);
            }
        }
        cards.extend_from_slice(&g.wild[..missing.len()]);
        let sol = make_sol(r, ctx, rank_key, cards);
        replace_best(&mut best, sol);
    }
    best.map(|sol| build_rec(def, length as u32, sol.rank_key, sol.cards))
}

fn match_consecutive(
    r: &RuleSetBody,
    ctx: &RankContext,
    def: &PatternDef,
    groups: usize,
    per_group: usize,
    g: &Groups,
) -> Option<Recognized> {
    if !g.no_jokers() {
        return None;
    }
    let present: HashSet<usize> = g.base.keys().copied().collect();
    let substitute = substitute_set(r);
    let mut best: Option<Sol> = None;

    for (window, _) in feasible_windows(rank_count(r), &present, groups, g.wild.len()) {
        if !window.iter().all(|p| substitute.contains(p)) {
            continue;
        }
        let mut deficit = 0usize;
        let mut ok = true;
        for &rank in &window {
            let t = g.base.get(&rank).map(|b| b.len()).unwrap_or(0);
            if t > per_group {
                ok = false;
                break;
            }
            deficit += per_group - t;
        }
        if !ok || deficit > g.wild.len() {
            continue;
        }
        let Some(top) = consecutive_top(rank_count(r), &window, groups) else {
            continue;
        };
        let mut cards = Vec::new();
        let mut used_wild = 0usize;
        for &rank in &window {
            let bucket = g.base.get(&rank).cloned().unwrap_or_default();
            cards.extend_from_slice(&bucket);
            let need = per_group - bucket.len();
            cards.extend_from_slice(&g.wild[used_wild..used_wild + need]);
            used_wild += need;
        }
        // 三连对 / 钢板属组牌型：rankKey 只按自然序顶牌（第二十五次变更），不经过合成序。
        let sol = make_sol(r, ctx, ctx.natural_rank_key(r, top), cards);
        replace_best(&mut best, sol);
    }
    best.map(|sol| build_rec(def, (groups * per_group) as u32, sol.rank_key, sol.cards))
}

fn match_joker_bomb(
    r: &RuleSetBody,
    ctx: &RankContext,
    def: &PatternDef,
    g: &Groups,
) -> Option<Recognized> {
    // 百搭不得代王，王炸不接受百搭。
    if !g.base.is_empty() || !g.wild.is_empty() || g.little.len() != 2 || g.big.len() != 2 {
        return None;
    }
    let mut cards = Vec::new();
    cards.extend_from_slice(&g.little);
    cards.extend_from_slice(&g.big);
    Some(build_rec(def, 4, ctx.joker_key(r, true), cards))
}

/// 三带二（§5.13 第 930 行）：3 张同点主牌 + 1 对，共 5 张。
///
/// - `rankKey` 只取主牌三张的点数，对子不进 `rankKey`；主牌三张不得是王。
/// - 对子可以是任意同点两张；双副牌下两张小王 / 两张大王成对亦可；两点数不同即非法。
/// - 百搭可代主牌或对子任一张；主牌点数至少保留一张真牌（否则主牌点数无可锚定）。
fn match_triple_pair(
    r: &RuleSetBody,
    ctx: &RankContext,
    def: &PatternDef,
    g: &Groups,
) -> Option<Recognized> {
    let substitute = substitute_set(r);
    let mut best: Option<Sol> = None;

    // 双王对子候选（两张小王或两张大王）；百搭不得代王。
    let joker_pairs: Vec<Vec<u16>> = [&g.little, &g.big]
        .iter()
        .filter(|j| j.len() == 2)
        .map(|j| j[..2].to_vec())
        .collect();

    for (&main, main_bucket) in &g.base {
        if !substitute.contains(&main) {
            continue;
        }
        let real_main = main_bucket.len();
        if real_main > 3 {
            continue;
        }
        let need_main = 3 - real_main;

        // 真牌对子：另一点数取前两张，缺孔由百搭补。
        for (&pair, pair_bucket) in &g.base {
            if pair == main {
                continue;
            }
            let take_pair = pair_bucket.len().min(2);
            let need_pair = 2 - take_pair;
            if need_main + need_pair > g.wild.len() {
                continue;
            }
            let mut cards = main_bucket[..real_main].to_vec();
            cards.extend_from_slice(&g.wild[..need_main]);
            cards.extend_from_slice(&pair_bucket[..take_pair]);
            cards.extend_from_slice(&g.wild[need_main..need_main + need_pair]);
            let sol = make_sol(r, ctx, ctx.rank_key(r, main), cards);
            replace_best(&mut best, sol);
        }

        // 双王对子。
        for jp in &joker_pairs {
            if need_main > g.wild.len() {
                continue;
            }
            let mut cards = main_bucket[..real_main].to_vec();
            cards.extend_from_slice(&g.wild[..need_main]);
            cards.extend_from_slice(jp);
            let sol = make_sol(r, ctx, ctx.rank_key(r, main), cards);
            replace_best(&mut best, sol);
        }
    }

    best.map(|sol| build_rec(def, 5, sol.rank_key, sol.cards))
}

/// 提交牌总数必须与牌型张数一致（多余百搭 / 散牌不得被匹配器忽略）。
fn submitted_len_ok(def: &PatternDef, g: &Groups) -> bool {
    let total =
        g.base.values().map(Vec::len).sum::<usize>() + g.wild.len() + g.little.len() + g.big.len();
    match &def.params {
        PrimitiveParams::SameRank { count } => total == *count as usize,
        PrimitiveParams::Straight { length, .. } => total == *length as usize,
        PrimitiveParams::ConsecutivePairs { pairs } => total == *pairs as usize * 2,
        PrimitiveParams::ConsecutiveTriples { triples } => total == *triples as usize * 3,
        PrimitiveParams::Bomb { count } => total == *count as usize,
        PrimitiveParams::StraightFlush { length } => total == *length as usize,
        PrimitiveParams::JokerBomb => total == 4,
        PrimitiveParams::TriplePair => total == 5,
        _ => true,
    }
}

fn match_def(
    r: &RuleSetBody,
    ctx: &RankContext,
    def: &PatternDef,
    g: &Groups,
) -> Option<Recognized> {
    if !submitted_len_ok(def, g) {
        return None;
    }
    match &def.params {
        PrimitiveParams::SameRank { count } => match_same_rank(r, ctx, def, *count, g),
        PrimitiveParams::Straight { length, .. } => match_straight(r, ctx, def, *length, g, false),
        PrimitiveParams::ConsecutivePairs { pairs } => {
            match_consecutive(r, ctx, def, *pairs as usize, 2, g)
        }
        PrimitiveParams::ConsecutiveTriples { triples } => {
            match_consecutive(r, ctx, def, *triples as usize, 3, g)
        }
        PrimitiveParams::Bomb { count } => match_bomb(r, ctx, def, *count, g),
        PrimitiveParams::StraightFlush { length } => match_straight(r, ctx, def, *length, g, true),
        PrimitiveParams::JokerBomb => match_joker_bomb(r, ctx, def, g),
        PrimitiveParams::TriplePair => match_triple_pair(r, ctx, def, g),
        // TODO(M1-HOLD-with-attachment)：v2 预留原语（§5.2 第 495/547 行、§5.13 第 890 行），
        // L1 已拒绝引用；运行期即便进入也不识别为任何牌型。
        PrimitiveParams::Reserved { .. } => None,
        _ => None,
    }
}

/// 整手全百搭独立分支（第二十一次变更，§5.13 逢人配行）：张数直接决定牌型——
/// 1 `single`、2 `pair`、3 `triple`、4–8 `bomb-N`；同一 `rankKey`（百搭自身点数），
/// 不得判成顺子 / 三连对 / 钢板 / 三带二 / 同花顺。超出已声明值域返回 `None`。
fn recognize_all_wildcard(r: &RuleSetBody, ctx: &RankContext, wild: &[u16]) -> Option<Recognized> {
    let n = wild.len();
    let own = wildcard_own_rank(r, ctx)?;
    let rank_key = ctx.rank_key(r, own);
    let def = r.patterns.iter().find(|p| match &p.params {
        PrimitiveParams::SameRank { count } => n <= 3 && *count as usize == n,
        PrimitiveParams::Bomb { count } => (4..=8).contains(&n) && *count as usize == n,
        _ => false,
    })?;
    Some(build_rec(def, n as u32, rank_key, wild.to_vec()))
}

/// 按提交牌结构判定牌型；不匹配任何已声明牌型返回 `None`（§5.9：`patternId` 服务端判定）。
///
/// `ladder` 由引擎载入期缓存传入（`synthetic_ladder` 结果），识别侧不再每次重建。
pub(crate) fn recognize(
    ladder: &[PatternId],
    r: &RuleSetBody,
    ctx: &RankContext,
    cards: &[u16],
) -> Option<Recognized> {
    if cards.is_empty() {
        return None;
    }
    let mut seen = HashSet::new();
    if !cards.iter().all(|c| seen.insert(*c)) {
        return None;
    }
    let groups = Groups::build(r, ctx, cards);
    // 整手全百搭：独立分支，不走通用同点牌路径。
    if groups.base.is_empty()
        && groups.little.is_empty()
        && groups.big.is_empty()
        && !groups.wild.is_empty()
    {
        return recognize_all_wildcard(r, ctx, &groups.wild);
    }
    let mut defs: Vec<(&PatternDef, usize)> = r
        .patterns
        .iter()
        .filter(|p| !matches!(p.params, PrimitiveParams::Reserved { .. }))
        .map(|p| (p, ladder_position(ladder, p)))
        .collect();
    // 同手多解：ladder 位次最高者先匹配（§5.13 第 929 行）。
    defs.sort_by_key(|(_, pos)| std::cmp::Reverse(*pos));
    for (def, _) in defs {
        if let Some(rec) = match_def(r, ctx, def, &groups) {
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
    wild: Vec<u16>,
    little: Vec<u16>,
    big: Vec<u16>,
}

impl HandGroups {
    fn build(r: &RuleSetBody, ctx: &RankContext, hand: &[u16]) -> Self {
        let wild_ids = wildcard_ids(r, ctx);
        let mut base: BTreeMap<usize, Vec<u16>> = BTreeMap::new();
        let mut by_suit: BTreeMap<usize, BTreeMap<usize, Vec<u16>>> = BTreeMap::new();
        let mut wild = Vec::new();
        let mut little = Vec::new();
        let mut big = Vec::new();
        for &card in hand {
            if wild_ids.contains(&card) {
                wild.push(card);
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
        wild.sort_unstable();
        little.sort_unstable();
        big.sort_unstable();
        Self {
            base,
            by_suit,
            wild,
            little,
            big,
        }
    }
}

fn pick_one_per_rank(base: &BTreeMap<usize, Vec<u16>>, ranks: &[usize]) -> Vec<u16> {
    pick_n_per_rank(base, ranks, 1)
}

/// 每个点数取前 `n` 张（连对 n=2、钢板 n=3）。
fn pick_n_per_rank(base: &BTreeMap<usize, Vec<u16>>, ranks: &[usize], n: usize) -> Vec<u16> {
    let mut cards = Vec::new();
    for &rank in ranks {
        if let Some(bucket) = base.get(&rank) {
            let take = n.min(bucket.len());
            cards.extend_from_slice(&bucket[..take]);
        }
    }
    cards
}

/// 按窗口构造连对 / 钢板候选：每点数取前 `per_group` 张，缺孔用百搭前缀补齐。
fn consecutive_cards(
    base: &BTreeMap<usize, Vec<u16>>,
    wild: &[u16],
    window: &[usize],
    per_group: usize,
) -> Vec<u16> {
    let mut cards = Vec::new();
    let mut used = 0usize;
    for &rank in window {
        let bucket = base.get(&rank).cloned().unwrap_or_default();
        let take = per_group.min(bucket.len());
        cards.extend_from_slice(&bucket[..take]);
        let need = per_group - take;
        cards.extend_from_slice(&wild[used..used + need]);
        used += need;
    }
    cards
}

/// 枚举侧全窗口：候选是手牌子集，窗内不要求容纳全部手牌点数（识别侧仍用
/// `feasible_windows` 约束提交牌）。含轮子顺 A-2-3-4-5 特例。
fn all_windows(rank_count: usize, length: usize) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = (0..=rank_count.saturating_sub(length))
        .map(|s| (s..s + length).collect())
        .collect();
    if length == 5 {
        out.push(vec![rank_count - 1, 0, 1, 2, 3]);
    }
    out
}

/// 枚举手牌中所有可成型的牌型候选（领出 / 跟牌的压过过滤在接线层做）。
///
/// 枚举走「孔位填充 + 剪枝」：百搭不做全排列，孔位由窗口 / 目标点数唯一确定，
/// 百搭副本不可区分时只取规范前缀；同一 (牌型, 解) 只发一条。
///
/// 构造按牌型解析直取：同点数型按 `rank` 取桶、连续型按合法窗口滑动、含百搭按孔位
/// 定向补齐；构造完成后只经一次结构校验（[`recognize`]），`ladder` 由引擎载入期缓存传入。
pub(crate) fn enumerate(
    ladder: &[PatternId],
    r: &RuleSetBody,
    ctx: &RankContext,
    hand: &[u16],
) -> Vec<Recognized> {
    let g = HandGroups::build(r, ctx, hand);
    let mut out: Vec<Recognized> = Vec::new();
    let mut emitted: HashSet<Vec<u16>> = HashSet::new();

    let mut push = |rec: Recognized| {
        if emitted.insert(rec.cards.clone()) {
            out.push(rec);
        }
    };

    // 整手全百搭：独立分支按张数输出 single/pair/triple/bomb-N，不进入通用枚举。
    if g.base.is_empty() && g.little.is_empty() && g.big.is_empty() && !g.wild.is_empty() {
        for n in 1..=g.wild.len() {
            let cards = g.wild[..n].to_vec();
            if let Some(rec) = recognize(ladder, r, ctx, &cards) {
                push(rec);
            }
        }
        return out;
    }

    // 按 ladder 位次输出：更具体的牌型先占位，同一札多解时不被泛化牌型顶掉。
    let mut defs: Vec<(&PatternDef, usize)> = r
        .patterns
        .iter()
        .filter(|p| !matches!(p.params, PrimitiveParams::Reserved { .. }))
        .map(|p| (p, ladder_position(ladder, p)))
        .collect();
    defs.sort_by_key(|(_, pos)| std::cmp::Reverse(*pos));
    for (def, _) in defs {
        match &def.params {
            PrimitiveParams::SameRank { count } => {
                let count = *count as usize;
                if count == 1 {
                    for bucket in g.base.values() {
                        for &card in bucket {
                            if let Some(rec) = recognize(ladder, r, ctx, &[card]) {
                                push(rec);
                            }
                        }
                    }
                    for &card in &g.little {
                        if let Some(rec) = recognize(ladder, r, ctx, &[card]) {
                            push(rec);
                        }
                    }
                    for &card in &g.big {
                        if let Some(rec) = recognize(ladder, r, ctx, &[card]) {
                            push(rec);
                        }
                    }
                    for &card in &g.wild {
                        if let Some(rec) = recognize(ladder, r, ctx, &[card]) {
                            push(rec);
                        }
                    }
                } else {
                    for bucket in g.base.values() {
                        let need = count.saturating_sub(bucket.len());
                        if need <= g.wild.len() {
                            let take = count.min(bucket.len());
                            let mut cards = bucket[..take].to_vec();
                            cards.extend_from_slice(&g.wild[..need]);
                            if let Some(rec) = recognize(ladder, r, ctx, &cards) {
                                push(rec);
                            }
                        }
                    }
                }
            }
            PrimitiveParams::Bomb { count } => {
                let count = *count as usize;
                for bucket in g.base.values() {
                    let need = count.saturating_sub(bucket.len());
                    if need <= g.wild.len() {
                        let take = count.min(bucket.len());
                        let mut cards = bucket[..take].to_vec();
                        cards.extend_from_slice(&g.wild[..need]);
                        if let Some(rec) = recognize(ladder, r, ctx, &cards) {
                            push(rec);
                        }
                    }
                }
            }
            PrimitiveParams::Straight { length, .. } => {
                for window in all_windows(rank_count(r), *length as usize) {
                    let missing: Vec<usize> = window
                        .iter()
                        .copied()
                        .filter(|p| !g.base.contains_key(p))
                        .collect();
                    if missing.len() > g.wild.len() {
                        continue;
                    }
                    let mut cards = pick_one_per_rank(&g.base, &window);
                    cards.extend_from_slice(&g.wild[..missing.len()]);
                    if let Some(rec) = recognize(ladder, r, ctx, &cards) {
                        push(rec);
                    }
                }
            }
            PrimitiveParams::ConsecutivePairs { pairs } => {
                let groups = *pairs as usize;
                for window in all_windows(rank_count(r), groups) {
                    if !window_wildcard_fits(&g.base, &window, 2, g.wild.len()) {
                        continue;
                    }
                    let cards = consecutive_cards(&g.base, &g.wild, &window, 2);
                    if let Some(rec) = recognize(ladder, r, ctx, &cards) {
                        push(rec);
                    }
                }
            }
            PrimitiveParams::ConsecutiveTriples { triples } => {
                let groups = *triples as usize;
                for window in all_windows(rank_count(r), groups) {
                    if !window_wildcard_fits(&g.base, &window, 3, g.wild.len()) {
                        continue;
                    }
                    let cards = consecutive_cards(&g.base, &g.wild, &window, 3);
                    if let Some(rec) = recognize(ladder, r, ctx, &cards) {
                        push(rec);
                    }
                }
            }
            PrimitiveParams::TriplePair => {
                // 主牌三张 + 对子（真牌点对 / 双王对），百搭按缺口分配。
                let joker_pairs: Vec<Vec<u16>> = [&g.little, &g.big]
                    .iter()
                    .filter(|j| j.len() == 2)
                    .map(|j| j[..2].to_vec())
                    .collect();
                for (&main, main_bucket) in &g.base {
                    let real_main = main_bucket.len().min(3);
                    let need_main = 3 - real_main;
                    for (&pair, pair_bucket) in &g.base {
                        if pair == main {
                            continue;
                        }
                        let take_pair = pair_bucket.len().min(2);
                        let need_pair = 2 - take_pair;
                        if need_main + need_pair > g.wild.len() {
                            continue;
                        }
                        let mut cards = main_bucket[..real_main].to_vec();
                        cards.extend_from_slice(&g.wild[..need_main]);
                        cards.extend_from_slice(&pair_bucket[..take_pair]);
                        cards.extend_from_slice(&g.wild[need_main..need_main + need_pair]);
                        if let Some(rec) = recognize(ladder, r, ctx, &cards) {
                            push(rec);
                        }
                    }
                    for jp in &joker_pairs {
                        if need_main > g.wild.len() {
                            continue;
                        }
                        let mut cards = main_bucket[..real_main].to_vec();
                        cards.extend_from_slice(&g.wild[..need_main]);
                        cards.extend_from_slice(jp);
                        if let Some(rec) = recognize(ladder, r, ctx, &cards) {
                            push(rec);
                        }
                    }
                }
            }
            PrimitiveParams::StraightFlush { length } => {
                for suit_buckets in g.by_suit.values() {
                    for window in all_windows(rank_count(r), *length as usize) {
                        // 全百搭同花顺无花色可定；gate 关闭时百搭不入同花顺。
                        if suit_buckets.is_empty()
                            || (!wildcard_flush_allowed(r) && !g.wild.is_empty())
                        {
                            continue;
                        }
                        let missing: Vec<usize> = window
                            .iter()
                            .copied()
                            .filter(|p| !suit_buckets.contains_key(p))
                            .collect();
                        if missing.len() > g.wild.len() {
                            continue;
                        }
                        let mut cards = pick_one_per_rank(suit_buckets, &window);
                        cards.extend_from_slice(&g.wild[..missing.len()]);
                        if let Some(rec) = recognize(ladder, r, ctx, &cards) {
                            push(rec);
                        }
                    }
                }
            }
            PrimitiveParams::JokerBomb if g.little.len() >= 2 && g.big.len() >= 2 => {
                let mut cards = Vec::new();
                cards.extend_from_slice(&g.little[..2]);
                cards.extend_from_slice(&g.big[..2]);
                if let Some(rec) = recognize(ladder, r, ctx, &cards) {
                    push(rec);
                }
            }
            PrimitiveParams::JokerBomb => {}
            _ => {}
        }
    }
    out
}

/// 窗口内每个点数的缺孔总和 ≤ 百搭数；真牌超额不影响（枚举只取前缀）。
fn window_wildcard_fits(
    base: &BTreeMap<usize, Vec<u16>>,
    window: &[usize],
    per_group: usize,
    wild: usize,
) -> bool {
    let mut deficit = 0usize;
    for &rank in window {
        let t = base.get(&rank).map(|b| b.len()).unwrap_or(0);
        deficit += per_group.saturating_sub(t);
    }
    deficit <= wild
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
pub(crate) fn beats(
    ladder: &[PatternId],
    r: &RuleSetBody,
    candidate: &Recognized,
    last: &LastPlay,
) -> bool {
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

// ---------- 参照实现（仅 `#[cfg(test)]` 编译，不进发布产物） ----------

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod legacy_ref {
    //! 重构前的枚举 / 识别路径留档。差分测试用它与生产路径逐张比对
    //! `Vec<Action>`（含顺序）；旧实现每次调用都重建 `synthetic_ladder`，
    //! 与重构前行为一致。本模块不进发布产物。
    use super::*;
    use crate::ruleset::ladder::synthetic_ladder;

    fn legacy_ladder_position(r: &RuleSetBody, def: &PatternDef) -> usize {
        let ladder = synthetic_ladder(r);
        ladder
            .iter()
            .position(|p| *p == def.id)
            .unwrap_or(usize::MAX)
    }

    pub(crate) fn recognize_legacy(
        r: &RuleSetBody,
        ctx: &RankContext,
        cards: &[u16],
    ) -> Option<Recognized> {
        if cards.is_empty() {
            return None;
        }
        let mut seen = HashSet::new();
        if !cards.iter().all(|c| seen.insert(*c)) {
            return None;
        }
        let groups = Groups::build(r, ctx, cards);
        if groups.base.is_empty()
            && groups.little.is_empty()
            && groups.big.is_empty()
            && !groups.wild.is_empty()
        {
            return recognize_all_wildcard(r, ctx, &groups.wild);
        }
        let mut defs: Vec<(&PatternDef, usize)> = r
            .patterns
            .iter()
            .filter(|p| !matches!(p.params, PrimitiveParams::Reserved { .. }))
            .map(|p| (p, legacy_ladder_position(r, p)))
            .collect();
        defs.sort_by_key(|(_, pos)| std::cmp::Reverse(*pos));
        for (def, _) in defs {
            if let Some(rec) = match_def(r, ctx, def, &groups) {
                return Some(rec);
            }
        }
        None
    }

    pub(crate) fn enumerate_legacy(
        r: &RuleSetBody,
        ctx: &RankContext,
        hand: &[u16],
    ) -> Vec<Recognized> {
        let g = HandGroups::build(r, ctx, hand);
        let mut out: Vec<Recognized> = Vec::new();
        let mut emitted: HashSet<Vec<u16>> = HashSet::new();

        let mut push = |rec: Recognized| {
            if emitted.insert(rec.cards.clone()) {
                out.push(rec);
            }
        };

        if g.base.is_empty() && g.little.is_empty() && g.big.is_empty() && !g.wild.is_empty() {
            for n in 1..=g.wild.len() {
                let cards = g.wild[..n].to_vec();
                if let Some(rec) = recognize_legacy(r, ctx, &cards) {
                    push(rec);
                }
            }
            return out;
        }

        let mut defs: Vec<(&PatternDef, usize)> = r
            .patterns
            .iter()
            .filter(|p| !matches!(p.params, PrimitiveParams::Reserved { .. }))
            .map(|p| (p, legacy_ladder_position(r, p)))
            .collect();
        defs.sort_by_key(|(_, pos)| std::cmp::Reverse(*pos));
        for (def, _) in defs {
            match &def.params {
                PrimitiveParams::SameRank { count } => {
                    let count = *count as usize;
                    if count == 1 {
                        for bucket in g.base.values() {
                            for &card in bucket {
                                if let Some(rec) = recognize_legacy(r, ctx, &[card]) {
                                    push(rec);
                                }
                            }
                        }
                        for &card in &g.little {
                            if let Some(rec) = recognize_legacy(r, ctx, &[card]) {
                                push(rec);
                            }
                        }
                        for &card in &g.big {
                            if let Some(rec) = recognize_legacy(r, ctx, &[card]) {
                                push(rec);
                            }
                        }
                        for &card in &g.wild {
                            if let Some(rec) = recognize_legacy(r, ctx, &[card]) {
                                push(rec);
                            }
                        }
                    } else {
                        for bucket in g.base.values() {
                            let need = count.saturating_sub(bucket.len());
                            if need <= g.wild.len() {
                                let take = count.min(bucket.len());
                                let mut cards = bucket[..take].to_vec();
                                cards.extend_from_slice(&g.wild[..need]);
                                if let Some(rec) = recognize_legacy(r, ctx, &cards) {
                                    push(rec);
                                }
                            }
                        }
                    }
                }
                PrimitiveParams::Bomb { count } => {
                    let count = *count as usize;
                    for bucket in g.base.values() {
                        let need = count.saturating_sub(bucket.len());
                        if need <= g.wild.len() {
                            let take = count.min(bucket.len());
                            let mut cards = bucket[..take].to_vec();
                            cards.extend_from_slice(&g.wild[..need]);
                            if let Some(rec) = recognize_legacy(r, ctx, &cards) {
                                push(rec);
                            }
                        }
                    }
                }
                PrimitiveParams::Straight { length, .. } => {
                    for window in all_windows(rank_count(r), *length as usize) {
                        let missing: Vec<usize> = window
                            .iter()
                            .copied()
                            .filter(|p| !g.base.contains_key(p))
                            .collect();
                        if missing.len() > g.wild.len() {
                            continue;
                        }
                        let mut cards = pick_one_per_rank(&g.base, &window);
                        cards.extend_from_slice(&g.wild[..missing.len()]);
                        if let Some(rec) = recognize_legacy(r, ctx, &cards) {
                            push(rec);
                        }
                    }
                }
                PrimitiveParams::ConsecutivePairs { pairs } => {
                    let groups = *pairs as usize;
                    for window in all_windows(rank_count(r), groups) {
                        if !window_wildcard_fits(&g.base, &window, 2, g.wild.len()) {
                            continue;
                        }
                        let cards = consecutive_cards(&g.base, &g.wild, &window, 2);
                        if let Some(rec) = recognize_legacy(r, ctx, &cards) {
                            push(rec);
                        }
                    }
                }
                PrimitiveParams::ConsecutiveTriples { triples } => {
                    let groups = *triples as usize;
                    for window in all_windows(rank_count(r), groups) {
                        if !window_wildcard_fits(&g.base, &window, 3, g.wild.len()) {
                            continue;
                        }
                        let cards = consecutive_cards(&g.base, &g.wild, &window, 3);
                        if let Some(rec) = recognize_legacy(r, ctx, &cards) {
                            push(rec);
                        }
                    }
                }
                PrimitiveParams::TriplePair => {
                    let joker_pairs: Vec<Vec<u16>> = [&g.little, &g.big]
                        .iter()
                        .filter(|j| j.len() == 2)
                        .map(|j| j[..2].to_vec())
                        .collect();
                    for (&main, main_bucket) in &g.base {
                        let real_main = main_bucket.len().min(3);
                        let need_main = 3 - real_main;
                        for (&pair, pair_bucket) in &g.base {
                            if pair == main {
                                continue;
                            }
                            let take_pair = pair_bucket.len().min(2);
                            let need_pair = 2 - take_pair;
                            if need_main + need_pair > g.wild.len() {
                                continue;
                            }
                            let mut cards = main_bucket[..real_main].to_vec();
                            cards.extend_from_slice(&g.wild[..need_main]);
                            cards.extend_from_slice(&pair_bucket[..take_pair]);
                            cards.extend_from_slice(&g.wild[need_main..need_main + need_pair]);
                            if let Some(rec) = recognize_legacy(r, ctx, &cards) {
                                push(rec);
                            }
                        }
                        for jp in &joker_pairs {
                            if need_main > g.wild.len() {
                                continue;
                            }
                            let mut cards = main_bucket[..real_main].to_vec();
                            cards.extend_from_slice(&g.wild[..need_main]);
                            cards.extend_from_slice(jp);
                            if let Some(rec) = recognize_legacy(r, ctx, &cards) {
                                push(rec);
                            }
                        }
                    }
                }
                PrimitiveParams::StraightFlush { length } => {
                    for suit_buckets in g.by_suit.values() {
                        for window in all_windows(rank_count(r), *length as usize) {
                            if suit_buckets.is_empty()
                                || (!wildcard_flush_allowed(r) && !g.wild.is_empty())
                            {
                                continue;
                            }
                            let missing: Vec<usize> = window
                                .iter()
                                .copied()
                                .filter(|p| !suit_buckets.contains_key(p))
                                .collect();
                            if missing.len() > g.wild.len() {
                                continue;
                            }
                            let mut cards = pick_one_per_rank(suit_buckets, &window);
                            cards.extend_from_slice(&g.wild[..missing.len()]);
                            if let Some(rec) = recognize_legacy(r, ctx, &cards) {
                                push(rec);
                            }
                        }
                    }
                }
                PrimitiveParams::JokerBomb if g.little.len() >= 2 && g.big.len() >= 2 => {
                    let mut cards = Vec::new();
                    cards.extend_from_slice(&g.little[..2]);
                    cards.extend_from_slice(&g.big[..2]);
                    if let Some(rec) = recognize_legacy(r, ctx, &cards) {
                        push(rec);
                    }
                }
                PrimitiveParams::JokerBomb => {}
                _ => {}
            }
        }
        out
    }
}
