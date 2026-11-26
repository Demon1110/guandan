//! L1 构造期与校验期的错误分支覆盖（纯补测，不改实现）。
//!
//! 做法：夹具用 `include_str!` 原样嵌入，每个用例只「改一处」，锚点不存在即断言失败，
//! 夹具漂移后不会静默变成空跑。构造期用例断言 `from_yaml` 返回 `RS_SCHEMA_INVALID`；
//! 校验期用例断言 `validate_with(.., &[Level::L1], ..)` 在指定 location 上出 finding。

use gf_engine::contract::{ErrorCode, Level};
use gf_engine::ruleset::parse::{from_yaml, SchemaError};
use gf_engine::ruleset::validate_deep::{GoldenCase, ValidateOptions};
use gf_engine::ruleset::validate_with;

const M0: &str = include_str!("fixtures/m0-minimal/ruleset.yaml");
const CLASSIC: &str = include_str!("fixtures/guandan-classic/ruleset.yaml");

/// 替换唯一锚点；找不到直接失败（防止夹具改名后用例静默通过）。
fn one(src: &str, from: &str, to: &str) -> String {
    assert!(src.contains(from), "夹具里找不到锚点：{from}");
    src.replace(from, to)
}

fn parse_err(name: &str, src: &str) -> SchemaError {
    match from_yaml(src) {
        Ok(_) => panic!("{name}：应被构造期拒绝，实际构造成功"),
        Err(e) => {
            assert_eq!(
                e.code,
                ErrorCode::RsSchemaInvalid,
                "{name}：构造期错误码必须是 RS_SCHEMA_INVALID"
            );
            e
        }
    }
}

fn l1_locs(src: &str) -> Vec<String> {
    let body = from_yaml(src).unwrap_or_else(|e| panic!("变异体应可构造：{e}"));
    let v = validate_with(
        &body,
        &[Level::L1],
        &ValidateOptions::default(),
        &Vec::<GoldenCase>::new(),
    );
    v.findings.iter().map(|f| f.location.clone()).collect()
}

fn expect_l1(name: &str, src: &str, loc: &str) {
    let locs = l1_locs(src);
    assert!(
        locs.iter().any(|l| l.starts_with(loc)),
        "{name}：L1 应在 {loc} 出 finding，实际 {locs:?}"
    );
}

// ---------- 构造期：反序列化 / 原语分发 ----------

#[test]
fn schema_rejects_bad_rank_key_and_unknown_keys() {
    let rank = one(M0, r#"naturalOrder: ["2""#, r#"naturalOrder: ["1""#);
    let e = parse_err("RankKey 非枚举字面值", &rank);
    assert!(e.message.contains("unknown RankKey"), "{}", e.message);

    let unknown_field = one(
        M0,
        "{ id: single, primitiveId",
        "{ foo: 1, id: single, primitiveId",
    );
    parse_err("pattern 未知键", &unknown_field);

    // 非字符串键走 `as_str()` 为 None 的 continue 分支，不应报未知键。
    let numeric_key = one(
        M0,
        "{ id: single, primitiveId",
        "{ 3: x, id: single, primitiveId",
    );
    let body = from_yaml(&numeric_key).expect("数字键应被跳过而非拒绝");
    assert_eq!(body.patterns.len(), 1);

    let action_unknown = one(
        M0,
        "{ kind: pass, primitive: action.pass",
        "{ kind: pass, bar: 1, primitive: action.pass",
    );
    parse_err("action 未知键", &action_unknown);

    let action_numeric = one(
        M0,
        "{ kind: pass, primitive",
        "{ 7: x, kind: pass, primitive",
    );
    from_yaml(&action_numeric).expect("action 数字键应被跳过");
}

#[test]
fn schema_rejects_no_param_primitive_without_params() {
    let pattern = one(
        M0,
        "patterns:\n  - {",
        "patterns:\n  - { id: jb, primitiveId: pattern.joker-bomb }\n  - {",
    );
    let e = parse_err("无参原语缺 params", &pattern);
    assert!(e.message.contains("无参原语"), "{}", e.message);

    let action = one(
        M0,
        "action.pass, legalFrom: play, params: {} }",
        "action.pass, legalFrom: play }",
    );
    let e = parse_err("无参 action 缺 params", &action);
    assert!(e.message.contains("无参原语"), "{}", e.message);

    // 有参原语缺 params：走「默认空映射」分支，再由必填键缺失报错。
    let missing = one(M0, ", params: { count: 1 } }", " }");
    let e = parse_err("same-rank 缺 count", &missing);
    assert!(e.message.contains("count"), "{}", e.message);
}

#[test]
fn schema_covers_settle_variants() {
    let level_up_extra = one(
        M0,
        "settle: { algorithm: score-only }",
        "settle: { algorithm: level-up, levelTableRef: gd, foo: 1 }",
    );
    parse_err("level-up 未知键", &level_up_extra);

    let level_up_numeric = one(
        M0,
        "settle: { algorithm: score-only }",
        "settle: { algorithm: level-up, levelTableRef: gd, 1: x }",
    );
    from_yaml(&level_up_numeric).expect("settle 数字键应被跳过");

    let level_up_no_ref = one(
        M0,
        "settle: { algorithm: score-only }",
        "settle: { algorithm: level-up }",
    );
    let e = parse_err("level-up 缺 levelTableRef", &level_up_no_ref);
    assert!(e.message.contains("levelTableRef"), "{}", e.message);

    let score_extra = one(
        M0,
        "settle: { algorithm: score-only }",
        "settle: { algorithm: score-only, foo: 1 }",
    );
    parse_err("score-only 未知键", &score_extra);

    let score_numeric = one(
        M0,
        "settle: { algorithm: score-only }",
        "settle: { algorithm: score-only, 1: x }",
    );
    from_yaml(&score_numeric).expect("score-only 数字键应被跳过");

    let unknown_algo = one(M0, "algorithm: score-only", "algorithm: winner-only");
    parse_err("未知 settle algorithm", &unknown_algo);
}

#[test]
fn schema_covers_wildcard_rank_and_compose_shape() {
    let concrete = one(CLASSIC, r#"rank: "@level""#, r#"rank: "5""#);
    let body = from_yaml(&concrete).expect("具体点数作 wildcard 源应可构造");
    assert!(body.wildcard.is_some());

    let bad = one(CLASSIC, r#"rank: "@level""#, r#"rank: "zz""#);
    let e = parse_err("wildcard 非法点数", &bad);
    assert!(e.message.contains("unknown wildcard rank"), "{}", e.message);

    let extra = one(
        M0,
        "patterns:\n  - {",
        "patterns:\n  - { id: cmp, primitiveId: pattern.compose, params: { shape: { or: {} }, maxCards: 5 } }\n  - {",
    );
    parse_err("compose 判断器闭集外", &extra);

    let empty_shape = one(
        M0,
        "patterns:\n  - {",
        "patterns:\n  - { id: cmp, primitiveId: pattern.compose, params: { shape: {}, maxCards: 5 } }\n  - {",
    );
    parse_err("compose shape 空映射", &empty_shape);

    let two_shapes = one(
        M0,
        "patterns:\n  - {",
        "patterns:\n  - { id: cmp, primitiveId: pattern.compose, params: { shape: { rank.same: { count: 2 }, suit.same: {} }, maxCards: 5 } }\n  - {",
    );
    parse_err("compose shape 多键", &two_shapes);
}

#[test]
fn schema_covers_bad_enum_literals() {
    parse_err("action kind 非法", &one(M0, "kind: pass", "kind: fly"));
    parse_err(
        "levelInsert 非法",
        &one(M0, "levelInsert: above-A", "levelInsert: mid-A"),
    );
    parse_err(
        "zone scope 非法",
        &one(M0, "scope: shared", "scope: everyone"),
    );
}

// ---------- 校验期：L1 finding ----------

#[test]
fn l1_covers_seats_timing_and_caps() {
    expect_l1(
        "seats.count 越界",
        &one(CLASSIC, "count: 4, teams", "count: 1, teams"),
        "seats.count",
    );

    let timing = one(
        CLASSIC,
        "matchCap: { rounds: 12 }",
        "timing: { turnTimeSec: 1, graceSec: 1, offlineAllSec: 1 }\nmatchCap: { rounds: 12 }",
    );
    let locs = l1_locs(&timing);
    for want in [
        "timing.turnTimeSec",
        "timing.graceSec",
        "timing.offlineAllSec",
    ] {
        assert!(locs.iter().any(|l| l == want), "应报 {want}，实际 {locs:?}");
    }

    expect_l1(
        "matchCap 0 轮",
        &one(CLASSIC, "rounds: 12", "rounds: 0"),
        "matchCap.rounds",
    );

    expect_l1(
        "ladder 混入插入型",
        &one(
            CLASSIC,
            "bomb-8, joker-bomb]",
            "bomb-8, joker-bomb, straight-flush-5]",
        ),
        "compare.ladder",
    );
}

#[test]
fn l1_covers_team_sizes_and_level_table() {
    expect_l1(
        "空队伍集",
        &one(CLASSIC, "teams: [[0,2],[1,3]]", "teams: []"),
        "seats.teams",
    );
    expect_l1(
        "队伍规模不一致",
        &one(CLASSIC, "teams: [[0,2],[1,3]]", "teams: [[0,2],[1]]"),
        "seats.teams",
    );
    // 队规模 1：缺省 pass.combos 无定义，必须显式声明。
    expect_l1(
        "teamSize≠2 且缺省 combos",
        &one(
            &one(CLASSIC, "teams: [[0,2],[1,3]]", "teams: [[0],[1],[2],[3]]"),
            "matchCap: { rounds: 12 }",
            "level: { current: { scope: by-team }, pass: { streakLimit: 1, resetOnFail: true } }\nmatchCap: { rounds: 12 }",
        ),
        "level.pass.combos",
    );
}

#[test]
fn l1_covers_level_table_variants() {
    let no_upgrade = one(
        CLASSIC,
        "  upgrade: [ { teamRanks: [1,2], step: 3 },\n             { teamRanks: [1,3], step: 2 },\n             { teamRanks: [1,4], step: 1 } ]",
        "",
    );
    expect_l1("rank-combo 缺 upgrade", &no_upgrade, "levelTable.upgrade");

    let with_scores = one(
        CLASSIC,
        "  upgrade: [ {",
        "  rankScores: [10,20,30,40]\n  upgrade: [ {",
    );
    let locs = l1_locs(&with_scores);
    assert!(
        locs.iter()
            .any(|l| l == "levelTable.rankScores" || l == "levelTable.upgrade"),
        "两个 basis 键集混用应报错，实际 {locs:?}"
    );

    let dup = one(
        CLASSIC,
        "{ teamRanks: [1,3], step: 2 }",
        "{ teamRanks: [1,2], step: 2 }",
    );
    expect_l1("teamRanks 重复", &dup, "levelTable.upgrade[1].teamRanks");

    let bad_step = one(
        CLASSIC,
        "{ teamRanks: [1,4], step: 1 }",
        "{ teamRanks: [1,4], step: 4 }",
    );
    expect_l1("upgrade step 越界", &bad_step, "levelTable.upgrade[2].step");

    let bad_len = one(CLASSIC, "[1,2], step: 3 }", "[1,2,3], step: 3 }");
    expect_l1(
        "teamRanks 长度≠teamSize",
        &bad_len,
        "levelTable.upgrade[0].teamRanks",
    );
}

#[test]
fn l1_covers_level_pass() {
    expect_l1(
        "combos 空表",
        &one(
            CLASSIC,
            "matchCap: { rounds: 12 }",
            "level: { current: { scope: by-team }, pass: { combos: [], streakLimit: 1, resetOnFail: true } }\nmatchCap: { rounds: 12 }",
        ),
        "level.pass.combos",
    );
    expect_l1(
        "combos 重复",
        &one(
            CLASSIC,
            "matchCap: { rounds: 12 }",
            "level: { current: { scope: by-team }, pass: { combos: [[1,2],[1,2]], streakLimit: 1, resetOnFail: true } }\nmatchCap: { rounds: 12 }",
        ),
        "level.pass.combos",
    );
    expect_l1(
        "streakLimit 越界",
        &one(
            CLASSIC,
            "matchCap: { rounds: 12 }",
            "level: { current: { scope: by-team }, pass: { combos: [[1,2],[1,3]], streakLimit: 5, resetOnFail: true } }\nmatchCap: { rounds: 12 }",
        ),
        "level.pass.streakLimit",
    );
}

#[test]
fn l1_covers_deck_and_deal() {
    expect_l1(
        "jokers≠2",
        &one(CLASSIC, "jokers: 2, exclude", "jokers: 3, exclude"),
        "deck.jokers",
    );
    expect_l1(
        "ranks 含王",
        &one(
            CLASSIC,
            r#"ranks: ["2","3""#,
            r#"ranks: ["2","little-joker","3""#,
        ),
        "deck.ranks",
    );
    expect_l1(
        "perSeat 越界",
        &one(CLASSIC, "perSeat: 27", "perSeat: 200"),
        "flow.deal.perSeat",
    );

    let uneven = one(
        &one(
            CLASSIC,
            "deal:   { zone: hand, perSeat: 27, reveal: false }",
            "deal:   { zone: hand, reveal: false }",
        ),
        "copies: 2, ranks",
        "copies: 1, ranks",
    );
    expect_l1("总张数不能整除", &uneven, "flow.deal.perSeat");

    let two_private = one(
        &one(
            CLASSIC,
            "deal:   { zone: hand, perSeat: 27, reveal: false }",
            "deal:   { perSeat: 27, reveal: false }",
        ),
        "  - { id: discard,",
        "  - { id: hidden, scope: private, capacity: 27 }\n  - { id: discard,",
    );
    expect_l1(
        "两个 private 区且未声明落点",
        &two_private,
        "flow.deal.zone",
    );

    expect_l1(
        "rng stream 越界",
        &one(CLASSIC, "rng:    { stream: 0 }", "rng:    { stream: 9 }"),
        "flow.rng.stream",
    );
    expect_l1(
        "minPlayers 越界",
        &one(CLASSIC, "minPlayers: 4", "minPlayers: 0"),
        "flow.assert.minPlayers",
    );
}

#[test]
fn l1_covers_lifecycle_and_tribute() {
    expect_l1(
        "lifecycle 少一段",
        &one(
            CLASSIC,
            "lifecycle: [deal, tribute, play, settle]",
            "lifecycle: [deal, play, settle]",
        ),
        "lifecycle",
    );

    let no_trigger = one(CLASSIC, "  trigger: last-round-rank\n", "");
    expect_l1("tribute 缺 trigger", &no_trigger, "tribute.trigger");

    let pairs_block = "  pairs:\n    - { payer: 4, receiver: 1, when: single-loss }\n    - { payer: 4, receiver: 1, when: double-loss }\n    - { payer: 3, receiver: 2, when: double-loss }\n";
    expect_l1(
        "tribute 缺 pairs",
        &one(CLASSIC, pairs_block, ""),
        "tribute.pairs",
    );
    expect_l1(
        "tribute pairs 空表",
        &one(CLASSIC, pairs_block, "  pairs: []\n"),
        "tribute.pairs",
    );

    expect_l1(
        "tribute 缺 return",
        &one(
            CLASSIC,
            "  return: { ceiling: under-ten, count: 1, fallback: smallest }\n",
            "",
        ),
        "tribute.return",
    );
    expect_l1(
        "tribute 缺 resist",
        &one(CLASSIC, "  resist: two-big-jokers\n", ""),
        "tribute.resist",
    );
    expect_l1(
        "payer 越界",
        &one(
            CLASSIC,
            "{ payer: 4, receiver: 1, when: single-loss }",
            "{ payer: 9, receiver: 1, when: single-loss }",
        ),
        "tribute.pairs[0].payer",
    );
    expect_l1(
        "receiver 越界",
        &one(
            CLASSIC,
            "receiver: 1, when: single-loss",
            "receiver: 9, when: single-loss",
        ),
        "tribute.pairs[0].receiver",
    );
}

#[test]
fn l1_covers_pattern_param_ranges() {
    let cases = [
        ("same-rank count 0", one(CLASSIC, "{ count: 1 }", "{ count: 0 }"), "patterns[0].params"),
        (
            "straight length 4",
            one(CLASSIC, "id: straight-5,      primitiveId: pattern.straight,            params: { length: 5 }", "id: straight-5,      primitiveId: pattern.straight,            params: { length: 4 }"),
            "patterns[4].params",
        ),
        (
            "consecutive-pairs pairs 1",
            one(CLASSIC, "params: { pairs: 3 }", "params: { pairs: 1 }"),
            "patterns[5].params",
        ),
        (
            "consecutive-triples triples 1",
            one(CLASSIC, "params: { triples: 2 }", "params: { triples: 1 }"),
            "patterns[6].params",
        ),
        (
            "bomb count 3",
            one(CLASSIC, "id: bomb-4,          primitiveId: pattern.bomb,                params: { count: 4 }", "id: bomb-4,          primitiveId: pattern.bomb,                params: { count: 3 }"),
            "patterns[7].params",
        ),
        (
            "straight-flush length 4",
            one(CLASSIC, "id: straight-flush-5, primitiveId: pattern.straight-flush,      params: { length: 5 }", "id: straight-flush-5, primitiveId: pattern.straight-flush,      params: { length: 4 }"),
            "patterns[12].params",
        ),
    ];
    for (name, src, loc) in cases {
        expect_l1(name, &src, loc);
    }
}

#[test]
fn l1_covers_compose_branches() {
    let add = |params: &str| {
        one(
            CLASSIC,
            "patterns:\n  - {",
            &format!("patterns:\n  - {{ id: cmp, primitiveId: pattern.compose, params: {params} }}\n  - {{"),
        )
    };
    let cases = [
        ("maxCards 越界", add("{ shape: { rank.same: { count: 2 } }, maxCards: 1 }"), "patterns[0].params"),
        ("rank.same count 越界", add("{ shape: { rank.same: { count: 0 } }, maxCards: 5 }"), "patterns[0].params"),
        (
            "rank.contiguous steps 越界",
            add("{ shape: { rank.contiguous: { steps: 1, step: 1 } }, maxCards: 5 }"),
            "patterns[0].params",
        ),
        (
            "rank.contiguous step 越界",
            add("{ shape: { rank.contiguous: { steps: 5, step: 4 } }, maxCards: 5 }"),
            "patterns[0].params",
        ),
        (
            "joker.only big 越界",
            add("{ shape: { joker.only: { big: 3, small: 0 } }, maxCards: 5 }"),
            "patterns[0].params",
        ),
        (
            "attach count 越界",
            add("{ shape: { rank.same: { count: 3 } }, maxCards: 5, attach: { count: 0, unit: single } }"),
            "patterns[0].params",
        ),
        (
            "wheel 非法组合",
            add("{ shape: { rank.same: { count: 2 } }, maxCards: 5, wheel: true }"),
            "patterns[0].params",
        ),
        ("suit.same 合法", add("{ shape: { suit.same: {} }, maxCards: 5 }"), ""),
    ];
    for (name, src, loc) in cases {
        if loc.is_empty() {
            assert!(l1_locs(&src).is_empty(), "{name}：不应有任何 L1 finding");
        } else {
            expect_l1(name, &src, loc);
        }
    }
}

#[test]
fn l1_covers_duplicate_shape_reserved_and_legal_from() {
    expect_l1(
        "同形状重复",
        &one(
            CLASSIC,
            "patterns:\n  - {",
            "patterns:\n  - { id: single-dup, primitiveId: pattern.same-rank, params: { count: 1 } }\n  - {",
        ),
        "patterns[1].params",
    );
    expect_l1(
        "reserved 原语（pattern）",
        &one(
            CLASSIC,
            "  - { id: joker-bomb,",
            "  - { id: wa, primitiveId: pattern.with-attachment, params: {} }\n  - { id: joker-bomb,",
        ),
        "patterns[13].primitiveId",
    );
    expect_l1(
        "reserved 原语（action）",
        &one(
            CLASSIC,
            "  - { kind: pass,",
            "  - { kind: pass, primitive: action.claim, legalFrom: play, params: {} }\n  - { kind: pass,",
        ),
        "actions[1].primitive",
    );
    expect_l1(
        "legalFrom=turn",
        &one(
            CLASSIC,
            "legalFrom: play,     params: {}",
            "legalFrom: turn,     params: {}",
        ),
        "actions[1].legalFrom",
    );
}

#[test]
fn l1_covers_flush_anchor_and_ladder_conflicts() {
    // 锚点缺失：M0 没有炸弹牌型，插入型自然找不到 5 张锚点。
    let missing_anchor = one(
        M0,
        "patterns:\n  - {",
        "patterns:\n  - { id: sf, primitiveId: pattern.straight-flush, params: { length: 5 } }\n  - {",
    );
    let locs = l1_locs(&missing_anchor);
    assert!(
        locs.iter().any(|l| l == "compare.ladder"),
        "锚点缺失应在 compare.ladder 出 finding，实际 {locs:?}"
    );

    // 插入位争夺：classic 再加一个 straight-flush 牌型。
    let two_flush = one(
        CLASSIC,
        "  - { id: joker-bomb,",
        "  - { id: straight-flush-6, primitiveId: pattern.straight-flush, params: { length: 6 } }\n  - { id: joker-bomb,",
    );
    expect_l1("插入位被两个牌型争夺", &two_flush, "compare.flushRank");

    // 位次矛盾：把 bomb-6 排到 bomb-5 前面。
    let swapped = one(
        CLASSIC,
        "bomb-4, bomb-5, bomb-6,",
        "bomb-4, bomb-6, bomb-5,",
    );
    expect_l1("炸弹相对序矛盾", &swapped, "compare.ladder");
}
