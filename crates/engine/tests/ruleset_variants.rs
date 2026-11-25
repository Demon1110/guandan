//! 规则集变体矩阵：在 `guandan-classic` 上做结构性替换（去掉百搭、改级牌插入位、
//! 改牌堆规模 / 座位数 / 同花顺位次 / 贡牌开关），每个变体都走「发牌 → 逐座位
//! 枚举合法动作 → 实际出一手」。
//!
//! 目的是把这些退化路径都跑一遍：无百搭规则下的识别、级牌不在 naturalOrder 时的
//! 合成序、`copies` / `suits` / `jokers` 变化后的牌 id 几何、`flushRank` 三档位次。
//! 断言只看「不 panic + 每变体至少出一手」，具体牌型仍由 `patterns.rs` 全表负责。

use gf_engine::cases::M0_SEED;
use gf_engine::contract::{Action, Engine};
use gf_engine::engine::M0Engine;
use gf_engine::ruleset::from_yaml;
use std::fs;
use std::path::PathBuf;

fn classic_src() -> String {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/guandan-classic/ruleset.yaml");
    fs::read_to_string(p).expect("夹具读取失败")
}

/// 删除以 `prefix` 开头的整行（用于整段开关，如 `wildcard:`）。
fn drop_line(src: &str, prefix: &str) -> String {
    src.lines()
        .filter(|l| !l.trim_start().starts_with(prefix))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 按多个行首前缀批量删行（整段缺席用）。
fn drop_lines(src: &str, needles: &[&str]) -> String {
    src.lines()
        .filter(|l| !needles.iter().any(|n| l.trim_start().starts_with(n)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 替换一段文本（找不到替换目标直接 panic，避免静默变成 baseline）。
fn swap(src: &str, from: &str, to: &str, tag: &str) -> String {
    assert!(src.contains(from), "变体 {tag} 的替换目标 {from:?} 不存在");
    src.replace(from, to)
}

fn variants() -> Vec<(&'static str, String)> {
    let base = classic_src();
    let mut out: Vec<(&'static str, String)> = Vec::new();

    out.push(("classic", base.clone()));

    // —— 百搭相关 ——
    out.push(("no-wildcard", drop_line(&base, "wildcard:")));
    out.push((
        "wildcard-empty-substitute",
        swap(
            &base,
            r#"substitute: ["2","3","4","5","6","7","8","9","10","J","Q","K","A"]"#,
            "substitute: []",
            "wildcard-empty-substitute",
        ),
    ));
    out.push((
        "wildcard-no-flush",
        swap(
            &base,
            "inStraightFlush: true",
            "inStraightFlush: false",
            "wildcard-no-flush",
        ),
    ));
    // 百搭源点数写成具体牌面（不再是 @level 占位）。
    out.push((
        "wildcard-fixed-rank",
        swap(
            &base,
            "rank: \"@level\"",
            "rank: \"2\"",
            "wildcard-fixed-rank",
        ),
    ));
    // —— 级牌插入位 / 无级牌表 ——
    out.push((
        "level-insert-below-2",
        swap(
            &base,
            "levelInsert: above-A",
            "levelInsert: below-2",
            "level-insert-below-2",
        ),
    ));

    // —— 牌堆规模 ——
    // 2 花色 2 副：13*2+2 = 28 ×2 = 56 张，每座 14。
    out.push((
        "two-suits",
        {
            let s = swap(
                &base,
                "ranks: [\"2\",\"3\",\"4\",\"5\",\"6\",\"7\",\"8\",\"9\",\"10\",\"J\",\"Q\",\"K\",\"A\"], suits: [S,H,D,C], jokers: 2",
                "ranks: [\"2\",\"3\",\"4\",\"5\",\"6\",\"7\",\"8\",\"9\",\"10\",\"J\",\"Q\",\"K\",\"A\"], suits: [S,H], jokers: 2",
                "two-suits",
            );
            let s = swap(&s, "perSeat: 27", "perSeat: 14", "two-suits");
            swap(&s, "cardCount: 108", "cardCount: 56", "two-suits")
        },
    ));
    // 无王：13*4 = 52 ×2 = 104 张，每座 26。
    out.push(("no-jokers", {
        let s = swap(&base, "jokers: 2", "jokers: 0", "no-jokers");
        let s = swap(&s, "perSeat: 27", "perSeat: 26", "no-jokers");
        swap(&s, "cardCount: 108", "cardCount: 104", "no-jokers")
    }));
    // 一档点数缺失：12 点 ×4 + 2 王 = 50 ×2 = 100 张，每座 25；级牌表同步改到 3。
    out.push((
        "ranks-without-2",
        {
            let s = swap(
                &base,
                "ranks: [\"2\",\"3\",\"4\",\"5\",\"6\",\"7\",\"8\",\"9\",\"10\",\"J\",\"Q\",\"K\",\"A\"], suits",
                "ranks: [\"3\",\"4\",\"5\",\"6\",\"7\",\"8\",\"9\",\"10\",\"J\",\"Q\",\"K\",\"A\"], suits",
                "ranks-without-2",
            );
            let s = swap(&s, "initial: \"2\"", "initial: \"3\"", "ranks-without-2");
            let s = swap(
                &s,
                "ranks: [\"2\",\"3\",\"4\",\"5\",\"6\",\"7\",\"8\",\"9\",\"10\",\"J\",\"Q\",\"K\",\"A\"]",
                "ranks: [\"3\",\"4\",\"5\",\"6\",\"7\",\"8\",\"9\",\"10\",\"J\",\"Q\",\"K\",\"A\"]",
                "ranks-without-2",
            );
            let s = swap(&s, "perSeat: 27", "perSeat: 25", "ranks-without-2");
            swap(&s, "cardCount: 108", "cardCount: 100", "ranks-without-2")
        },
    ));
    // 两座位、1 副：54 张 / 2 = 27，正好占满 hand 容量。
    out.push(("two-seats-single-deck", {
        let s = swap(
            &base,
            "seats: { count: 4, teams: [[0,2],[1,3]] }",
            "seats: { count: 2, teams: [[0],[1]] }",
            "two-seats-single-deck",
        );
        let s = swap(&s, "copies: 2", "copies: 1", "two-seats-single-deck");
        swap(
            &s,
            "cardCount: 108",
            "cardCount: 54",
            "two-seats-single-deck",
        )
    }));

    // —— 同花顺位次三档 / 炸弹序 ——
    out.push((
        "flush-below-bomb-5",
        swap(
            &base,
            "flushRank: above-bomb-5",
            "flushRank: below-bomb-5",
            "flush-below-bomb-5",
        ),
    ));
    out.push((
        "flush-above-bomb-6",
        swap(
            &base,
            "flushRank: above-bomb-5",
            "flushRank: above-bomb-6",
            "flush-above-bomb-6",
        ),
    ));
    out.push((
        "flush-above-bomb-8",
        swap(
            &base,
            "flushRank: above-bomb-5",
            "flushRank: above-bomb-8",
            "flush-above-bomb-8",
        ),
    ));

    // —— 贡牌开关 / 整段缺席 ——
    out.push((
        "tribute-disabled",
        swap(&base, "enabled: true", "enabled: false", "tribute-disabled"),
    ));
    out.push((
        "no-tribute-block",
        drop_lines(
            &base,
            &[
                "tribute:",
                "enabled: true",
                "trigger:",
                "pairs:",
                "- { payer",
                "return: {",
                "resist:",
            ],
        ),
    ));

    // —— 发牌 / 弃牌区退化 ——
    // 显式去掉 `deal.zone`：发牌落点由引擎按 private 区推断。
    out.push((
        "deal-without-zone",
        swap(
            &base,
            "zone: hand, perSeat: 27",
            "perSeat: 27",
            "deal-without-zone",
        ),
    ));
    // 没有 discard 区：出牌只落到手牌，不得 panic。
    out.push(("no-discard-zone", drop_lines(&base, &["- { id: discard"])));

    out
}

#[test]
fn ruleset_variants_deal_and_play_one_hand_per_seat() {
    let mut checked = 0usize;
    for (name, src) in variants() {
        let body = from_yaml(&src).unwrap_or_else(|e| panic!("变体 {name} 解析失败：{e:?}"));
        let engine = M0Engine::new(body);
        let state = engine
            .deal(M0_SEED)
            .unwrap_or_else(|e| panic!("变体 {name} 发牌失败：{e:?}"));
        let seats = engine.ruleset.seats.count;
        let mut plays = 0usize;

        // 第二局：走 `deal_next_round`（贡牌 / 抗贡 / 级牌推进的退化路径都在这条链上）。
        if let Ok(round2) = engine.deal_next_round(&state, M0_SEED ^ 1) {
            let mut s = round2;
            s.turn_seat = 0;
            if let Ok(actions) = engine.legal_actions(&s) {
                if let Some(Action::Play { cards }) = actions
                    .into_iter()
                    .find(|a| matches!(a, Action::Play { .. }))
                {
                    engine
                        .apply(s, Action::Play { cards })
                        .unwrap_or_else(|e| panic!("变体 {name} 第二局出牌被拒：{e:?}"));
                    plays += 1;
                }
            }
        }

        for seat in 0..seats {
            let mut s = state.clone();
            s.turn_seat = seat;
            let actions = engine
                .legal_actions(&s)
                .unwrap_or_else(|e| panic!("变体 {name} seat {seat} 枚举候选失败：{e:?}"));
            let Some(Action::Play { cards }) = actions
                .iter()
                .find(|a| matches!(a, Action::Play { .. }))
                .cloned()
            else {
                continue;
            };
            engine
                .apply(s, Action::Play { cards })
                .unwrap_or_else(|e| panic!("变体 {name} seat {seat} 出牌被拒：{e:?}"));
            plays += 1;
        }

        assert!(
            plays >= 1,
            "变体 {name} 至少应有一手可出（座位 {seats}，实际 {plays}）"
        );
        checked += 1;
    }
    assert!(checked >= 12, "变体数量不足：{checked}");
}
