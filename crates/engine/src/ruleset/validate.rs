//! L1 结构校验（§5.4 第 569 行、§5.1 / §5.2 取值范围）。
//!
//! 本轮只实现 L1；L2–L4 本轮不实现：请求了未实现的级别不记入 `ran_levels`
//! （`ran_levels` 语义是「实际跑完的级别」，§5.4 第 578 行），不伪造通过。

use crate::contract::{ErrorCode, Finding, Level, Validation};
use crate::ruleset::ladder;
use crate::ruleset::model::{FlushRank, LegalFrom, RuleSetBody};
use crate::ruleset::params::PrimitiveParams;
use std::collections::HashSet;

pub fn validate(ruleset: &RuleSetBody, levels: &[Level]) -> Validation {
    let mut result = Validation::default();

    let run_l1 = levels.contains(&Level::L1);
    if !run_l1 {
        // L2–L4 本轮未实现：未跑完的级别不进 ran_levels。
        return result;
    }
    result.ran_levels.insert(Level::L1);

    let mut push = |code: ErrorCode, location: &str, message: String| {
        result.findings.push(Finding {
            level: Level::L1,
            code,
            severity: code.default_severity(),
            location: location.to_owned(),
            message,
        });
    };

    // —— seats.count 2–8（§5.1 第 380 行、§5.4 L1） ——
    if !(2..=8).contains(&ruleset.seats.count) {
        push(
            ErrorCode::RsSchemaInvalid,
            "seats.count",
            format!("seats.count 必须在 2–8 之间，实际 {}", ruleset.seats.count),
        );
    }

    // —— timing 三键区间（§5.1 第 396 行） ——
    if let Some(timing) = &ruleset.timing {
        if !(5..=120).contains(&timing.turn_time_sec) {
            push(
                ErrorCode::RsSchemaInvalid,
                "timing.turnTimeSec",
                format!(
                    "turnTimeSec 必须在 5–120 秒之间，实际 {}",
                    timing.turn_time_sec
                ),
            );
        }
        if !(30..=180).contains(&timing.grace_sec) {
            push(
                ErrorCode::RsSchemaInvalid,
                "timing.graceSec",
                format!("graceSec 必须在 30–180 秒之间，实际 {}", timing.grace_sec),
            );
        }
        if !(60..=600).contains(&timing.offline_all_sec) {
            push(
                ErrorCode::RsSchemaInvalid,
                "timing.offlineAllSec",
                format!(
                    "offlineAllSec 必须在 60–600 秒之间，实际 {}",
                    timing.offline_all_sec
                ),
            );
        }
    }

    // —— matchCap.rounds 1–99（§5.1 第 394 行） ——
    if let Some(cap) = &ruleset.match_cap {
        if !(1..=99).contains(&cap.rounds) {
            push(
                ErrorCode::RsSchemaInvalid,
                "matchCap.rounds",
                format!("matchCap.rounds 必须在 1–99 之间，实际 {}", cap.rounds),
            );
        }
    }

    // —— ladder 不得混入 straight-flush-*（§5.1 第 388 行、§5.4 L1） ——
    for pid in &ruleset.compare.ladder {
        if pid.0.starts_with("straight-flush-") {
            push(
                ErrorCode::RsSchemaInvalid,
                "compare.ladder",
                format!("ladder 不得插入型牌型 `{}`，其位次由 flushRank 合成", pid.0),
            );
        }
    }

    // —— levelTable.upgrade[]：teamRanks 不得重复、step 1–3（§5.1 第 383 行） ——
    if let Some(table) = &ruleset.level_table {
        let mut seen: HashSet<[u8; 2]> = HashSet::new();
        for row in &table.upgrade {
            if !seen.insert(row.team_ranks) {
                push(
                    ErrorCode::RsSchemaInvalid,
                    "levelTable.upgrade",
                    format!("teamRanks {:?} 重复出现", row.team_ranks),
                );
            }
            if !(1..=3).contains(&row.step) {
                push(
                    ErrorCode::RsSchemaInvalid,
                    "levelTable.upgrade.step",
                    format!("step 必须在 1–3 之间，实际 {}", row.step),
                );
            }
        }
    }

    // —— level.pass.combos / streakLimit（§5.1 第 384 行、§5.2 第 537 行） ——
    // G2（待裁决）：§5.1/§5.2 两处写 1–3，§5.4 第 569 行写「1–9」；按两处一致的 1–3 实现，
    // §5.4 那处记为笔误待裁决。
    if let Some(level) = &ruleset.level {
        if level.pass.combos.is_empty() {
            push(
                ErrorCode::RsSchemaInvalid,
                "level.pass.combos",
                "pass.combos 不得为空表".to_owned(),
            );
        }
        for combo in &level.pass.combos {
            let [a, b] = *combo;
            if !(1..=4).contains(&a) || !(1..=4).contains(&b) {
                push(
                    ErrorCode::RsSchemaInvalid,
                    "level.pass.combos",
                    format!("名次组合 {:?} 元素必须在 1–4 之间", combo),
                );
            }
            if a == b {
                push(
                    ErrorCode::RsSchemaInvalid,
                    "level.pass.combos",
                    format!("名次组合 {:?} 两值不得相同", combo),
                );
            }
        }
        if !(1..=3).contains(&level.pass.streak_limit) {
            push(
                ErrorCode::RsSchemaInvalid,
                "level.pass.streakLimit",
                format!(
                    "streakLimit 必须在 1–3 之间，实际 {}",
                    level.pass.streak_limit
                ),
            );
        }
    }

    // —— deck：jokers 固定 2；ranks 不含王（§5.1 第 381 行） ——
    if ruleset.deck.jokers != 2 {
        push(
            ErrorCode::RsSchemaInvalid,
            "deck.jokers",
            format!("deck.jokers 固定为 2，实际 {}", ruleset.deck.jokers),
        );
    }
    for rank in &ruleset.deck.ranks {
        if rank.is_joker() {
            push(
                ErrorCode::RsSchemaInvalid,
                "deck.ranks",
                format!("ranks 不得包含王 `{}`，王只进 naturalOrder", rank.as_str()),
            );
        }
    }

    // —— flow.deal 可判定性（§5.1 第 404–405 行） ——
    let private_zones = ruleset
        .zones
        .iter()
        .filter(|z| z.scope == crate::ruleset::model::ZoneScope::Private)
        .count();
    if let Some(deal) = ruleset.flow.as_ref().and_then(|f| f.deal.as_ref()) {
        let rank_count = (ruleset
            .deck
            .ranks
            .len()
            .saturating_sub(ruleset.deck.exclude.len()))
            * ruleset.deck.suits.len()
            * ruleset.deck.copies as usize;
        let total = rank_count + ruleset.deck.jokers as usize * ruleset.deck.copies as usize;
        if deal.per_seat.is_none()
            && ruleset.seats.count > 0
            && !total.is_multiple_of(ruleset.seats.count as usize)
        {
            push(
                ErrorCode::RsSchemaInvalid,
                "flow.deal.perSeat",
                format!(
                    "总张数 {total} 不能被座位数 {} 整除，必须显式声明 perSeat（v1 不定义余量发牌）",
                    ruleset.seats.count
                ),
            );
        }
        if deal.zone.is_none() && private_zones > 1 {
            push(
                ErrorCode::RsSchemaInvalid,
                "flow.deal.zone",
                format!("存在 {private_zones} 个 private 区且未显式声明发牌落点区"),
            );
        }
    }

    // —— flow.rng.stream 0–3；flow.assert 取值（§5.1 第 412–414 行） ——
    if let Some(rng) = ruleset.flow.as_ref().and_then(|f| f.rng.as_ref()) {
        if let Some(stream) = rng.stream {
            if stream > 3 {
                push(
                    ErrorCode::RsSchemaInvalid,
                    "flow.rng.stream",
                    format!("stream 必须在 0–3 之间，实际 {stream}"),
                );
            }
        }
    }
    if let Some(assert) = ruleset.flow.as_ref().and_then(|f| f.assert.as_ref()) {
        if let Some(min_players) = assert.min_players {
            if !(1..=8).contains(&min_players) {
                push(
                    ErrorCode::RsSchemaInvalid,
                    "flow.assert.minPlayers",
                    format!("minPlayers 必须在 1–8 之间，实际 {min_players}"),
                );
            }
        }
        // cardCount: u32 ≥ 0（§5.1 第 414 行），类型域内无越界可判，跳过范围检查。
    }

    // —— lifecycle 固定四段（§5.1 第 391 行、§5.14） ——
    let expected_lifecycle = [
        crate::ruleset::model::Phase::Deal,
        crate::ruleset::model::Phase::Tribute,
        crate::ruleset::model::Phase::Play,
        crate::ruleset::model::Phase::Settle,
    ];
    if ruleset.lifecycle != expected_lifecycle {
        push(
            ErrorCode::RsSchemaInvalid,
            "lifecycle",
            "lifecycle 必须固定为 [deal, tribute, play, settle] 四段".to_owned(),
        );
    }

    // —— v2 预留 / v1 未启用 / 未知原语引用即拒绝（§5.2 第 547–549 行） ——
    for (i, pattern) in ruleset.patterns.iter().enumerate() {
        if let PrimitiveParams::Reserved { primitive_id } = &pattern.params {
            push(
                ErrorCode::RsSchemaInvalid,
                &format!("patterns[{i}].primitiveId"),
                format!("原语 `{primitive_id}` 属 v2 预留 / v1 未启用，v1 解释器不收录"),
            );
        }
    }
    for (i, action) in ruleset.actions.iter().enumerate() {
        if let PrimitiveParams::Reserved { primitive_id } = &action.params {
            push(
                ErrorCode::RsSchemaInvalid,
                &format!("actions[{i}].primitive"),
                format!("原语 `{primitive_id}` 属 v2 预留 / v1 未启用，v1 解释器不收录"),
            );
        }
        // §5.1 第 392 行：`turn` 是 play 内轮次子阶段，不是合法 legalFrom 取值。
        if let LegalFrom::Zone(zone) = &action.legal_from {
            if zone.0 == "turn" {
                push(
                    ErrorCode::RsSchemaInvalid,
                    &format!("actions[{i}].legalFrom"),
                    "`turn` 不是合法 legalFrom 取值（§5.1 第 392 行）".to_owned(),
                );
            }
        }
    }

    // —— flushRank 锚点与位次冲突（§5.1 第 389 行、§5.4 L1；惰性：无插入型牌型跳过） ——
    let sf = ladder::flush_patterns(ruleset);
    if !sf.is_empty() {
        let anchor = ladder::anchor_count(ruleset.compare.flush_rank);
        let flush_literal = match ruleset.compare.flush_rank {
            FlushRank::BelowBomb5 => "below-bomb-5",
            FlushRank::AboveBomb5 => "above-bomb-5",
            FlushRank::AboveBomb6 => "above-bomb-6",
            FlushRank::AboveBomb8 => "above-bomb-8",
        };
        match ladder::find_anchor_index(ruleset) {
            None => {
                // 锚点缺失：不降级、不插到「最近位置」（§5.1 第 389 行②）。
                let existing = ladder::ladder_bomb_counts(ruleset);
                let existing_msg = if existing.is_empty() {
                    "无".to_owned()
                } else {
                    existing
                        .iter()
                        .map(|c| c.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let sf_ids: Vec<&str> = sf.iter().map(|p| p.id.0.as_str()).collect();
                push(
                    ErrorCode::RsFlushAnchorMissing,
                    "compare.ladder",
                    format!(
                        "插入型牌型 [{}] 在 ladder 中找不到 {anchor} 张炸弹锚点（primitiveId=pattern.bomb、params.count={anchor}）；ladder 现有炸弹张数：[{existing_msg}]；不做降级插入",
                        sf_ids.join(", ")
                    ),
                );
            }
            Some(anchor_idx) => {
                // 冲突①：同一插入位被多个插入型牌型争夺（flushRank 只定义一个插入位）。
                if sf.len() > 1 {
                    let sf_ids: Vec<&str> = sf.iter().map(|p| p.id.0.as_str()).collect();
                    push(
                        ErrorCode::RsLadderConflict,
                        "compare.flushRank",
                        format!(
                            "插入位被多个插入型牌型争夺：[{}] 均挤入 flushRank={flush_literal}（锚点 {anchor} 张）的同一插入位，该取值只定义一个插入位",
                            sf_ids.join(", ")
                        ),
                    );
                }
                // 冲突②：锚点两侧炸弹张数相对序矛盾——小于锚点张数的炸弹必须在锚点下方、
                // 大于的在上方（例：bomb-6 排到 bomb-5 前面）。
                let bombs: Vec<(usize, u8, &str)> = ruleset
                    .compare
                    .ladder
                    .iter()
                    .enumerate()
                    .filter_map(|(i, pid)| {
                        ladder::find_pattern(ruleset, pid).and_then(|p| match p.params {
                            PrimitiveParams::Bomb { count } => Some((i, count, pid.0.as_str())),
                            _ => None,
                        })
                    })
                    .collect();
                for &(i, count, id) in &bombs {
                    let violates = if i < anchor_idx {
                        count > anchor
                    } else {
                        count < anchor
                    };
                    if violates {
                        push(
                            ErrorCode::RsLadderConflict,
                            "compare.ladder",
                            format!(
                                "位次矛盾：炸弹 `{id}`（{count} 张）与 {anchor} 张锚点炸弹相对序矛盾——小于锚点张数的炸弹必须排在锚点下方、大于的排在上方"
                            ),
                        );
                    }
                }
            }
        }
    }

    result
}

/// 便捷构造一条 L1 Finding（供测试与上层复用）。
pub fn l1_finding(code: ErrorCode, location: &str, message: String) -> Finding {
    Finding {
        level: Level::L1,
        code,
        severity: code.default_severity(),
        location: location.to_owned(),
        message,
    }
}
