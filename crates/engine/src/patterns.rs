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
//! - TODO(M1-HOLD-with-attachment)：三带二（`pattern.with-attachment`）。§5.2 第 495、
//!   547 行定为 v2 预留、引用即 L1 拒；§5.13 第 890 行牌型清单没有它；`guandan-classic`
//!   夹具也未声明。本批不识别。
//! - TODO(M1-HOLD-level-rank)：级牌点力重排。§5.13 第 892 行写「当前 `levels[team]`
//!   对应的牌…按高于 A 参与比较」，但每队一个 `levels`、两队档位可能不同，跨队比较时
//!   同一张牌点力不唯一；本批 `rankKey` 保持 `naturalOrder`，不按级牌高位重排。
//! - TODO(M1-GAP-levels-init)：`levels` 按队初始化尚未落地（`deal()` 置空序列），百搭
//!   自身点数暂按 `levelTable.initial` 解析；levels 初始化落地后，自身点数改为读
//!   `state.levels[team]`。此点与 level-rank hold 正交：B3 裁决前不做级牌高位插入。

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

// ---------- 逢人配定位 ----------

/// 逢人配牌 id 集合（源花色 + 源点数）。
fn wildcard_ids(r: &RuleSetBody) -> HashSet<u16> {
    let mut out = HashSet::new();
    let Some(w) = &r.wildcard else { return out };
    let Some(rank_idx) = wildcard_own_rank(r) else {
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

/// 百搭自身点数的下标（`@level` 占位符当前按 `levelTable.initial` 解析）。
///
/// TODO(M1-GAP-levels-init)：levels 按队初始化落地后，这里改为读 `state.levels[team]`；
/// 本函数签名目前拿不到 state，待接线层把当前队级牌传入。
fn wildcard_own_rank(r: &RuleSetBody) -> Option<usize> {
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

/// 一个可行解：解析后的高点位 `rank_key`、真牌点数组合（tiebreak）、提交牌原 id。
struct Sol {
    rank_key: u32,
    true_combo: Vec<usize>,
    cards: Vec<u16>,
}

/// 选解：`rankKey` 最大者胜；相同则真牌点数组合 `naturalOrder` 字典序最小者胜。
fn replace_best(best: &mut Option<Sol>, candidate: Sol) {
    let take = match best.as_ref() {
        None => true,
        Some(cur) => {
            candidate.rank_key > cur.rank_key
                || (candidate.rank_key == cur.rank_key && candidate.true_combo < cur.true_combo)
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
    fn build(r: &RuleSetBody, cards: &[u16]) -> Self {
        let wild_ids = wildcard_ids(r);
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

fn match_same_rank(r: &RuleSetBody, def: &PatternDef, count: u8, g: &Groups) -> Option<Recognized> {
    let k = count as usize;

    // 王只在单张牌型里出现；百搭不能代王。
    if !g.little.is_empty() || !g.big.is_empty() {
        if k == 1 && g.base.is_empty() && g.wild.is_empty() {
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
            let sol = Sol {
                rank_key: rank_key_of(r, rank),
                true_combo: vec![rank],
                cards,
            };
            replace_best(&mut best, sol);
        }
    }

    // 整手全百搭：按自身点数用，不走最大解。
    if g.base.is_empty() && k <= g.wild.len() {
        let Some(own) = wildcard_own_rank(r) else {
            return best.map(|sol| build_rec(def, k as u32, sol.rank_key, sol.cards));
        };
        let cards = g.wild[..k].to_vec();
        let sol = Sol {
            rank_key: rank_key_of(r, own),
            true_combo: Vec::new(),
            cards,
        };
        replace_best(&mut best, sol);
    }

    best.map(|sol| build_rec(def, k as u32, sol.rank_key, sol.cards))
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

    for (window, missing) in feasible_windows(rank_count(r), &present, length, g.wild.len()) {
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
        let Some(top) = consecutive_top(rank_count(r), &window, length) else {
            continue;
        };
        let mut cards = Vec::new();
        for &rank in &window {
            if let Some(bucket) = g.base.get(&rank) {
                cards.push(bucket[0]);
            }
        }
        cards.extend_from_slice(&g.wild[..missing.len()]);
        let mut true_combo: Vec<usize> = present.iter().copied().collect();
        true_combo.sort_unstable();
        let sol = Sol {
            rank_key: rank_key_of(r, top),
            true_combo,
            cards,
        };
        replace_best(&mut best, sol);
    }
    best.map(|sol| build_rec(def, length as u32, sol.rank_key, sol.cards))
}

fn match_consecutive(
    r: &RuleSetBody,
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
        let mut true_combo: Vec<usize> = present.iter().copied().collect();
        true_combo.sort_unstable();
        let sol = Sol {
            rank_key: rank_key_of(r, top),
            true_combo,
            cards,
        };
        replace_best(&mut best, sol);
    }
    best.map(|sol| build_rec(def, (groups * per_group) as u32, sol.rank_key, sol.cards))
}

fn match_joker_bomb(r: &RuleSetBody, def: &PatternDef, g: &Groups) -> Option<Recognized> {
    // 百搭不得代王，王炸不接受百搭。
    if !g.base.is_empty() || !g.wild.is_empty() || g.little.len() != 2 || g.big.len() != 2 {
        return None;
    }
    let mut cards = Vec::new();
    cards.extend_from_slice(&g.little);
    cards.extend_from_slice(&g.big);
    Some(build_rec(def, 4, joker_rank_key(r, true), cards))
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
        _ => true,
    }
}

fn match_def(r: &RuleSetBody, def: &PatternDef, g: &Groups) -> Option<Recognized> {
    if !submitted_len_ok(def, g) {
        return None;
    }
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
    wild: Vec<u16>,
    little: Vec<u16>,
    big: Vec<u16>,
}

impl HandGroups {
    fn build(r: &RuleSetBody, hand: &[u16]) -> Self {
        let wild_ids = wildcard_ids(r);
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
pub(crate) fn enumerate(r: &RuleSetBody, hand: &[u16]) -> Vec<Recognized> {
    let g = HandGroups::build(r, hand);
    let mut out: Vec<Recognized> = Vec::new();
    let mut emitted: HashSet<Vec<u16>> = HashSet::new();

    let mut push = |rec: Recognized| {
        if emitted.insert(rec.cards.clone()) {
            out.push(rec);
        }
    };

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
                    for &card in &g.wild {
                        if let Some(rec) = recognize(r, &[card]) {
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
                            if let Some(rec) = recognize(r, &cards) {
                                push(rec);
                            }
                        }
                    }
                    // 整手全百搭 → 自身点数。
                    if g.base.is_empty() && count <= g.wild.len() {
                        let cards = g.wild[..count].to_vec();
                        if let Some(rec) = recognize(r, &cards) {
                            push(rec);
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
                        if let Some(rec) = recognize(r, &cards) {
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
                    if let Some(rec) = recognize(r, &cards) {
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
                    if let Some(rec) = recognize(r, &cards) {
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
                    if let Some(rec) = recognize(r, &cards) {
                        push(rec);
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
