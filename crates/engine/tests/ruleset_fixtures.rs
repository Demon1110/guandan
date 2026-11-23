//! 夹具 L1 验收（§2.4 gate 第 4 项「非法结构不可构造」、§5.4 L1）。
//!
//! - `m0-minimal` 与 `guandan-classic`：构造成功，L1 跑完且 findings 为空；
//! - 三份非法 RuleSet：构造失败，错误码 `RS_SCHEMA_INVALID`，打印实际 location。

use gf_engine::contract::{ErrorCode, Level};
use gf_engine::ruleset::{from_yaml, validate};
use std::fs;
use std::path::PathBuf;

fn fixture_path(rel: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures");
    p.push(rel);
    p
}

fn read_fixture(rel: &str) -> String {
    fs::read_to_string(fixture_path(rel)).expect("fixture 读取失败")
}

#[test]
fn m0_minimal_passes_l1() {
    let src = read_fixture("m0-minimal/ruleset.yaml");
    let body = from_yaml(&src).expect("m0-minimal 应构造成功");
    let validation = validate(&body, &[Level::L1]);

    assert!(
        validation.ran_levels.contains(&Level::L1),
        "ran_levels 应记录实际跑完的 L1: {validation:?}"
    );
    assert!(
        validation.findings.is_empty(),
        "m0-minimal L1 不应有 finding: {:?}",
        validation.findings
    );
}

#[test]
fn guandan_classic_passes_l1() {
    let src = read_fixture("guandan-classic/ruleset.yaml");
    let body = from_yaml(&src).expect("guandan-classic 应构造成功");
    let validation = validate(&body, &[Level::L1]);

    assert!(validation.ran_levels.contains(&Level::L1));
    assert!(
        validation.findings.is_empty(),
        "guandan-classic L1 不应有 finding: {:?}",
        validation.findings
    );
}

#[test]
fn requested_unimplemented_level_is_not_recorded() {
    // L2–L4 本轮未实现：请求了未实现的级别不得记入 ran_levels（§5.4 第 578 行语义）。
    let src = read_fixture("m0-minimal/ruleset.yaml");
    let body = from_yaml(&src).unwrap();
    let validation = validate(&body, &[Level::L2, Level::L3, Level::L4]);
    assert!(validation.ran_levels.is_empty());
    assert!(validation.findings.is_empty());
}

#[test]
fn invalid_rulesets_rejected_at_construction() {
    let cases = [
        "invalid/unknown-key",
        "invalid/missing-required-key",
        "invalid/type-mismatch",
    ];
    for rel in cases {
        let src = read_fixture(&format!("{rel}/ruleset.yaml"));
        let err = from_yaml(&src).expect_err(&format!("{rel} 必须构造失败"));
        println!("[{rel}] code={} location={} message={}", err.code.code_str(), err.location, err.message);
        assert_eq!(
            err.code,
            ErrorCode::RsSchemaInvalid,
            "{rel} 错误码应为 RS_SCHEMA_INVALID"
        );
        assert!(!err.location.is_empty(), "{rel} location 不应为空");
    }
}
