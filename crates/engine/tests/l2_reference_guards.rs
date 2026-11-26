//! L2 引用完整性 / 静态牌堆的负面用例（纯补测，不改实现）。
//!
//! 全部用例都是「结构合法、语义越界」的规则集：`from_yaml` 能构造出来，
//! 由 `validate_l2` 报 `RS_TEAM_COVERAGE_INVALID` / `RS_REF_MISSING` /
//! `RS_PHASE_UNIMPLEMENTED` / `RS_DECK_COUNT_MISMATCH`。
//! 另附 `parse_golden` 的坏夹具用例（hash 长度、缺 ruleset）。

use gf_engine::contract::{ErrorCode, Validation};
use gf_engine::ruleset::parse::from_yaml;
use gf_engine::ruleset::validate_deep::{parse_golden, validate_l2};

const CLASSIC: &str = include_str!("fixtures/guandan-classic/ruleset.yaml");

fn with(from: &str, to: &str) -> String {
    let src = CLASSIC.replace(from, to);
    assert_ne!(src, CLASSIC, "锚点没命中：{from}");
    src
}

fn l2_of(src: &str, what: &str) -> Validation {
    let body = match from_yaml(src) {
        Ok(b) => b,
        Err(e) => panic!("{what}：替换后应仍能构造，实际 {e:?}"),
    };
    let mut v = Validation::default();
    validate_l2(&body, &mut v);
    v
}

fn codes(v: &Validation) -> Vec<ErrorCode> {
    v.findings.iter().map(|f| f.code).collect()
}

#[test]
fn teams_out_of_range_and_duplicate_and_missing() {
    // 越界座位：count=4 但列了 5。
    let v = l2_of(
        &with("teams: [[0,2],[1,3]]", "teams: [[0,2],[1,5]]"),
        "teams 越界",
    );
    assert!(codes(&v).contains(&ErrorCode::RsTeamCoverageInvalid));
    assert!(
        v.findings.iter().any(|f| f.message.contains("越界")),
        "越界应写进 message：{:?}",
        v.findings
    );

    // 重复座位：0 出现在两队。
    let v = l2_of(
        &with("teams: [[0,2],[1,3]]", "teams: [[0,0],[1,2]]"),
        "teams 重复",
    );
    assert!(codes(&v).contains(&ErrorCode::RsTeamCoverageInvalid));

    // 缺座：只覆盖 0/1，2/3 缺。
    let v = l2_of(
        &with("teams: [[0,2],[1,3]]", "teams: [[0,1],[]]"),
        "teams 缺座",
    );
    assert!(codes(&v).contains(&ErrorCode::RsTeamCoverageInvalid));
}

#[test]
fn ladder_and_zone_references_missing() {
    // ladder 里写一个 patterns 索引里不存在的牌型 id。
    let v = l2_of(
        &with("ladder: [single, pair", "ladder: [ghost-pattern, pair"),
        "ladder 悬空",
    );
    assert!(codes(&v).contains(&ErrorCode::RsRefMissing));
    assert!(
        v.findings.iter().any(|f| f.location == "compare.ladder[0]"),
        "位置应精确到 ladder 下标：{:?}",
        v.findings
    );

    // 动作 legalFrom 指向不存在的区。
    let v = l2_of(
        &with(
            "legalFrom: play,     params: {}",
            "legalFrom: ghost-zone, params: {}",
        ),
        "legalFrom 悬空",
    );
    assert!(codes(&v).contains(&ErrorCode::RsRefMissing));

    // deal.zone 指向不存在的区。
    let v = l2_of(
        &with("deal:   { zone: hand,", "deal:   { zone: ghost-zone,"),
        "deal.zone 悬空",
    );
    assert!(codes(&v).contains(&ErrorCode::RsRefMissing));
    assert!(
        v.findings.iter().any(|f| f.location == "flow.deal.zone"),
        "位置应为 flow.deal.zone：{:?}",
        v.findings
    );
}

#[test]
fn lifecycle_play_without_entry() {
    // play 在 lifecycle 里，但没有 kind=play 的动作。
    let v = l2_of(
        &with(
            "- { kind: play,    primitive: action.require-legal, legalFrom: patterns, params: { force: true, consume: hand } }",
            "- { kind: pass,    primitive: action.pass,         legalFrom: play,     params: {} }",
        ),
        "play 段无入口",
    );
    assert!(codes(&v).contains(&ErrorCode::RsPhaseUnimplemented));
}

#[test]
fn deck_formula_mismatch_and_capacity_overflow() {
    // 声明总张数与公式不符。
    let v = l2_of(
        &with(
            "assert: { minPlayers: 4, cardCount: 108 }",
            "assert: { minPlayers: 4, cardCount: 100 }",
        ),
        "cardCount 不符",
    );
    assert!(codes(&v).contains(&ErrorCode::RsDeckCountMismatch));
    assert!(
        v.findings
            .iter()
            .any(|f| f.location == "flow.assert.cardCount"),
        "位置应为 flow.assert.cardCount：{:?}",
        v.findings
    );

    // 手牌区容量不足：27→20（4 座 ×20 = 80 < 108）。
    let v = l2_of(&with("capacity: 27 }", "capacity: 20 }"), "容量不足");
    assert!(codes(&v).contains(&ErrorCode::RsDeckCountMismatch));
    assert!(
        v.findings.iter().any(|f| f.location == "zones"),
        "位置应为 zones：{:?}",
        v.findings
    );
}

#[test]
fn clean_classic_has_no_l2_findings() {
    let v = l2_of(CLASSIC, "classic 原样");
    assert!(
        v.findings.is_empty(),
        "干净规则集不应有 L2 findings：{:?}",
        v.findings
    );
}

#[test]
fn parse_golden_rejects_broken_fixtures() {
    // hash 长度不是 64。
    let bad_hash = parse_golden(
        "bad-hash",
        "ruleset=guandan-classic\nseed=0000000000000001\nhash=abcd\n0 pass\n",
    );
    assert!(bad_hash.is_err(), "hash 长度不足应报错");
    assert!(bad_hash.unwrap_err().contains("hash 长度"));

    // 缺 ruleset= 行。
    let no_ruleset = parse_golden(
        "no-ruleset",
        "seed=0000000000000001\nhash=0000000000000000000000000000000000000000000000000000000000000000\n0 pass\n",
    );
    assert!(no_ruleset.is_err(), "缺 ruleset 应报错");
    assert!(no_ruleset.unwrap_err().contains("ruleset="));

    // 缺 hash= 行。
    let no_hash = parse_golden("no-hash", "ruleset=guandan-classic\n0 pass\n");
    assert!(no_hash.is_err(), "缺 hash 应报错");
    assert!(no_hash.unwrap_err().contains("hash="));

    // 座位解析失败。
    let bad_seat = parse_golden(
        "bad-seat",
        "ruleset=guandan-classic\nhash=0000000000000000000000000000000000000000000000000000000000000000\nx pass\n",
    );
    assert!(bad_seat.is_err(), "非法座位应报错");

    // 牌 id 解析失败。
    let bad_card = parse_golden(
        "bad-card",
        "ruleset=guandan-classic\nhash=0000000000000000000000000000000000000000000000000000000000000000\n0 3,x\n",
    );
    assert!(bad_card.is_err(), "非法牌 id 应报错");

    // 正常夹具：注释行、诊断 k=v 行都被跳过。
    let ok = parse_golden(
        "ok",
        "# 注释\nruleset=guandan-classic\nseed=0000000000000001\nsteps=3\nhash=0000000000000000000000000000000000000000000000000000000000000000\n0 3,4\n1 pass\n",
    )
    .expect("合法夹具应解析成功");
    assert_eq!(ok.seed, 1);
    assert_eq!(ok.ruleset_id, "guandan-classic");
    assert_eq!(ok.actions.len(), 2);
    assert!(matches!(
        ok.actions[0],
        gf_engine::contract::Action::Play { .. }
    ));
    assert!(matches!(ok.actions[1], gf_engine::contract::Action::Pass));
}
