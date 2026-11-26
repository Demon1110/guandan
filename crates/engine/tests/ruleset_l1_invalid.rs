//! L1 闸门的「非法配置」矩阵：每条用例只破坏一个字段，断言 `validate(L1)`
//! 真的抛出了对应的 `code` 与 `location`。
//!
//! 与 `ruleset_l1_gates.rs` 的分工：那里按字段逐条证明闸门存在（内联迷你 YAML）；
//! 这里在完整 `guandan-classic` 上做单点破坏，用来扫「合法规则集里少见的取值组合」
//! ——timing / matchCap / basis 互斥 / 队伍规模 / 贡牌三键 / 原语参数 / legalFrom。

use gf_engine::contract::{ErrorCode, Level};
use gf_engine::ruleset::{from_yaml, validate};
use std::fs;
use std::path::PathBuf;

fn classic_src() -> String {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/guandan-classic/ruleset.yaml");
    fs::read_to_string(p).expect("夹具读取失败")
}

fn swap(src: &str, from: &str, to: &str) -> String {
    assert!(src.contains(from), "替换目标 {from:?} 不存在");
    src.replace(from, to)
}

/// 把第一条含 `needle` 的行整行替换为 `new_line`（比多行字面替换稳）。
fn replace_line(src: &str, needle: &str, new_line: &str) -> String {
    let mut done = false;
    let out: Vec<String> = src
        .lines()
        .map(|l| {
            if !done && l.contains(needle) {
                done = true;
                new_line.to_owned()
            } else {
                l.to_owned()
            }
        })
        .collect();
    assert!(done, "找不到含 {needle:?} 的行");
    out.join("\n")
}

/// 删掉所有含 `needle` 的行。
fn drop_lines(src: &str, needle: &str) -> String {
    src.lines()
        .filter(|l| !l.contains(needle))
        .collect::<Vec<_>>()
        .join("\n")
}

type Expect = (ErrorCode, &'static str);

fn cases() -> Vec<(&'static str, String, Vec<Expect>)> {
    let base = classic_src();
    let s = |f: &str, t: &str| swap(&base, f, t);
    let d = |n: &str| drop_lines(&base, n);
    let mut out: Vec<(&'static str, String, Vec<Expect>)> = Vec::new();

    out.push((
        "seats-count-1",
        s("count: 4, teams", "count: 1, teams"),
        vec![(ErrorCode::RsSchemaInvalid, "seats.count")],
    ));
    out.push((
        "seats-count-9",
        s("count: 4, teams", "count: 9, teams"),
        vec![(ErrorCode::RsSchemaInvalid, "seats.count")],
    ));
    out.push((
        "timing-out-of-range",
        s(
            "matchCap: { rounds: 12 }",
            "timing: { turnTimeSec: 0, graceSec: 10, offlineAllSec: 900 }\nmatchCap: { rounds: 12 }",
        ),
        vec![
            (ErrorCode::RsSchemaInvalid, "timing.turnTimeSec"),
            (ErrorCode::RsSchemaInvalid, "timing.graceSec"),
            (ErrorCode::RsSchemaInvalid, "timing.offlineAllSec"),
        ],
    ));
    out.push((
        "matchcap-zero",
        s("matchCap: { rounds: 12 }", "matchCap: { rounds: 0 }"),
        vec![(ErrorCode::RsSchemaInvalid, "matchCap.rounds")],
    ));
    out.push((
        "ladder-has-flush",
        s(
            "ladder: [single, pair",
            "ladder: [single, straight-flush-5, pair",
        ),
        vec![(ErrorCode::RsSchemaInvalid, "compare.ladder")],
    ));
    out.push((
        "teams-empty",
        s("teams: [[0,2],[1,3]]", "teams: []"),
        vec![(ErrorCode::RsSchemaInvalid, "seats.teams")],
    ));
    out.push((
        "teams-ragged",
        s("teams: [[0,2],[1,3]]", "teams: [[0,2],[1,3,4]]"),
        vec![(ErrorCode::RsSchemaInvalid, "seats.teams")],
    ));
    out.push((
        "upgrade-duplicate-ranks",
        s(
            "  upgrade: [ { teamRanks: [1,2], step: 3 },",
            "  upgrade: [ { teamRanks: [1,2], step: 3 },\n             { teamRanks: [1,2], step: 2 },",
        ),
        vec![(ErrorCode::RsSchemaInvalid, "levelTable.upgrade")],
    ));
    out.push((
        "upgrade-step-out-of-range",
        s(
            "{ teamRanks: [1,2], step: 3 }",
            "{ teamRanks: [1,2], step: 9 }",
        ),
        vec![(ErrorCode::RsSchemaInvalid, "levelTable.upgrade[0].step")],
    ));
    out.push((
        "upgrade-missing-rows",
        {
            let s = drop_lines(&base, "{ teamRanks:");
            drop_lines(&s, "upgrade: [ ")
        },
        vec![(ErrorCode::RsSchemaInvalid, "levelTable.upgrade")],
    ));
    out.push((
        "deck-jokers-3",
        s("jokers: 2", "jokers: 3"),
        vec![(ErrorCode::RsSchemaInvalid, "deck.jokers")],
    ));
    out.push((
        "deal-per-seat-zero",
        s("perSeat: 27", "perSeat: 0"),
        vec![(ErrorCode::RsSchemaInvalid, "flow.deal.perSeat")],
    ));
    out.push((
        "deal-indivisible-without-per-seat",
        {
            let s = s("count: 4, teams", "count: 5, teams");
            // 只去掉 perSeat 键，保留 deal 段（整行删掉会让 deal=None，闸门根本不进）。
            s.replace("perSeat: 27, ", "")
        },
        vec![(ErrorCode::RsSchemaInvalid, "flow.deal.perSeat")],
    ));
    out.push((
        "rng-stream-out-of-range",
        s("rng:    { stream: 0 }", "rng:    { stream: 5 }"),
        vec![(ErrorCode::RsSchemaInvalid, "flow.rng.stream")],
    ));
    out.push((
        "assert-min-players-zero",
        s("minPlayers: 4", "minPlayers: 0"),
        vec![(ErrorCode::RsSchemaInvalid, "flow.assert.minPlayers")],
    ));
    out.push((
        "lifecycle-out-of-order",
        s(
            "lifecycle: [deal, tribute, play, settle]",
            "lifecycle: [deal, play, tribute, settle]",
        ),
        vec![(ErrorCode::RsSchemaInvalid, "lifecycle")],
    ));
    out.push((
        "tribute-missing-return",
        d("return: { ceiling:"),
        vec![(ErrorCode::RsSchemaInvalid, "tribute.return")],
    ));
    out.push((
        "tribute-missing-trigger",
        d("trigger: last-round-rank"),
        vec![(ErrorCode::RsSchemaInvalid, "tribute.trigger")],
    ));
    out.push((
        "tribute-empty-pairs",
        {
            let s = drop_lines(&base, "- { payer:");
            swap(&s, "  pairs:\n", "  pairs: []\n")
        },
        vec![(ErrorCode::RsSchemaInvalid, "tribute.pairs")],
    ));
    out.push((
        "tribute-missing-resist",
        d("resist: two-big-jokers"),
        vec![(ErrorCode::RsSchemaInvalid, "tribute.resist")],
    ));
    out.push((
        "tribute-payer-zero",
        s(
            "{ payer: 4, receiver: 1, when: single-loss }",
            "{ payer: 0, receiver: 1, when: single-loss }",
        ),
        vec![(ErrorCode::RsSchemaInvalid, "tribute.pairs[0].payer")],
    ));
    out.push((
        "tribute-receiver-out-of-range",
        s(
            "{ payer: 4, receiver: 1, when: single-loss }",
            "{ payer: 4, receiver: 9, when: single-loss }",
        ),
        vec![(ErrorCode::RsSchemaInvalid, "tribute.pairs[0].receiver")],
    ));
    out.push((
        "bomb-count-below-four",
        replace_line(
            &base,
            "id: bomb-4,",
            "  - { id: bomb-4, primitiveId: pattern.bomb, params: { count: 3 } }",
        ),
        vec![(ErrorCode::RsSchemaInvalid, "patterns[7].params.count")],
    ));
    out.push((
        "straight-length-too-short",
        replace_line(
            &base,
            "id: straight-5,",
            "  - { id: straight-5, primitiveId: pattern.straight, params: { length: 4 } }",
        ),
        vec![(ErrorCode::RsSchemaInvalid, "patterns[4].params.length")],
    ));
    out.push((
        "duplicate-normalized-shape",
        swap(
            &base,
            "  - { id: triple-pair,",
            "  - { id: pair-dup, primitiveId: pattern.same-rank, params: { count: 2 } }\n  - { id: triple-pair,",
        ),
        vec![(ErrorCode::RsSchemaInvalid, "patterns[3].params")],
    ));
    out.push((
        "legalfrom-turn",
        replace_line(
            &base,
            "legalFrom: play,",
            "  - { kind: pass, primitive: action.pass, legalFrom: turn, params: {} }",
        ),
        vec![(ErrorCode::RsSchemaInvalid, "actions[1].legalFrom")],
    ));
    out.push((
        "flush-bomb-order-conflict",
        s(
            "ladder: [single, pair, triple, triple-pair, straight-5, plate-3, steel-2, bomb-4, bomb-5, bomb-6",
            "ladder: [single, pair, triple, triple-pair, straight-5, plate-3, steel-2, bomb-6, bomb-4, bomb-5",
        ),
        vec![(ErrorCode::RsLadderConflict, "compare.ladder")],
    ));

    out
}

#[test]
fn invalid_rulesets_each_trip_the_matching_l1_gate() {
    let mut hit = 0usize;
    for (name, src, expects) in cases() {
        let body = from_yaml(&src).unwrap_or_else(|e| {
            panic!("变体 {name} 应在解析期通过、由 L1 拦截，实际解析失败：{e:?}")
        });
        let result = validate(&body, &[Level::L1]);
        for (code, loc) in &expects {
            assert!(
                result
                    .findings
                    .iter()
                    .any(|f| f.code == *code && f.location.starts_with(*loc)),
                "变体 {name} 期望 {:?} @ {}，实际 findings：{:#?}",
                code,
                loc,
                result.findings
            );
            hit += 1;
        }
    }
    assert!(hit >= 25, "命中的闸门数量不足：{hit}");
}

/// 合法边界值：证明闸门只在越界时拦，边界值本身必须放行（否则闸门过紧）。
#[test]
fn legal_boundary_configs_pass_l1() {
    let base = classic_src();
    let s = |from: &str, to: &str| swap(&base, from, to);
    let cases: Vec<(&str, String)> = vec![
        (
            "timing-min",
            s(
                "matchCap: { rounds: 12 }",
                "timing: { turnTimeSec: 5, graceSec: 30, offlineAllSec: 60 }\nmatchCap: { rounds: 12 }",
            ),
        ),
        (
            "timing-max",
            s(
                "matchCap: { rounds: 12 }",
                "timing: { turnTimeSec: 120, graceSec: 180, offlineAllSec: 600 }\nmatchCap: { rounds: 12 }",
            ),
        ),
        ("matchcap-1", s("matchCap: { rounds: 12 }", "matchCap: { rounds: 1 }")),
        ("matchcap-99", s("matchCap: { rounds: 12 }", "matchCap: { rounds: 99 }")),
        ("rng-stream-3", s("rng:    { stream: 0 }", "rng:    { stream: 3 }")),
        ("perseat-1", s("perSeat: 27, reveal", "perSeat: 1, reveal")),
        ("perseat-108", s("perSeat: 27, reveal", "perSeat: 108, reveal")),
    ];

    for (name, src) in cases {
        let body = from_yaml(&src).unwrap_or_else(|e| panic!("{name} 解析失败：{e:?}"));
        let result = validate(&body, &[Level::L1]);
        assert!(
            result.findings.is_empty(),
            "{name} 应通过 L1，实际 findings：{:#?}",
            result.findings
        );
        assert!(result.ran_levels.contains(&Level::L1), "{name} 应记入 L1");
    }
}

/// 级别派发：只请求 L2 时不跑 L1；L1 有错时 L2/L3 的深跑必须短路。
#[test]
fn level_dispatch_respects_ran_levels_and_short_circuit() {
    let ok = from_yaml(&classic_src()).expect("夹具解析失败");
    let only_l2 = validate(&ok, &[Level::L2]);
    assert!(
        !only_l2.ran_levels.contains(&Level::L1),
        "未请求 L1 时不得记入 ran_levels：{:?}",
        only_l2.ran_levels
    );

    let both = validate(&ok, &[Level::L1, Level::L2]);
    assert!(both.ran_levels.contains(&Level::L1));
    assert!(
        !both.findings.iter().any(|f| f.level == Level::L3),
        "未请求 L3 时不应出现 L3 finding"
    );

    let bad = from_yaml(&swap(&classic_src(), "count: 4, teams", "count: 1, teams"))
        .expect("坏体解析失败");
    let deep = validate(&bad, &[Level::L1, Level::L2, Level::L3]);
    assert!(
        !deep.findings.iter().any(|f| f.level == Level::L3),
        "L1 有错时 L3 必须跳过：{:#?}",
        deep.findings
    );
}
