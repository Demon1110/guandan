//! 载入期合成阶梯断言（§5.1 第 389 行、§5.13 第 878 行）。
//!
//! `synthetic_ladder` 返回把 `straight-flush-*` 按 `flushRank` 插入后的完整牌力阶梯；
//! 四值必须各自合成不同阶梯，不允许写死单一结果。

use gf_engine::ruleset::ladder::synthetic_ladder;
use gf_engine::ruleset::{from_yaml, RuleSetBody};
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

fn classic_body() -> RuleSetBody {
    let src = read_fixture("guandan-classic/ruleset.yaml");
    from_yaml(&src).expect("guandan-classic 应构造成功")
}

fn ladder_ids(body: &RuleSetBody) -> Vec<String> {
    synthetic_ladder(body).iter().map(|p| p.0.clone()).collect()
}

#[test]
fn classic_synthetic_ladder_matches_expected() {
    let body = classic_body();
    assert_eq!(
        ladder_ids(&body),
        vec![
            "single",
            "pair",
            "triple",
            "straight-5",
            "plate-3",
            "steel-2",
            "bomb-4",
            "bomb-5",
            "straight-flush-5",
            "bomb-6",
            "bomb-7",
            "bomb-8",
            "joker-bomb"
        ]
    );
}

#[test]
fn classic_compare_order_per_878() {
    // §5.13 第 878 行压制阶梯：
    // bomb-4 < bomb-5 < straight-flush-5 < bomb-6 < bomb-7 < bomb-8 < joker-bomb。
    let body = classic_body();
    let ladder = synthetic_ladder(&body);
    let idx = |id: &str| {
        ladder
            .iter()
            .position(|p| p.0 == id)
            .unwrap_or_else(|| panic!("阶梯中缺 `{id}`"))
    };
    assert!(idx("bomb-4") < idx("bomb-5"));
    assert!(idx("bomb-5") < idx("straight-flush-5"));
    assert!(idx("straight-flush-5") < idx("bomb-6"));
    assert!(idx("bomb-6") < idx("bomb-7"));
    assert!(idx("bomb-7") < idx("bomb-8"));
    assert!(idx("bomb-8") < idx("joker-bomb"));
}

#[test]
fn below_bomb_5_inserts_before_anchor() {
    // `below-bomb-5`：同花顺落在 bomb-4 与 bomb-5 之间（与 above-bomb-5 位置不同）。
    let src = read_fixture("guandan-classic/ruleset.yaml")
        .replace("flushRank: above-bomb-5", "flushRank: below-bomb-5");
    let body = from_yaml(&src).expect("below-bomb-5 变体应构造成功");
    let ladder = synthetic_ladder(&body);
    let idx = |id: &str| {
        ladder
            .iter()
            .position(|p| p.0 == id)
            .unwrap_or_else(|| panic!("阶梯中缺 `{id}`"))
    };
    assert!(idx("bomb-4") < idx("straight-flush-5"));
    assert!(idx("straight-flush-5") < idx("bomb-5"));
}
