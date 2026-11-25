//! L1 校验器的「另一侧」补充：字段级构造（不走 YAML 字符串替换），把合法夹具里
//! 出现不到的分支走一遍——level.pass 显式 combos、score-diff basis 整套键、
//! deck.ranks 夹王、flow 缺段、多 private 区无发牌落点。

use gf_engine::contract::{ErrorCode, Level, Validation, ZoneId};
use gf_engine::ruleset::model::{
    LevelConfig, LevelCurrentConfig, LevelScope, PassConfig, UpgradeBasis, UpgradeDiffRow, ZoneDef,
    ZoneScope,
};
use gf_engine::ruleset::{from_yaml, validate};
use std::fs;
use std::path::PathBuf;

fn classic_src() -> String {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/guandan-classic/ruleset.yaml");
    fs::read_to_string(p).expect("夹具读取失败")
}

fn classic() -> gf_engine::ruleset::model::RuleSetBody {
    from_yaml(&classic_src()).expect("classic 应可解析")
}

fn has(result: &Validation, code: ErrorCode, loc_prefix: &str) -> bool {
    result
        .findings
        .iter()
        .any(|f| f.code == code && f.location.starts_with(loc_prefix))
}

fn codes(result: &Validation) -> Vec<String> {
    result
        .findings
        .iter()
        .map(|f| format!("{:?}@{}", f.code, f.location))
        .collect()
}

#[test]
fn explicit_duplicate_pass_combos_are_rejected() {
    let mut body = classic();
    body.level = Some(LevelConfig {
        pass: PassConfig {
            combos: Some(vec![vec![1, 2], vec![1, 2]]),
            streak_limit: 3,
            reset_on_fail: true,
        },
        current: LevelCurrentConfig {
            scope: LevelScope::ByTeam,
        },
    });
    let result = validate(&body, &[Level::L1]);
    assert!(
        has(&result, ErrorCode::RsSchemaInvalid, "level.pass.combos"),
        "重复名次组合应报 level.pass.combos：{:?}",
        codes(&result)
    );
}

#[test]
fn explicit_pass_combos_and_streak_limit_bounds() {
    // 合法显式 combos（teamSize=2 的两组）→ 走 `Some` 侧且不报错。
    let mut body = classic();
    body.level = Some(LevelConfig {
        pass: PassConfig {
            combos: Some(vec![vec![1, 2], vec![1, 3]]),
            streak_limit: 3,
            reset_on_fail: true,
        },
        current: LevelCurrentConfig {
            scope: LevelScope::ByTeam,
        },
    });
    let ok = validate(&body, &[Level::L1]);
    assert!(
        !has(&ok, ErrorCode::RsSchemaInvalid, "level.pass"),
        "合法 combos 不应报错：{:?}",
        codes(&ok)
    );

    // streakLimit 越界（1–3 之外）。
    let level = body.level.as_mut().expect("上一步已建 level 段");
    level.pass.streak_limit = 4;
    let bad = validate(&body, &[Level::L1]);
    assert!(
        has(&bad, ErrorCode::RsSchemaInvalid, "level.pass.streakLimit"),
        "streakLimit 4 应报错：{:?}",
        codes(&bad)
    );
}

#[test]
fn rank_combo_basis_rejects_foreign_basis_key_sets() {
    let mut body = classic();
    let table = body.level_table.as_mut().expect("classic 有 levelTable");
    assert_eq!(table.upgrade_basis, UpgradeBasis::RankCombo);
    table.rank_scores = Some(vec![0, 1, 2, 3]);
    let result = validate(&body, &[Level::L1]);
    assert!(
        has(&result, ErrorCode::RsSchemaInvalid, "levelTable.rankScores"),
        "rank-combo basis 出现 rankScores 应报错：{:?}",
        codes(&result)
    );
}

#[test]
fn score_diff_basis_validates_its_own_key_set() {
    let mut body = classic();
    let table = body.level_table.as_mut().expect("classic 有 levelTable");
    table.upgrade_basis = UpgradeBasis::ScoreDiff;
    table.upgrade = None;
    table.rank_scores = Some(vec![3, 1, 2, 0]);
    table.upgrade_by_diff = Some(vec![
        UpgradeDiffRow {
            min_diff: 30,
            step: 1,
        },
        UpgradeDiffRow {
            min_diff: 0,
            step: 0,
        },
    ]);
    // 有序、含 0 兜底、step 在 0–3 → 应通过。
    let ok = validate(&body, &[Level::L1]);
    assert!(
        !has(&ok, ErrorCode::RsSchemaInvalid, "levelTable"),
        "合法 score-diff 表不应报错：{:?}",
        codes(&ok)
    );

    // step 越界。
    let table = body.level_table.as_mut().expect("classic 有 levelTable");
    table.upgrade_by_diff = Some(vec![
        UpgradeDiffRow {
            min_diff: 30,
            step: 9,
        },
        UpgradeDiffRow {
            min_diff: 0,
            step: 0,
        },
    ]);
    let bad = validate(&body, &[Level::L1]);
    assert!(
        has(&bad, ErrorCode::RsSchemaInvalid, "levelTable.upgradeByDiff"),
        "step 9 应报错：{:?}",
        codes(&bad)
    );

    // minDiff 未按降序。
    let table = body.level_table.as_mut().expect("classic 有 levelTable");
    table.upgrade_by_diff = Some(vec![
        UpgradeDiffRow {
            min_diff: 0,
            step: 1,
        },
        UpgradeDiffRow {
            min_diff: 30,
            step: 1,
        },
    ]);
    let bad = validate(&body, &[Level::L1]);
    assert!(
        has(&bad, ErrorCode::RsSchemaInvalid, "levelTable.upgradeByDiff"),
        "乱序 minDiff 应报错：{:?}",
        codes(&bad)
    );
}

#[test]
fn deck_ranks_must_not_contain_jokers() {
    let mut body = classic();
    let last_rank = body
        .natural_order
        .last()
        .cloned()
        .expect("naturalOrder 非空");
    body.deck.ranks.push(last_rank);
    // 取的是 naturalOrder 末位（大王）：只要夹了王就会被拦。
    let result = validate(&body, &[Level::L1]);
    assert!(
        has(&result, ErrorCode::RsSchemaInvalid, "deck.ranks"),
        "ranks 夹王应报错：{:?}",
        codes(&result)
    );
}

#[test]
fn two_private_zones_without_deal_target_is_rejected() {
    let mut body = classic();
    if let Some(flow) = body.flow.as_mut() {
        if let Some(deal) = flow.deal.as_mut() {
            deal.zone = None;
        }
    }
    body.zones.push(ZoneDef {
        id: ZoneId("stash".to_owned()),
        scope: ZoneScope::Private,
        capacity: -1,
    });
    let result = validate(&body, &[Level::L1]);
    assert!(
        has(&result, ErrorCode::RsSchemaInvalid, "flow.deal.zone"),
        "两个 private 区且无发牌落点应报错：{:?}",
        codes(&result)
    );
}

#[test]
fn absent_flow_section_still_runs_l1() {
    let mut body = classic();
    body.flow = None;
    let result = validate(&body, &[Level::L1]);
    assert!(
        result.ran_levels.contains(&Level::L1),
        "没有 flow 段也应把 L1 标记为已跑"
    );
    assert!(
        !has(&result, ErrorCode::RsSchemaInvalid, "flow.deal"),
        "缺 flow 段不应报 deal 相关错：{:?}",
        codes(&result)
    );
}
