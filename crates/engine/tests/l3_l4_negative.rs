//! L3 跑局账目 / L4 黄金重放的负面分支（纯补测，不改实现）。
//!
//! 覆盖点：`validate_l3` 的步数上限、候选数上限、失败分类（`RS_NO_TERMINATION` /
//! `RS_ILLEGAL_ACTION_RATE`）与正常通过；`validate_l4` 的规则集绑定校验、重放失败、
//! `stateHash` 不一致与正常通过。

use gf_engine::contract::{Action, ErrorCode, Level, Validation};
use gf_engine::ruleset::parse::from_yaml;
use gf_engine::ruleset::validate_deep::{builtin_golden, validate_l3, validate_l4, ValidateOptions};

const CLASSIC: &str = include_str!("fixtures/guandan-classic/ruleset.yaml");

fn classic() -> gf_engine::ruleset::model::RuleSetBody {
    from_yaml(CLASSIC).expect("classic 夹具应能构造")
}

fn fresh() -> Validation {
    Validation::default()
}

#[test]
fn l3_passes_with_default_options() {
    let body = classic();
    let mut opts = ValidateOptions::default();
    opts.l3_rounds = 2;
    let mut v = fresh();
    let stats = validate_l3(&body, &opts, &mut v);
    assert_eq!(stats.rounds_run, 2, "两局都应跑完");
    assert!(stats.steps_total > 0, "应累计动作步数");
    assert!(v.findings.is_empty(), "默认配置不应报 findings：{:?}", v.findings);
    assert!(v.ran_levels.contains(&Level::L3));
}

#[test]
fn l3_step_cap_reports_no_termination() {
    let body = classic();
    let mut opts = ValidateOptions::default();
    opts.l3_rounds = 1;
    opts.l3_step_cap = 1;
    let mut v = fresh();
    let stats = validate_l3(&body, &opts, &mut v);
    assert_eq!(stats.rounds_run, 0, "超步数上限的局不计入跑完");
    let f = v.findings.first().expect("应报一条 L3 finding");
    assert_eq!(f.code, ErrorCode::RsNoTermination);
    assert!(f.message.contains("超过"), "消息应指出超限：{}", f.message);
    assert!(!v.ran_levels.contains(&Level::L3));
}

#[test]
fn l3_candidate_cap_reports_no_termination() {
    let body = classic();
    let mut opts = ValidateOptions::default();
    opts.l3_rounds = 1;
    opts.l3_candidate_cap = 1;
    let mut v = fresh();
    let stats = validate_l3(&body, &opts, &mut v);
    assert_eq!(stats.rounds_run, 0);
    assert!(stats.peak_candidates > 1, "峰值候选数应记录超过 1 的实测值");
    assert_eq!(v.findings[0].code, ErrorCode::RsNoTermination);
}

#[test]
fn l4_passes_with_builtin_golden() {
    let body = classic();
    let mut v = fresh();
    validate_l4(&body, &builtin_golden(), &mut v);
    assert!(v.findings.is_empty(), "内置黄金应通过：{:?}", v.findings);
    assert!(v.ran_levels.contains(&Level::L4));
}

#[test]
fn l4_rejects_foreign_ruleset_binding() {
    let body = classic();
    let mut case = builtin_golden().remove(0);
    case.ruleset_id = "another-ruleset".to_owned();
    let mut v = fresh();
    validate_l4(&body, &[case], &mut v);
    assert_eq!(v.findings[0].code, ErrorCode::RsL4Mismatch);
    assert!(
        v.findings[0].message.contains("绑定规则集"),
        "消息应指出绑定不符：{}",
        v.findings[0].message
    );
}

#[test]
fn l4_rejects_illegal_replay_and_hash_mismatch() {
    let body = classic();

    // ① 重放失败：第一条动作放一个不存在的牌 id。
    let mut bad_play = builtin_golden().remove(0);
    bad_play.actions = vec![Action::Play {
        cards: vec![9999],
    }];
    let mut v = fresh();
    validate_l4(&body, &[bad_play], &mut v);
    assert_eq!(v.findings[0].code, ErrorCode::RsL4Mismatch);
    assert!(
        v.findings[0].message.contains("重放失败"),
        "消息应指出重放失败：{}",
        v.findings[0].message
    );

    // ② hash 不一致：动作全保留，只把期望 hash 改成全 0。
    let mut bad_hash = builtin_golden().remove(0);
    bad_hash.expected_hash = [0u8; 32];
    let mut v = fresh();
    validate_l4(&body, &[bad_hash], &mut v);
    assert_eq!(v.findings[0].code, ErrorCode::RsL4Mismatch);
    assert!(
        v.findings[0].message.contains("stateHash 不一致"),
        "消息应指出 hash 不一致：{}",
        v.findings[0].message
    );

    // ③ 多例同时坏：mismatch 汇总成一条 finding。
    let mut a = builtin_golden().remove(0);
    a.ruleset_id = "x".to_owned();
    let mut b = builtin_golden().remove(0);
    b.expected_hash = [0u8; 32];
    let mut v = fresh();
    validate_l4(&body, &[a, b], &mut v);
    assert_eq!(v.findings.len(), 1, "多条不一致应汇总为一条 finding");
    assert!(v.findings[0].message.lines().count() >= 2);
}
