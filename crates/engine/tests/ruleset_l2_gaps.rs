//! L2 / L3 / L4 的「单点破坏」用例：在完整 `guandan-classic` 上改一个字段或
//! 换一条黄金用例，断言对应级别抓到、且不记入 `ran_levels`。
//!
//! 与 `ruleset_l2_l4.rs` 的分工：那边每个级别一正一负；这里把 L2 的引用 /
//! 覆盖 / 容量分支与 L4 的三类不匹配（绑定不符 / 重放失败 / hash 不符）铺满。

use gf_engine::contract::{Action, ErrorCode, Level, Validation, ZoneId};
use gf_engine::ruleset::model::{ActionKind, LegalFrom, RuleSetBody};
use gf_engine::ruleset::parse::from_yaml;
use gf_engine::ruleset::validate_deep::{
    builtin_golden, validate_l2, validate_l3, validate_l4, ValidateOptions,
};

const CLASSIC: &str = include_str!("fixtures/guandan-classic/ruleset.yaml");

fn classic() -> RuleSetBody {
    from_yaml(CLASSIC).expect("classic 夹具解析失败")
}

fn run_l2(body: &RuleSetBody) -> Validation {
    let mut out = Validation::default();
    validate_l2(body, &mut out);
    out
}

#[test]
fn l2_catches_team_out_of_range_and_overlap() {
    // 越界座位：teams 里出现 >= seats.count 的座位。
    let mut body = classic();
    body.seats.teams = vec![vec![0, 2], vec![1, 3, 9]];
    let out = run_l2(&body);
    assert!(
        out.findings
            .iter()
            .any(|f| f.code == ErrorCode::RsTeamCoverageInvalid && f.message.contains("越界")),
        "应报越界座位：{:#?}",
        out.findings
    );

    // 重叠 + 缺座：座位 0 出现在两队，座位 1 无人覆盖。
    let mut body = classic();
    body.seats.teams = vec![vec![0, 2], vec![0, 3]];
    let out = run_l2(&body);
    assert!(
        out.findings
            .iter()
            .any(|f| f.code == ErrorCode::RsTeamCoverageInvalid && f.message.contains("缺座")),
        "应报重叠/缺座：{:#?}",
        out.findings
    );
    assert!(!out.ran_levels.contains(&Level::L2));
}

#[test]
fn l2_catches_dangling_references_and_missing_entry() {
    // legalFrom 指向不存在的区。
    let mut body = classic();
    body.actions[0].legal_from = LegalFrom::Zone(ZoneId("ghost".to_owned()));
    let out = run_l2(&body);
    assert!(
        out.findings
            .iter()
            .any(|f| f.code == ErrorCode::RsRefMissing && f.location.contains("legalFrom")),
        "应报 legalFrom 悬空引用：{:#?}",
        out.findings
    );

    // flow.deal.zone 指向不存在的区。
    let mut body = classic();
    if let Some(flow) = body.flow.as_mut() {
        if let Some(deal) = flow.deal.as_mut() {
            deal.zone = Some(ZoneId("ghost".to_owned()));
        }
    }
    let out = run_l2(&body);
    assert!(
        out.findings
            .iter()
            .any(|f| f.code == ErrorCode::RsRefMissing && f.location == "flow.deal.zone"),
        "应报 deal.zone 悬空引用：{:#?}",
        out.findings
    );

    // 没有 play 类动作，但 lifecycle 声明了 play 段。
    let mut body = classic();
    body.actions.retain(|a| a.kind != ActionKind::Play);
    let out = run_l2(&body);
    assert!(
        out.findings
            .iter()
            .any(|f| f.code == ErrorCode::RsPhaseUnimplemented),
        "应报 play 段缺少入口：{:#?}",
        out.findings
    );

    // settle=level-up 却没有级牌表。
    let mut body = classic();
    body.level_table = None;
    let out = run_l2(&body);
    assert!(
        out.findings
            .iter()
            .any(|f| f.code == ErrorCode::RsRefMissing && f.location == "levelTable"),
        "应报 levelTable 缺失：{:#?}",
        out.findings
    );
}

#[test]
fn l2_catches_deck_count_and_capacity_mismatch() {
    // 声明张数与公式不符。
    let mut body = classic();
    if let Some(flow) = body.flow.as_mut() {
        if let Some(assert) = flow.assert.as_mut() {
            assert.card_count = Some(107);
        }
    }
    let out = run_l2(&body);
    assert!(
        out.findings
            .iter()
            .any(|f| f.code == ErrorCode::RsDeckCountMismatch
                && f.location == "flow.assert.cardCount"),
        "应报 cardCount 不符：{:#?}",
        out.findings
    );

    // 手牌区容量不够整桌。
    let mut body = classic();
    body.zones[0].capacity = 10;
    let out = run_l2(&body);
    assert!(
        out.findings
            .iter()
            .any(|f| f.code == ErrorCode::RsDeckCountMismatch && f.location == "zones"),
        "应报容量不足：{:#?}",
        out.findings
    );
}

#[test]
fn l3_flags_stalled_round_as_no_termination() {
    let body = classic();
    let opts = ValidateOptions {
        l3_rounds: 1,
        l3_candidate_cap: 1, // 第一步候选数必然超过 1 → 判「无法终局」
        ..ValidateOptions::default()
    };
    let mut out = Validation::default();
    let stats = validate_l3(&body, &opts, &mut out);
    assert!(
        out.findings
            .iter()
            .any(|f| f.code == ErrorCode::RsNoTermination),
        "应报 RS_NO_TERMINATION：{:#?}",
        out.findings
    );
    assert!(!out.ran_levels.contains(&Level::L3));
    assert!(stats.rounds_run < opts.l3_rounds);
}

#[test]
fn l3_flags_deal_failure_as_illegal_action_rate() {
    // 座位数与牌堆不匹配 → deal 失败，L3 的失败消息里不含「超过」，走另一条错误码。
    let mut body = classic();
    body.seats.count = 5;
    if let Some(flow) = body.flow.as_mut() {
        if let Some(deal) = flow.deal.as_mut() {
            deal.per_seat = None;
        }
    }
    let opts = ValidateOptions {
        l3_rounds: 1,
        ..ValidateOptions::default()
    };
    let mut out = Validation::default();
    validate_l3(&body, &opts, &mut out);
    assert!(
        out.findings
            .iter()
            .any(|f| f.code == ErrorCode::RsIllegalActionRate),
        "应报 RS_ILLEGAL_ACTION_RATE：{:#?}",
        out.findings
    );
}

#[test]
fn l4_reports_foreign_tampered_and_hash_mismatch() {
    let body = classic();
    let golden = builtin_golden();
    let base = golden[0].clone();

    let mut foreign = base.clone();
    foreign.ruleset_id = "not-guandan-classic".to_owned();

    let mut tampered = base.clone();
    tampered.actions.insert(0, Action::Pass); // 领出态过牌非法 → 重放第 0 步就失败

    let mut wrong_hash = base.clone();
    wrong_hash.expected_hash = [0u8; 32];

    let mut out = Validation::default();
    validate_l4(&body, &[foreign, tampered, wrong_hash], &mut out);
    let msg: String = out
        .findings
        .iter()
        .filter(|f| f.code == ErrorCode::RsL4Mismatch)
        .map(|f| f.message.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(msg.contains("不符"), "应报绑定规则集不符：{msg}");
    assert!(msg.contains("重放失败"), "应报重放失败：{msg}");
    assert!(msg.contains("stateHash 不一致"), "应报 hash 不一致：{msg}");
    assert!(!out.ran_levels.contains(&Level::L4));

    // 原样用例必须通过并记入 L4。
    let mut ok = Validation::default();
    validate_l4(&body, &golden, &mut ok);
    assert!(
        ok.ran_levels.contains(&Level::L4),
        "黄金用例应通过：{:#?}",
        ok.findings
    );
}
