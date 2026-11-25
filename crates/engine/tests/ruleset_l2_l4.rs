//! L2 / L3 / L4 校验器用例（§5.4 第 607–609 行）。
//!
//! 每个级别一正一负：夹具必须通过；构造一处对应缺陷必须被该级别抓出来、且不记入
//! `ran_levels`（不伪造通过）。L3 用小局数跑（仓库用例不背 200 局的耗时，默认值仍是
//! 200，见 `ValidateOptions::default`）。
use gf_engine::contract::Level;
use gf_engine::ruleset::model::RuleSetBody;
use gf_engine::ruleset::parse::from_yaml;
use gf_engine::ruleset::validate_deep::{
    builtin_golden, validate_l2, validate_l3, validate_l4, GoldenCase, ValidateOptions,
};
use gf_engine::ruleset::validate_with;

const CLASSIC: &str = include_str!("fixtures/guandan-classic/ruleset.yaml");

fn classic() -> RuleSetBody {
    from_yaml(CLASSIC).expect("classic 夹具解析失败")
}

fn small_l3() -> ValidateOptions {
    ValidateOptions {
        l3_rounds: 2,
        ..ValidateOptions::default()
    }
}

#[test]
fn l2_passes_on_classic() {
    let body = classic();
    let mut out = Default::default();
    validate_l2(&body, &mut out);
    assert!(
        out.findings.is_empty(),
        "classic 应过 L2：{:?}",
        out.findings
    );
    assert!(out.ran_levels.contains(&Level::L2));
}

#[test]
fn l2_catches_team_coverage_gap() {
    let mut body = classic();
    // 造错：把座位 3 从队伍里摘掉 → 覆盖不全。
    body.seats.teams = vec![vec![0, 2], vec![1]];
    let mut out = Default::default();
    validate_l2(&body, &mut out);
    assert!(
        out.findings
            .iter()
            .any(|f| f.code == gf_engine::contract::ErrorCode::RsTeamCoverageInvalid),
        "应报 RS_TEAM_COVERAGE_INVALID：{:?}",
        out.findings
    );
    assert!(
        !out.ran_levels.contains(&Level::L2),
        "有 finding 不得记为跑完"
    );
}

#[test]
fn l2_catches_dangling_ladder_reference() {
    let mut body = classic();
    // 造错：把一个 id 改成 patterns 索引里没有的名字 → 引用悬空。
    let dangling = body.compare.ladder[0].0.clone();
    body.compare.ladder[0] = gf_engine::contract::PatternId(format!("{dangling}-missing"));
    let mut out = Default::default();
    validate_l2(&body, &mut out);
    assert!(
        out.findings
            .iter()
            .any(|f| f.code == gf_engine::contract::ErrorCode::RsRefMissing),
        "应报 RS_REF_MISSING：{:?}",
        out.findings
    );
}

#[test]
fn l3_plays_classic_to_termination() {
    let body = classic();
    let mut out = Default::default();
    validate_l3(&body, &small_l3(), &mut out);
    assert!(
        out.findings.is_empty(),
        "classic 应能终局：{:?}",
        out.findings
    );
    assert!(out.ran_levels.contains(&Level::L3));
}

#[test]
fn l3_catches_non_termination_with_step_cap() {
    let body = classic();
    // 造错：把单局步数上限压到 3，任何牌局都跑不完 → 必须报 RS_NO_TERMINATION。
    let opts = ValidateOptions {
        l3_rounds: 1,
        l3_step_cap: 3,
        ..ValidateOptions::default()
    };
    let mut out = Default::default();
    validate_l3(&body, &opts, &mut out);
    assert!(
        out.findings
            .iter()
            .any(|f| f.code == gf_engine::contract::ErrorCode::RsNoTermination),
        "应报 RS_NO_TERMINATION：{:?}",
        out.findings
    );
    assert!(!out.ran_levels.contains(&Level::L3));
}

#[test]
fn l4_replays_builtin_golden() {
    let body = classic();
    let mut out = Default::default();
    validate_l4(&body, &builtin_golden(), &mut out);
    assert!(
        out.findings.is_empty(),
        "内置黄金应一致：{:?}",
        out.findings
    );
    assert!(out.ran_levels.contains(&Level::L4));
}

#[test]
fn l4_catches_tampered_hash() {
    let body = classic();
    // 造错：把期望哈希改一个字节。
    let mut cases: Vec<GoldenCase> = builtin_golden();
    cases[0].expected_hash[0] ^= 0xff;
    let mut out = Default::default();
    validate_l4(&body, &cases, &mut out);
    assert!(
        out.findings
            .iter()
            .any(|f| f.code == gf_engine::contract::ErrorCode::RsL4Mismatch),
        "应报 RS_L4_MISMATCH：{:?}",
        out.findings
    );
    assert!(!out.ran_levels.contains(&Level::L4));
}

#[test]
fn full_pipeline_records_all_four_levels() {
    let body = classic();
    let out = validate_with(
        &body,
        &[Level::L1, Level::L2, Level::L3, Level::L4],
        &small_l3(),
        &builtin_golden(),
    );
    assert!(
        out.findings.is_empty(),
        "classic 应四级全过：{:?}",
        out.findings
    );
    for level in [Level::L1, Level::L2, Level::L3, Level::L4] {
        assert!(out.ran_levels.contains(&level), "{level:?} 未记为跑完");
    }
}
