//! L1 结构校验（§5.4 第 569 行、§5.1 / §5.2 取值范围）。
//!
//! 本轮只实现 L1；L2–L4 本轮不实现：请求了未实现的级别不记入 `ran_levels`
//! （`ran_levels` 语义是「实际跑完的级别」，§5.4 第 578 行），不伪造通过。

use crate::contract::{ErrorCode, Finding, Level, Validation};
use crate::ruleset::ladder;
use crate::ruleset::model::{FlushRank, LegalFrom, RuleSetBody, ScoreDimension, UpgradeBasis};
use crate::ruleset::params::PrimitiveParams;
use std::collections::HashSet;

/// 校验一个名次组合（`teamRanks` / `pass.combos` 共用同一套名次编码，§5.1）：
/// 长度 = `teamSize`、元素 `1..=seats.count`、互不相同且升序。返回逐条错误消息。
fn rank_combo_errors(combo: &[u8], team_size: Option<usize>, seats_count: u8) -> Vec<String> {
    let mut errs = Vec::new();
    if let Some(ts) = team_size {
        if combo.len() != ts {
            errs.push(format!(
                "名次组合 {:?} 长度 {} ≠ teamSize {ts}",
                combo,
                combo.len()
            ));
        }
    }
    if combo.iter().any(|r| !(1..=seats_count).contains(r)) {
        errs.push(format!(
            "名次组合 {:?} 元素必须在 1..={seats_count} 之间",
            combo
        ));
    }
    if combo.windows(2).any(|w| w[0] == w[1]) {
        errs.push(format!("名次组合 {:?} 元素不得重复", combo));
    }
    if combo.windows(2).any(|w| w[0] > w[1]) {
        errs.push(format!("名次组合 {:?} 必须按升序规范书写", combo));
    }
    errs
}

/// 原语侧参数值域闸门（§5.2 参数表第 510/525/532/533/541/543 行）。
///
/// `loc_prefix` 形如 `actions[0].params` / `patterns[0].params`，字段路径拼在其后。
/// 牌型六类值域由 patterns 循环内的专用闸门覆盖；本函数兜底进贡 / 行动 / 轮转 / 发牌 /
/// 随机 / 断言 / 级牌原语的数值区间。
fn gate_primitive_params(
    params: &PrimitiveParams,
    loc_prefix: &str,
    team_size: Option<usize>,
    seats_count: u8,
    push: &mut impl FnMut(ErrorCode, &str, String),
) {
    let mut u8_range = |field: &str, value: u8, lo: u8, hi: u8, what: &str| {
        if !(lo..=hi).contains(&value) {
            push(
                ErrorCode::RsSchemaInvalid,
                &format!("{loc_prefix}.{field}"),
                format!("{what} 必须在 {lo}–{hi} 之间，实际 {value}"),
            );
        }
    };
    match params {
        PrimitiveParams::PayParams { count, .. } => {
            u8_range("count", *count, 1, 2, "tribute.pay count")
        }
        PrimitiveParams::ReturnParams { count, .. } => {
            u8_range("count", *count, 1, 2, "tribute.return count")
        }
        PrimitiveParams::Shuffle {
            per_seat: Some(ps), ..
        } => {
            if !(1..=27).contains(ps) {
                push(
                    ErrorCode::RsSchemaInvalid,
                    &format!("{loc_prefix}.perSeat"),
                    format!("deal.shuffle perSeat 必须在 1–27 之间，实际 {ps}"),
                );
            }
        }
        PrimitiveParams::Shuffle { .. } => {}
        PrimitiveParams::ConsumeCards { count, .. } => {
            u8_range("count", *count, 0, 8, "consume-cards count")
        }
        PrimitiveParams::RngPick { stream } => {
            if *stream > 3 {
                push(
                    ErrorCode::RsSchemaInvalid,
                    &format!("{loc_prefix}.stream"),
                    format!("rng.pick stream 必须在 0–3 之间，实际 {stream}"),
                );
            }
        }
        PrimitiveParams::MinPlayers { count } => {
            u8_range("count", *count, 1, 8, "min-players count")
        }
        PrimitiveParams::LevelPass {
            combos,
            streak_limit,
            ..
        } => {
            if let Some(combos) = combos {
                for combo in combos {
                    for err in rank_combo_errors(combo, team_size, seats_count) {
                        push(
                            ErrorCode::RsSchemaInvalid,
                            &format!("{loc_prefix}.combos"),
                            err,
                        );
                    }
                }
            }
            if !(1..=3).contains(streak_limit) {
                push(
                    ErrorCode::RsSchemaInvalid,
                    &format!("{loc_prefix}.streakLimit"),
                    format!("streakLimit 必须在 1–3 之间，实际 {streak_limit}"),
                );
            }
        }
        _ => {}
    }
}

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

    // —— seats.teams：各队规模一致才可确定 teamSize（§5.1 第 380 行、§5.4 L1） ——
    // 队伍覆盖不全 / 重叠属 L2（RS_TEAM_COVERAGE_INVALID），本轮不实现。
    let team_size: Option<usize> = {
        let teams = &ruleset.seats.teams;
        if teams.is_empty() {
            push(
                ErrorCode::RsSchemaInvalid,
                "seats.teams",
                "teams 不能为空队伍集合；队伍覆盖/重叠校验属 L2（RS_TEAM_COVERAGE_INVALID）"
                    .to_owned(),
            );
            None
        } else {
            let first = teams[0].len();
            let inconsistent = teams.iter().any(|t| t.len() != first);
            if inconsistent {
                let sizes: Vec<usize> = teams.iter().map(|t| t.len()).collect();
                push(
                    ErrorCode::RsSchemaInvalid,
                    "seats.teams",
                    format!(
                        "各队规模不一致（{sizes:?}），无法确定 teamSize；队伍覆盖本身属 L2（RS_TEAM_COVERAGE_INVALID）"
                    ),
                );
                None
            } else {
                Some(first)
            }
        }
    };

    // flow.score.dimension 缺省 team（§5.1 第 411 行）；未声明 flow 视为 team，视为满足。
    let score_dimension_team = ruleset
        .flow
        .as_ref()
        .and_then(|f| f.score.as_ref())
        .and_then(|s| s.dimension)
        != Some(ScoreDimension::Player);

    // —— levelTable：basis 互斥键集、rank-combo 名次组合 / step、score-diff 分值表（§5.1 第 383 行、§5.4 L1） ——
    if let Some(table) = &ruleset.level_table {
        match table.upgrade_basis {
            UpgradeBasis::RankCombo => {
                if table.upgrade.is_none() {
                    push(
                        ErrorCode::RsSchemaInvalid,
                        "levelTable.upgrade",
                        "rank-combo basis 要求 upgrade[] 齐备".to_owned(),
                    );
                }
                if table.rank_scores.is_some() || table.upgrade_by_diff.is_some() {
                    push(
                        ErrorCode::RsSchemaInvalid,
                        "levelTable.rankScores",
                        "rank-combo basis 不得出现 rankScores / upgradeByDiff（两个 basis 键集互斥）".to_owned(),
                    );
                }
                if let Some(rows) = &table.upgrade {
                    let mut seen: HashSet<Vec<u8>> = HashSet::new();
                    for (i, row) in rows.iter().enumerate() {
                        let loc = format!("levelTable.upgrade[{i}].teamRanks");
                        for err in
                            rank_combo_errors(&row.team_ranks, team_size, ruleset.seats.count)
                        {
                            push(ErrorCode::RsSchemaInvalid, &loc, err);
                        }
                        if !seen.insert(row.team_ranks.clone()) {
                            push(
                                ErrorCode::RsSchemaInvalid,
                                &loc,
                                format!("teamRanks {:?} 与另一条重复", row.team_ranks),
                            );
                        }
                        if !(0..=3).contains(&row.step) {
                            push(
                                ErrorCode::RsSchemaInvalid,
                                &format!("levelTable.upgrade[{i}].step"),
                                format!(
                                    "step 必须在 0–3 之间（0 = 该组合不升档），实际 {}",
                                    row.step
                                ),
                            );
                        }
                    }
                }
            }
            UpgradeBasis::ScoreDiff => {
                if table.upgrade.is_some() {
                    push(
                        ErrorCode::RsSchemaInvalid,
                        "levelTable.upgrade",
                        "score-diff basis 要求 upgrade[] 缺席".to_owned(),
                    );
                }
                match (&table.rank_scores, &table.upgrade_by_diff) {
                    (Some(scores), Some(diff_rows)) => {
                        if scores.len() != ruleset.seats.count as usize {
                            push(
                                ErrorCode::RsSchemaInvalid,
                                "levelTable.rankScores",
                                format!(
                                    "rankScores 长度必须等于 seats.count（{}），实际 {}",
                                    ruleset.seats.count,
                                    scores.len()
                                ),
                            );
                        }
                        let mut prev: Option<u16> = None;
                        let mut has_zero_fallback = false;
                        for (i, row) in diff_rows.iter().enumerate() {
                            if row.min_diff == 0 {
                                has_zero_fallback = true;
                            }
                            if let Some(p) = prev {
                                if row.min_diff > p {
                                    push(
                                        ErrorCode::RsSchemaInvalid,
                                        &format!("levelTable.upgradeByDiff[{i}].minDiff"),
                                        format!(
                                            "minDiff {} 未按降序排列（前一条 {p}）",
                                            row.min_diff
                                        ),
                                    );
                                }
                            }
                            prev = Some(row.min_diff);
                            if !(0..=3).contains(&row.step) {
                                push(
                                    ErrorCode::RsSchemaInvalid,
                                    &format!("levelTable.upgradeByDiff[{i}].step"),
                                    format!("step 必须在 0–3 之间，实际 {}", row.step),
                                );
                            }
                        }
                        if !has_zero_fallback {
                            push(
                                ErrorCode::RsSchemaInvalid,
                                "levelTable.upgradeByDiff",
                                "upgradeByDiff 必须含一条 minDiff: 0 兜底".to_owned(),
                            );
                        }
                        if !score_dimension_team {
                            push(
                                ErrorCode::RsSchemaInvalid,
                                "flow.score.dimension",
                                "score-diff basis 要求 flow.score.dimension = team".to_owned(),
                            );
                        }
                    }
                    _ => {
                        push(
                            ErrorCode::RsSchemaInvalid,
                            "levelTable.upgradeByDiff",
                            "score-diff basis 要求 rankScores[] 与 upgradeByDiff[] 齐备".to_owned(),
                        );
                    }
                }
            }
        }
    }

    // —— level.pass.combos / streakLimit（§5.1 第 384 行、§5.2 第 537 行） ——
    // streakLimit 值域 1–3（§5.4 L1 已对齐；原 1–9 为笔误）。
    if let Some(level) = &ruleset.level {
        let combos: Vec<Vec<u8>> = match &level.pass.combos {
            Some(c) => c.clone(),
            None => match team_size {
                Some(2) => vec![vec![1, 2], vec![1, 3]],
                _ => {
                    push(
                        ErrorCode::RsSchemaInvalid,
                        "level.pass.combos",
                        "缺省 combos 只在 teamSize = 2 时有定义；teamSize ≠ 2 时必须显式声明"
                            .to_owned(),
                    );
                    Vec::new()
                }
            },
        };
        if combos.is_empty() {
            push(
                ErrorCode::RsSchemaInvalid,
                "level.pass.combos",
                "pass.combos 不得为空表".to_owned(),
            );
        }
        let mut seen: HashSet<Vec<u8>> = HashSet::new();
        for combo in &combos {
            for err in rank_combo_errors(combo, team_size, ruleset.seats.count) {
                push(ErrorCode::RsSchemaInvalid, "level.pass.combos", err);
            }
            if !seen.insert(combo.clone()) {
                push(
                    ErrorCode::RsSchemaInvalid,
                    "level.pass.combos",
                    format!("名次组合 {:?} 与另一条重复", combo),
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

    // —— tribute 必填性与 pairs 值域（§5.1 第 390 行、§5.2 第 531–534 行） ——
    match &ruleset.tribute {
        None => push(
            ErrorCode::RsSchemaInvalid,
            "tribute",
            "tribute 段必填（§5.1 `tribute` 行：掼蛋必填，缺省即拒）".to_owned(),
        ),
        Some(t) => {
            if t.enabled {
                if t.trigger.is_none() {
                    push(
                        ErrorCode::RsSchemaInvalid,
                        "tribute.trigger",
                        "enabled=true 时 trigger 必填".to_owned(),
                    );
                }
                match &t.pairs {
                    None => push(
                        ErrorCode::RsSchemaInvalid,
                        "tribute.pairs",
                        "enabled=true 时 pairs 必填".to_owned(),
                    ),
                    Some(pairs) if pairs.is_empty() => push(
                        ErrorCode::RsSchemaInvalid,
                        "tribute.pairs",
                        "pairs 不得为空表".to_owned(),
                    ),
                    Some(_) => {}
                }
                if t.return_.is_none() {
                    push(
                        ErrorCode::RsSchemaInvalid,
                        "tribute.return",
                        "enabled=true 时 return 必填（ceiling/count/fallback 三键齐全）".to_owned(),
                    );
                }
                if t.resist.is_none() {
                    push(
                        ErrorCode::RsSchemaInvalid,
                        "tribute.resist",
                        "enabled=true 时 resist 必填".to_owned(),
                    );
                }
            }
            // pairs 值域：payer/receiver 为名次数值 1..=seats.count（§5.1 第 390 行），显式给出即校验。
            if let Some(pairs) = &t.pairs {
                for (j, pair) in pairs.iter().enumerate() {
                    if !(1..=ruleset.seats.count).contains(&pair.payer) {
                        push(
                            ErrorCode::RsSchemaInvalid,
                            &format!("tribute.pairs[{j}].payer"),
                            format!(
                                "payer 必须在 1..={} 之间，实际 {}",
                                ruleset.seats.count, pair.payer
                            ),
                        );
                    }
                    if !(1..=ruleset.seats.count).contains(&pair.receiver) {
                        push(
                            ErrorCode::RsSchemaInvalid,
                            &format!("tribute.pairs[{j}].receiver"),
                            format!(
                                "receiver 必须在 1..={} 之间，实际 {}",
                                ruleset.seats.count, pair.receiver
                            ),
                        );
                    }
                }
            }
        }
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

    // —— patterns[].params 取值范围（§5.2 参数表第 513–518 行） ——
    for (i, pattern) in ruleset.patterns.iter().enumerate() {
        let mut range = |field: &str, value: u8, lo: u8, hi: u8, what: &str| {
            if !(lo..=hi).contains(&value) {
                push(
                    ErrorCode::RsSchemaInvalid,
                    &format!("patterns[{i}].params.{field}"),
                    format!("{what} 必须在 {lo}–{hi} 之间，实际 {value}"),
                );
            }
        };
        match &pattern.params {
            PrimitiveParams::SameRank { count } => range("count", *count, 1, 8, "same-rank count"),
            PrimitiveParams::Straight { length, .. } => {
                range("length", *length, 5, 12, "straight length")
            }
            PrimitiveParams::ConsecutivePairs { pairs } => {
                range("pairs", *pairs, 2, 10, "consecutive-pairs pairs")
            }
            PrimitiveParams::ConsecutiveTriples { triples } => {
                range("triples", *triples, 2, 6, "consecutive-triples triples")
            }
            PrimitiveParams::Bomb { count } => range("count", *count, 4, 8, "bomb count"),
            PrimitiveParams::StraightFlush { length } => {
                range("length", *length, 5, 12, "straight-flush length")
            }
            _ => {}
        }
        gate_primitive_params(
            &pattern.params,
            &format!("patterns[{i}].params"),
            team_size,
            ruleset.seats.count,
            &mut push,
        );
    }
    for (i, action) in ruleset.actions.iter().enumerate() {
        if let PrimitiveParams::Reserved { primitive_id } = &action.params {
            push(
                ErrorCode::RsSchemaInvalid,
                &format!("actions[{i}].primitive"),
                format!("原语 `{primitive_id}` 属 v2 预留 / v1 未启用，v1 解释器不收录"),
            );
        }
        gate_primitive_params(
            &action.params,
            &format!("actions[{i}].params"),
            team_size,
            ruleset.seats.count,
            &mut push,
        );
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
