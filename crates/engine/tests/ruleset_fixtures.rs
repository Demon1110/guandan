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
        println!(
            "[{rel}] code={} location={} message={}",
            err.code.code_str(),
            err.location,
            err.message
        );
        assert_eq!(
            err.code,
            ErrorCode::RsSchemaInvalid,
            "{rel} 错误码应为 RS_SCHEMA_INVALID"
        );
        assert!(!err.location.is_empty(), "{rel} location 不应为空");
    }
}

// ---------- P1/P2 补充口径测试 ----------

/// 组装一份最小合法 RuleSet YAML 模板（`deck.exclude` 缺省空数组，§5.1 第 381 行）。
fn assemble(patterns: &str, actions: &str, settle: &str, level: Option<&str>) -> String {
    let level_block = level.unwrap_or_default();
    format!(
        r#"rulesetId: test
interpreter: ">=1.2,<2"
seats: {{ count: 4, teams: [[0,2],[1,3]] }}
deck: {{ copies: 2, ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"], suits: [S,H,D,C], jokers: 2 }}
zones:
  - {{ id: hand, scope: private, capacity: 27 }}
  - {{ id: discard, scope: shared, capacity: -1 }}
naturalOrder: ["2","3","4","5","6","7","8","9","10","J","Q","K","A",little-joker,big-joker]
levelInsert: above-A
{level_block}
patterns:
{patterns}
compare:
  ladder: [single]
  bombOrder: count-then-rank
  flushRank: above-bomb-5
actions:
{actions}
lifecycle: [deal, tribute, play, settle]
flow:
  deal: {{ zone: hand, perSeat: 27, reveal: false }}
  turn: {{ direction: clockwise, skipFinished: true, partnerLead: true, afterAllPass: last-player }}
  score: {{ dimension: team }}
  rng: {{ stream: 0 }}
  assert: {{ minPlayers: 4, cardCount: 108 }}
settle: {{ {settle} }}
view: {{ schemaVersion: 1, fields: [hand, lastPlay] }}
"#,
        patterns = patterns,
        actions = actions,
        settle = settle,
        level_block = level_block,
    )
}

const SINGLE_PATTERN: &str =
    "  - { id: single, primitiveId: pattern.same-rank, params: { count: 1 } }";
const PLAY_ACTION: &str =
    "  - { kind: play, primitive: action.require-legal, legalFrom: patterns, params: { force: true, consume: hand } }";

#[test]
fn camel_case_params_keys_accepted() {
    // §5.2 参数表 camelCase 键名：wildcardAllowed / skipFinished / streakLimit / resetOnFail。
    let level = "level:\n  pass: { combos: [[1,2],[1,3]], streakLimit: 2, resetOnFail: false }\n  current: { scope: by-team }";
    let patterns =
        "  - { id: straight-5, primitiveId: pattern.straight, params: { length: 5, wildcardAllowed: false } }";
    let actions =
        "  - { kind: pass, primitive: turn.next, legalFrom: patterns, params: { direction: clockwise, skipFinished: false } }";
    let src = assemble(patterns, actions, "algorithm: score-only", Some(level));
    let body = from_yaml(&src).expect("camelCase 键名应构造成功");
    let validation = validate(&body, &[Level::L1]);
    assert!(
        validation.findings.is_empty(),
        "camelCase 参数不应有 finding: {:?}",
        validation.findings
    );
}

#[test]
fn snake_case_params_keys_rejected() {
    // 同一键写成 snake_case 必须被拒（只认 §5.2 表的一套写法）。
    let patterns =
        "  - { id: straight-5, primitiveId: pattern.straight, params: { length: 5, wildcard_allowed: false } }";
    let src = assemble(patterns, PLAY_ACTION, "algorithm: score-only", None);
    let err = from_yaml(&src).expect_err("snake_case 键名应构造失败");
    assert_eq!(err.code, ErrorCode::RsSchemaInvalid);
}

#[test]
fn deck_exclude_omitted_constructs_and_passes_l1() {
    // §5.1 第 381 行：exclude 缺省空数组；缺该键不得构造失败。
    let src = assemble(SINGLE_PATTERN, PLAY_ACTION, "algorithm: score-only", None);
    let body = from_yaml(&src).expect("省略 exclude 应构造成功");
    let validation = validate(&body, &[Level::L1]);
    assert!(validation.findings.is_empty());
}

#[test]
fn settle_score_only_rejects_extra_keys() {
    // 内部标签枚举会静默吃掉多余键；显式反序列化必须拒绝未知键（§5.3 红线）。
    let src = assemble(
        SINGLE_PATTERN,
        PLAY_ACTION,
        "algorithm: score-only, bogus: 1",
        None,
    );
    let err = from_yaml(&src).expect_err("score-only 多余键应构造失败");
    assert_eq!(err.code, ErrorCode::RsSchemaInvalid);
}

#[test]
fn no_param_primitive_missing_params_rejected() {
    // §5.1 第 392 行：无参原语也必须写空对象 `{}`。
    let patterns = "  - { id: joker-bomb, primitiveId: pattern.joker-bomb }";
    let src = assemble(patterns, PLAY_ACTION, "algorithm: score-only", None);
    let err = from_yaml(&src).expect_err("joker-bomb 缺 params 应构造失败");
    assert_eq!(err.code, ErrorCode::RsSchemaInvalid);

    let actions = "  - { kind: pass, primitive: action.pass, legalFrom: play }";
    let src = assemble(SINGLE_PATTERN, actions, "algorithm: score-only", None);
    let err = from_yaml(&src).expect_err("action.pass 缺 params 应构造失败");
    assert_eq!(err.code, ErrorCode::RsSchemaInvalid);
}

#[test]
fn legal_from_turn_rejected_at_l1() {
    // §5.1 第 392 行：`turn` 不是合法 legalFrom 取值。
    let actions =
        "  - { kind: play, primitive: action.require-legal, legalFrom: turn, params: { force: true, consume: hand } }";
    let src = assemble(SINGLE_PATTERN, actions, "algorithm: score-only", None);
    let body = from_yaml(&src).expect("legalFrom: turn 应能构造，交由 L1 拒绝");
    let validation = validate(&body, &[Level::L1]);
    let turn_finding = validation
        .findings
        .iter()
        .find(|f| f.location == "actions[0].legalFrom");
    assert!(
        turn_finding.is_some(),
        "应在 actions[0].legalFrom 报 RS_SCHEMA_INVALID: {:?}",
        validation.findings
    );
    assert_eq!(turn_finding.unwrap().code, ErrorCode::RsSchemaInvalid);
}

#[test]
fn rank_key_numeric_literal_rejected() {
    // §5.1 第 486 行：RankKey 只接受字符串字面值，未加引号的数字必须拒绝。
    let src = r#"rulesetId: test
interpreter: ">=1.2,<2"
seats: { count: 4, teams: [[0,2],[1,3]] }
deck: { copies: 2, ranks: [2], suits: [S,H,D,C], jokers: 2 }
zones:
  - { id: hand, scope: private, capacity: 27 }
naturalOrder: ["2", little-joker, big-joker]
levelInsert: above-A
patterns:
  - { id: single, primitiveId: pattern.same-rank, params: { count: 1 } }
compare:
  ladder: [single]
  bombOrder: count-then-rank
  flushRank: above-bomb-5
actions:
  - { kind: play, primitive: action.require-legal, legalFrom: patterns, params: { force: true, consume: hand } }
lifecycle: [deal, tribute, play, settle]
flow:
  deal: { zone: hand, perSeat: 27, reveal: false }
  turn: { direction: clockwise, skipFinished: true, partnerLead: true, afterAllPass: last-player }
  score: { dimension: team }
  rng: { stream: 0 }
  assert: { minPlayers: 4, cardCount: 108 }
settle: { algorithm: score-only }
view: { schemaVersion: 1, fields: [hand, lastPlay] }
"#;
    let err = from_yaml(src).expect_err("数字 RankKey 应构造失败");
    assert_eq!(err.code, ErrorCode::RsSchemaInvalid);
}
