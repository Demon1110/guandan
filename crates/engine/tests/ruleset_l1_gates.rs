//! M0 契约对齐：新增 L1 值域 / 必填性闸门的反证与正例（§5.1 / §5.2 / §5.4）。
//!
//! 每条新增闸门都配一条「故意造错」用例证明闸门真的会拦；正例证明合法形状可通过。
//! 夹具以内联 YAML 字符串组装，断言具体 `location` 与 `code`，不只看 findings 非空。

use gf_engine::contract::{ErrorCode, Finding, Level};
use gf_engine::ruleset::{from_yaml, validate};

const DEFAULT_PATTERNS: &str =
    "  - { id: single, primitiveId: pattern.same-rank, params: { count: 1 } }";
const DEFAULT_ACTIONS: &str = "  - { kind: play, primitive: action.require-legal, legalFrom: patterns, params: { force: true, consume: hand } }\n  - { kind: pass, primitive: action.pass, legalFrom: play, params: {} }";

struct Opts<'a> {
    seats: &'a str,
    per_seat: u16,
    min_players: u8,
    level_table: &'a str,
    level_block: &'a str,
    patterns: &'a str,
    actions: &'a str,
    /// `tribute` 段内联体；空串表示整段省略。
    tribute: &'a str,
    score_dim: &'a str,
}

impl<'a> Opts<'a> {
    fn four_seat() -> Self {
        Self {
            seats: "{ count: 4, teams: [[0,2],[1,3]] }",
            per_seat: 27,
            min_players: 4,
            level_table: "",
            level_block: "",
            patterns: DEFAULT_PATTERNS,
            actions: DEFAULT_ACTIONS,
            tribute: "enabled: false",
            score_dim: "team",
        }
    }
}

fn base(o: &Opts) -> String {
    let tribute_line = if o.tribute.is_empty() {
        String::new()
    } else {
        let body = o
            .tribute
            .lines()
            .map(|l| format!("  {}", l.trim_start()))
            .collect::<Vec<_>>()
            .join("\n");
        format!("tribute:\n{body}")
    };
    format!(
        r#"rulesetId: test
interpreter: ">=1.2,<2"
seats: {seats}
deck: {{ copies: 2, ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"], suits: [S,H,D,C], jokers: 2 }}
zones:
  - {{ id: hand, scope: private, capacity: 27 }}
  - {{ id: discard, scope: shared, capacity: -1 }}
naturalOrder: ["2","3","4","5","6","7","8","9","10","J","Q","K","A",little-joker,big-joker]
levelInsert: above-A
{level_table}
{level_block}
patterns:
{patterns}
compare:
  ladder: [single]
  bombOrder: count-then-rank
  flushRank: above-bomb-5
{tribute_line}
actions:
{actions}
lifecycle: [deal, tribute, play, settle]
flow:
  deal: {{ zone: hand, perSeat: {per_seat}, reveal: false }}
  turn: {{ direction: clockwise, skipFinished: true, partnerLead: true, afterAllPass: last-player }}
  score: {{ dimension: {score_dim} }}
  rng: {{ stream: 0 }}
  assert: {{ minPlayers: {min_players}, cardCount: 108 }}
settle: {{ algorithm: score-only }}
view: {{ schemaVersion: 1, fields: [hand, lastPlay] }}
"#,
        seats = o.seats,
        per_seat = o.per_seat,
        min_players = o.min_players,
        level_table = o.level_table,
        level_block = o.level_block,
        patterns = o.patterns,
        actions = o.actions,
        tribute_line = tribute_line,
        score_dim = o.score_dim,
    )
}

fn l1_findings(src: &str) -> Vec<Finding> {
    let body = from_yaml(src).expect("基础夹具应构造成功，交由 L1 闸门判定");
    validate(&body, &[Level::L1]).findings
}

fn assert_gate(src: &str, code: ErrorCode, location: &str) {
    let findings = l1_findings(src);
    let hit = findings
        .iter()
        .find(|f| f.code == code && f.location.contains(location));
    assert!(
        hit.is_some(),
        "应在 `{location}` 报 {}，实际 findings: {:?}",
        code.code_str(),
        findings
    );
}

fn assert_clean(src: &str) {
    let findings = l1_findings(src);
    assert!(
        findings.is_empty(),
        "合法 RuleSet 不应有 finding: {:?}",
        findings
    );
}

// ---------- rank-combo：teamRanks 值域 / 序 / 长度 / step ----------

#[test]
fn team_ranks_out_of_range_rejected() {
    let mut o = Opts::four_seat();
    o.level_table = r#"levelTable:
  id: lv
  initial: "2"
  ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"]
  upgrade: [ { teamRanks: [1,5], step: 3 } ]"#;
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "levelTable.upgrade[0].teamRanks",
    );
}

#[test]
fn team_ranks_not_ascending_rejected() {
    let mut o = Opts::four_seat();
    o.level_table = r#"levelTable:
  id: lv
  initial: "2"
  ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"]
  upgrade: [ { teamRanks: [2,1], step: 3 } ]"#;
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "levelTable.upgrade[0].teamRanks",
    );
}

#[test]
fn team_ranks_duplicate_element_rejected() {
    let mut o = Opts::four_seat();
    o.level_table = r#"levelTable:
  id: lv
  initial: "2"
  ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"]
  upgrade: [ { teamRanks: [1,1], step: 3 } ]"#;
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "levelTable.upgrade[0].teamRanks",
    );
}

#[test]
fn team_ranks_length_mismatch_team_size_rejected() {
    // 六人两队 teamSize=3，写长度 2 的 [1,2] 必须被拒。
    let mut o = Opts::four_seat();
    o.seats = "{ count: 6, teams: [[0,2,4],[1,3,5]] }";
    o.per_seat = 18;
    o.min_players = 6;
    o.level_table = r#"levelTable:
  id: lv
  initial: "2"
  ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"]
  upgrade: [ { teamRanks: [1,2], step: 3 } ]"#;
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "levelTable.upgrade[0].teamRanks",
    );
}

#[test]
fn step_zero_is_accepted() {
    // step: 0 = 该组合不升档，合法值，不得写成拒绝用例。
    let mut o = Opts::four_seat();
    o.level_table = r#"levelTable:
  id: lv
  initial: "2"
  ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"]
  upgrade: [ { teamRanks: [1,2], step: 0 } ]"#;
    assert_clean(&base(&o));
}

#[test]
fn step_out_of_range_rejected() {
    let mut o = Opts::four_seat();
    o.level_table = r#"levelTable:
  id: lv
  initial: "2"
  ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"]
  upgrade: [ { teamRanks: [1,2], step: 4 } ]"#;
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "levelTable.upgrade[0].step",
    );
}

// ---------- score-diff：键集互斥 / 兜底 / 降序 / 分值长度 / 计分维度 ----------

#[test]
fn score_diff_with_upgrade_rows_rejected() {
    let mut o = Opts::four_seat();
    o.level_table = r#"levelTable:
  id: lv
  initial: "2"
  ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"]
  upgradeBasis: score-diff
  upgrade: [ { teamRanks: [1,2], step: 3 } ]
  rankScores: [10,8,6,4]
  upgradeByDiff:
    - { minDiff: 0, step: 0 }"#;
    assert_gate(&base(&o), ErrorCode::RsSchemaInvalid, "levelTable.upgrade");
}

#[test]
fn score_diff_missing_zero_fallback_rejected() {
    let mut o = Opts::four_seat();
    o.level_table = r#"levelTable:
  id: lv
  initial: "2"
  ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"]
  upgradeBasis: score-diff
  rankScores: [10,8,6,4]
  upgradeByDiff:
    - { minDiff: 6, step: 2 }
    - { minDiff: 3, step: 1 }"#;
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "levelTable.upgradeByDiff",
    );
}

#[test]
fn score_diff_not_descending_rejected() {
    let mut o = Opts::four_seat();
    o.level_table = r#"levelTable:
  id: lv
  initial: "2"
  ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"]
  upgradeBasis: score-diff
  rankScores: [10,8,6,4]
  upgradeByDiff:
    - { minDiff: 3, step: 1 }
    - { minDiff: 6, step: 2 }
    - { minDiff: 0, step: 0 }"#;
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "levelTable.upgradeByDiff[1].minDiff",
    );
}

#[test]
fn rank_scores_length_mismatch_rejected() {
    let mut o = Opts::four_seat();
    o.level_table = r#"levelTable:
  id: lv
  initial: "2"
  ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"]
  upgradeBasis: score-diff
  rankScores: [10,8,6]
  upgradeByDiff:
    - { minDiff: 0, step: 0 }"#;
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "levelTable.rankScores",
    );
}

#[test]
fn score_diff_with_player_dimension_rejected() {
    let mut o = Opts::four_seat();
    o.score_dim = "player";
    o.level_table = r#"levelTable:
  id: lv
  initial: "2"
  ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"]
  upgradeBasis: score-diff
  rankScores: [10,8,6,4]
  upgradeByDiff:
    - { minDiff: 0, step: 0 }"#;
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "flow.score.dimension",
    );
}

#[test]
fn score_diff_rule_set_accepted() {
    let mut o = Opts::four_seat();
    o.level_table = r#"levelTable:
  id: lv
  initial: "2"
  ranks: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"]
  upgradeBasis: score-diff
  rankScores: [10,8,6,4]
  upgradeByDiff:
    - { minDiff: 6, step: 2 }
    - { minDiff: 3, step: 1 }
    - { minDiff: 0, step: 0 }"#;
    assert_clean(&base(&o));
}

// ---------- level.pass.combos：长度 / 序 / 重复 / 缺省 ----------

#[test]
fn combos_length_mismatch_team_size_rejected() {
    // 六人两队 teamSize=3，组合 [1,2] 长度 2 必须被拒。
    let mut o = Opts::four_seat();
    o.seats = "{ count: 6, teams: [[0,2,4],[1,3,5]] }";
    o.per_seat = 18;
    o.min_players = 6;
    o.level_block = r#"level:
  pass: { combos: [[1,2]], streakLimit: 3, resetOnFail: true }
  current: { scope: by-team }"#;
    assert_gate(&base(&o), ErrorCode::RsSchemaInvalid, "level.pass.combos");
}

#[test]
fn combos_not_ascending_rejected() {
    let mut o = Opts::four_seat();
    o.level_block = r#"level:
  pass: { combos: [[2,1]], streakLimit: 3, resetOnFail: true }
  current: { scope: by-team }"#;
    assert_gate(&base(&o), ErrorCode::RsSchemaInvalid, "level.pass.combos");
}

#[test]
fn combos_duplicate_element_rejected() {
    let mut o = Opts::four_seat();
    o.level_block = r#"level:
  pass: { combos: [[1,1]], streakLimit: 3, resetOnFail: true }
  current: { scope: by-team }"#;
    assert_gate(&base(&o), ErrorCode::RsSchemaInvalid, "level.pass.combos");
}

#[test]
fn combos_missing_for_team_size_three_rejected() {
    // teamSize=3 时缺省 combos 无定义，必须显式声明。
    let mut o = Opts::four_seat();
    o.seats = "{ count: 6, teams: [[0,2,4],[1,3,5]] }";
    o.per_seat = 18;
    o.min_players = 6;
    o.level_block = r#"level:
  pass: { streakLimit: 3, resetOnFail: true }
  current: { scope: by-team }"#;
    assert_gate(&base(&o), ErrorCode::RsSchemaInvalid, "level.pass.combos");
}

// ---------- patterns[].params 取值范围（§5.2 第 513–518 行） ----------

#[test]
fn bomb_count_out_of_range_rejected() {
    for count in [3u8, 12] {
        let mut o = Opts::four_seat();
        let patterns =
            format!("  - {{ id: bomb, primitiveId: pattern.bomb, params: {{ count: {count} }} }}");
        o.patterns = &patterns;
        assert_gate(
            &base(&o),
            ErrorCode::RsSchemaInvalid,
            "patterns[0].params.count",
        );
    }
}

#[test]
fn bomb_count_boundaries_accepted() {
    for count in [4u8, 8] {
        let mut o = Opts::four_seat();
        let patterns =
            format!("  - {{ id: bomb, primitiveId: pattern.bomb, params: {{ count: {count} }} }}");
        o.patterns = &patterns;
        assert_clean(&base(&o));
    }
}

#[test]
fn same_rank_count_out_of_range_rejected() {
    let mut o = Opts::four_seat();
    o.patterns = "  - { id: s, primitiveId: pattern.same-rank, params: { count: 9 } }";
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "patterns[0].params.count",
    );
}

#[test]
fn straight_length_out_of_range_rejected() {
    let mut o = Opts::four_seat();
    o.patterns = "  - { id: st, primitiveId: pattern.straight, params: { length: 4 } }";
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "patterns[0].params.length",
    );
}

#[test]
fn consecutive_pairs_out_of_range_rejected() {
    let mut o = Opts::four_seat();
    o.patterns = "  - { id: cp, primitiveId: pattern.consecutive-pairs, params: { pairs: 1 } }";
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "patterns[0].params.pairs",
    );
}

#[test]
fn consecutive_triples_out_of_range_rejected() {
    let mut o = Opts::four_seat();
    o.patterns = "  - { id: ct, primitiveId: pattern.consecutive-triples, params: { triples: 1 } }";
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "patterns[0].params.triples",
    );
}

#[test]
fn straight_flush_length_out_of_range_rejected() {
    let mut o = Opts::four_seat();
    o.patterns = "  - { id: sf, primitiveId: pattern.straight-flush, params: { length: 4 } }";
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "patterns[0].params.length",
    );
}

// ---------- tribute 必填性与 pairs 值域 ----------

#[test]
fn tribute_omitted_rejected() {
    let mut o = Opts::four_seat();
    o.tribute = "";
    assert_gate(&base(&o), ErrorCode::RsSchemaInvalid, "tribute");
}

#[test]
fn tribute_enabled_missing_return_rejected() {
    let mut o = Opts::four_seat();
    o.tribute = r#"enabled: true
trigger: last-round-rank
pairs:
  - { payer: 4, receiver: 1, when: single-loss }
resist: two-big-jokers"#;
    assert_gate(&base(&o), ErrorCode::RsSchemaInvalid, "tribute.return");
}

#[test]
fn tribute_payer_out_of_range_rejected() {
    let mut o = Opts::four_seat();
    o.tribute = r#"enabled: true
trigger: last-round-rank
pairs:
  - { payer: 5, receiver: 1, when: single-loss }
return: { ceiling: under-ten, count: 1, fallback: smallest }
resist: two-big-jokers"#;
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "tribute.pairs[0].payer",
    );
}

#[test]
fn tribute_disabled_single_key_accepted() {
    let mut o = Opts::four_seat();
    o.tribute = "enabled: false";
    assert_clean(&base(&o));
}

// ---------- 原语侧参数值域（§5.2 参数表第 510/525/532/533/541/543 行） ----------

#[test]
fn tribute_pay_count_out_of_range_rejected() {
    let mut o = Opts::four_seat();
    o.actions = "  - { kind: pass, primitive: tribute.pay, legalFrom: play, params: { count: 3, selection: max-rank-except-wildcard } }";
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "actions[0].params.count",
    );
}

#[test]
fn tribute_pay_count_boundaries_accepted() {
    for count in [1u8, 2] {
        let mut o = Opts::four_seat();
        let actions = format!("  - {{ kind: pass, primitive: tribute.pay, legalFrom: play, params: {{ count: {count}, selection: max-rank-except-wildcard }} }}");
        o.actions = &actions;
        assert_clean(&base(&o));
    }
}

#[test]
fn tribute_return_count_out_of_range_rejected() {
    let mut o = Opts::four_seat();
    o.actions = "  - { kind: pass, primitive: tribute.return, legalFrom: play, params: { ceiling: under-ten, count: 3, fallback: smallest } }";
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "actions[0].params.count",
    );
}

#[test]
fn tribute_return_count_boundaries_accepted() {
    for count in [1u8, 2] {
        let mut o = Opts::four_seat();
        let actions = format!("  - {{ kind: pass, primitive: tribute.return, legalFrom: play, params: {{ ceiling: under-ten, count: {count}, fallback: smallest }} }}");
        o.actions = &actions;
        assert_clean(&base(&o));
    }
}

#[test]
fn deal_shuffle_per_seat_out_of_range_rejected() {
    let mut o = Opts::four_seat();
    o.actions = "  - { kind: pass, primitive: deal.shuffle, legalFrom: play, params: { zone: hand, perSeat: 28 } }";
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "actions[0].params.perSeat",
    );
}

#[test]
fn deal_shuffle_per_seat_boundaries_accepted() {
    for per_seat in [1u16, 27] {
        let mut o = Opts::four_seat();
        let actions = format!("  - {{ kind: pass, primitive: deal.shuffle, legalFrom: play, params: {{ zone: hand, perSeat: {per_seat} }} }}");
        o.actions = &actions;
        assert_clean(&base(&o));
    }
}

#[test]
fn consume_cards_count_out_of_range_rejected() {
    let mut o = Opts::four_seat();
    o.actions = "  - { kind: pass, primitive: action.consume-cards, legalFrom: patterns, params: { zone: hand, count: 9 } }";
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "actions[0].params.count",
    );
}

#[test]
fn consume_cards_count_boundaries_accepted() {
    for count in [0u8, 8] {
        let mut o = Opts::four_seat();
        let actions = format!("  - {{ kind: pass, primitive: action.consume-cards, legalFrom: patterns, params: {{ zone: hand, count: {count} }} }}");
        o.actions = &actions;
        assert_clean(&base(&o));
    }
}

#[test]
fn rng_pick_stream_out_of_range_rejected() {
    let mut o = Opts::four_seat();
    o.actions = "  - { kind: pass, primitive: rng.pick, legalFrom: play, params: { stream: 4 } }";
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "actions[0].params.stream",
    );
}

#[test]
fn rng_pick_stream_boundaries_accepted() {
    for stream in [0u32, 3] {
        let mut o = Opts::four_seat();
        let actions = format!("  - {{ kind: pass, primitive: rng.pick, legalFrom: play, params: {{ stream: {stream} }} }}");
        o.actions = &actions;
        assert_clean(&base(&o));
    }
}

#[test]
fn min_players_count_out_of_range_rejected() {
    let mut o = Opts::four_seat();
    o.actions =
        "  - { kind: pass, primitive: assert.min-players, legalFrom: play, params: { count: 9 } }";
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "actions[0].params.count",
    );
}

#[test]
fn min_players_count_boundaries_accepted() {
    for count in [1u8, 8] {
        let mut o = Opts::four_seat();
        let actions = format!("  - {{ kind: pass, primitive: assert.min-players, legalFrom: play, params: {{ count: {count} }} }}");
        o.actions = &actions;
        assert_clean(&base(&o));
    }
}

// ---------- level.pass 原语侧 combos 值域（问题 a 收口） ----------

#[test]
fn level_pass_primitive_combos_out_of_range_rejected() {
    let mut o = Opts::four_seat();
    o.actions = "  - { kind: pass, primitive: level.pass, legalFrom: play, params: { combos: [[2,1]], streakLimit: 3, resetOnFail: true } }";
    assert_gate(
        &base(&o),
        ErrorCode::RsSchemaInvalid,
        "actions[0].params.combos",
    );
}

#[test]
fn level_pass_primitive_combos_accepted() {
    let mut o = Opts::four_seat();
    o.actions = "  - { kind: pass, primitive: level.pass, legalFrom: play, params: { combos: [[1,2]], streakLimit: 3, resetOnFail: true } }";
    assert_clean(&base(&o));
}
