//! `flushRank` 锚点解析与载入期合成阶梯（§5.1 第 389 行、§5.13 第 878 行）。
//!
//! 锚点按语义认、不按 id 字符串认：在 `compare.ladder` 中找
//! `primitiveId = pattern.bomb` 且 `params.count` 等于锚点张数的那一项。
//! `below-bomb-5` 把插入型牌型插在锚点之前，其余三值插在锚点之后。

use crate::contract::PatternId;
use crate::ruleset::model::{FlushRank, PatternDef, RuleSetBody};
use crate::ruleset::params::PrimitiveParams;

/// 锚点张数：`flushRank` 取值里的炸弹张数（§5.1 第 389 行）。
///
/// `below-bomb-5` / `above-bomb-5` → 5，`above-bomb-6` → 6，`above-bomb-8` → 8。
pub fn anchor_count(flush: FlushRank) -> u8 {
    match flush {
        FlushRank::BelowBomb5 | FlushRank::AboveBomb5 => 5,
        FlushRank::AboveBomb6 => 6,
        FlushRank::AboveBomb8 => 8,
    }
}

/// 锚点插入侧：`below-bomb-5` 插在锚点之前，其余三值插在锚点之后。
fn insert_before(flush: FlushRank) -> bool {
    matches!(flush, FlushRank::BelowBomb5)
}

/// 按 id 解析 `patterns[]` 中的牌型定义。
pub fn find_pattern<'a>(ruleset: &'a RuleSetBody, id: &PatternId) -> Option<&'a PatternDef> {
    ruleset.patterns.iter().find(|p| &p.id == id)
}

/// 插入型牌型（`pattern.straight-flush`）定义列表，按声明序。
pub fn flush_patterns(ruleset: &RuleSetBody) -> Vec<&PatternDef> {
    ruleset
        .patterns
        .iter()
        .filter(|p| matches!(p.params, PrimitiveParams::StraightFlush { .. }))
        .collect()
}

/// `ladder` 中出现的炸弹张数列表（按 ladder 序；仅认 `pattern.bomb` + `params.count`）。
pub fn ladder_bomb_counts(ruleset: &RuleSetBody) -> Vec<u8> {
    ruleset
        .compare
        .ladder
        .iter()
        .filter_map(|pid| {
            find_pattern(ruleset, pid).and_then(|p| match p.params {
                PrimitiveParams::Bomb { count } => Some(count),
                _ => None,
            })
        })
        .collect()
}

/// 在 `ladder` 中定位锚点炸弹（`pattern.bomb` 且 `count == 锚点张数`），返回 ladder 下标。
pub fn find_anchor_index(ruleset: &RuleSetBody) -> Option<usize> {
    let anchor = anchor_count(ruleset.compare.flush_rank);
    ruleset.compare.ladder.iter().position(|pid| {
        find_pattern(ruleset, pid)
            .is_some_and(|p| matches!(p.params, PrimitiveParams::Bomb { count } if count == anchor))
    })
}

/// 合成阶梯：把插入型牌型按 `flushRank` 插到锚点之前 / 之后，返回完整牌力阶梯的 id 序列。
///
/// 锚点缺失或无插入型牌型时原样返回 ladder（校验路径应已先报
/// `RS_FLUSH_ANCHOR_MISSING`；本函数不做降级插入）。
pub fn synthetic_ladder(ruleset: &RuleSetBody) -> Vec<PatternId> {
    let sf: Vec<PatternId> = flush_patterns(ruleset)
        .iter()
        .map(|p| p.id.clone())
        .collect();
    if sf.is_empty() {
        return ruleset.compare.ladder.clone();
    }
    let Some(anchor) = find_anchor_index(ruleset) else {
        return ruleset.compare.ladder.clone();
    };

    let before = insert_before(ruleset.compare.flush_rank);
    let mut out = Vec::with_capacity(ruleset.compare.ladder.len() + sf.len());
    for (i, pid) in ruleset.compare.ladder.iter().enumerate() {
        if i == anchor {
            if before {
                out.extend(sf.iter().cloned());
            }
            out.push(pid.clone());
            if !before {
                out.extend(sf.iter().cloned());
            }
        } else {
            out.push(pid.clone());
        }
    }
    out
}
