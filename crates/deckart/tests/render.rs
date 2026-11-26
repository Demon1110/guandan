//! 渲染层验收：54 面全覆盖、级牌/逢人配标记、自包含性。

use std::path::PathBuf;

use gf_deckart::{render_all, render_back, render_face, Face, LevelState, Rank, Suit, Theme};

fn theme_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../themes/defaults")
}

fn theme() -> Theme {
    Theme::load_dir(&theme_dir()).expect("默认主题应能加载")
}

fn level(v: u8) -> Option<LevelState> {
    Some(LevelState::new(Rank::new(v).unwrap()))
}

#[test]
fn renders_all_54_faces_and_back() {
    let theme = theme();
    let faces = render_all(&theme, None).unwrap();
    assert_eq!(faces.len(), 54);

    let mut codes: Vec<String> = faces.iter().map(|f| f.code.clone()).collect();
    codes.sort();
    codes.dedup();
    assert_eq!(codes.len(), 54, "牌面编码必须唯一");

    for f in &faces {
        assert!(f.svg.starts_with("<svg"), "{} 必须以 <svg 开头", f.code);
        assert!(f.svg.trim_end().ends_with("</svg>"), "{} 必须闭合", f.code);
        assert!(f.svg.contains("<g id=\"L1-base\">"), "{} 缺底面层", f.code);
    }

    let back = render_back(&theme).unwrap();
    assert!(back.contains("data-face=\"BACK\""));
    assert!(back.contains("<g id=\"L1-back\">"));
}

#[test]
fn level_flag_only_on_matching_rank() {
    let theme = theme();
    let h8 = Face::normal(Suit::Heart, Rank::new(8).unwrap());
    let s8 = Face::normal(Suit::Spade, Rank::new(8).unwrap());
    let h9 = Face::normal(Suit::Heart, Rank::new(9).unwrap());

    let heart_level = render_face(&theme, h8, level(8)).unwrap();
    assert!(heart_level.contains("data-leveled=\"true\""));
    assert!(
        heart_level.contains("data-wild=\"true\""),
        "红桃级牌必须是逢人配"
    );
    assert!(heart_level.contains("<g id=\"L4-level\">"));
    assert!(heart_level.contains("<g id=\"L4b-wild\""));

    let spade_level = render_face(&theme, s8, level(8)).unwrap();
    assert!(spade_level.contains("data-leveled=\"true\""));
    assert!(
        spade_level.contains("data-wild=\"false\""),
        "非红桃级牌不是逢人配"
    );
    assert!(spade_level.contains("<g id=\"L4-level\">"));
    assert!(!spade_level.contains("<g id=\"L4b-wild\""));

    let not_level = render_face(&theme, h9, level(8)).unwrap();
    assert!(not_level.contains("data-leveled=\"false\""));
    assert!(!not_level.contains("<g id=\"L4-level\">"));
}

#[test]
fn no_level_state_means_no_badge() {
    let theme = theme();
    let h2 = Face::normal(Suit::Heart, Rank::new(2).unwrap());
    let svg = render_face(&theme, h2, None).unwrap();
    assert!(svg.contains("data-level=\"none\""));
    assert!(svg.contains("data-leveled=\"false\""));
    assert!(!svg.contains("<g id=\"L4-level\">"));
}

#[test]
fn jokers_never_switch_on_level() {
    let theme = theme();
    for big in [true, false] {
        let svg = render_face(&theme, Face::Joker { big }, level(2)).unwrap();
        assert!(svg.contains("data-leveled=\"false\""));
        assert!(!svg.contains("<g id=\"L3-rank\">"), "王不应叠点数层");
        assert!(!svg.contains("<g id=\"L4-level\">"), "王永远不是级牌");
    }
}

#[test]
fn output_is_self_contained() {
    let theme = theme();
    let text_on = theme
        .meta()
        .text_layer
        .as_ref()
        .map(|t| t.enabled)
        .unwrap_or(false);
    for f in render_all(&theme, level(8)).unwrap() {
        let occurrences = f.svg.matches("http").count();
        assert_eq!(
            occurrences, 1,
            "{} 只允许 xmlns 里的 http 命名空间，不得引外链",
            f.code
        );
        assert!(!f.svg.contains("<image"), "{} 不得内嵌位图", f.code);
        // 可选文本层：开启时只在级牌上出现 <text>，关闭时必须完全没有
        if text_on {
            let is_level = f.svg.contains("data-leveled=\"true\"");
            assert_eq!(
                f.svg.contains("<text"),
                is_level,
                "{} 的文本层应与级牌标记同步",
                f.code
            );
            assert!(
                !f.svg.contains("font-family=\"\"") && !f.svg.contains("&#"),
                "{} 文本层字段不得为空或转义异常",
                f.code
            );
        } else {
            assert!(!f.svg.contains("<text"), "{} 不得依赖字体渲染", f.code);
        }
        // 无论开关如何，都不得引用外部字体文件
        assert!(
            !f.svg.contains("@font-face") && !f.svg.contains(".ttf") && !f.svg.contains(".woff"),
            "{} 不得外链字体",
            f.code
        );
    }
}

#[test]
fn text_layer_can_be_switched_off() {
    let mut meta = theme().meta().clone();
    meta.text_layer = None;
    let src = theme();
    let assets: std::collections::HashMap<String, String> = meta
        .asset_paths()
        .iter()
        .map(|p| (p.clone(), src.asset(p).to_string()))
        .collect();
    let theme = Theme::from_parts(meta, assets, "test:no-text").unwrap();
    let h8 = Face::normal(Suit::Heart, Rank::new(8).unwrap());
    let svg = render_face(&theme, h8, level(8)).unwrap();
    assert!(svg.contains("<g id=\"L4-level\">"), "徽章仍在");
    assert!(!svg.contains("<text"), "关掉文本层后不应再有 text");
}

#[test]
fn classic_layout_uses_pip_table() {
    let src = theme();
    let mut meta = src.meta().clone();
    meta.layout = gf_deckart::Layout::Classic;
    let assets: std::collections::HashMap<String, String> = meta
        .asset_paths()
        .iter()
        .map(|p| (p.clone(), src.asset(p).to_string()))
        .collect();
    let theme = Theme::from_parts(meta, assets, "test:classic").unwrap();

    // 7：中央 7 个花色 + 四角 4 个花色
    let s7 = Face::normal(Suit::Spade, Rank::new(7).unwrap());
    let svg = render_face(&theme, s7, None).unwrap();
    assert!(svg.contains("data-layout=\"classic\""));
    let suit_layer = &svg[svg.find("<g id=\"L2-suit\">").unwrap()..];
    let suit_layer = &suit_layer[..suit_layer.find("<g id=\"L3-rank\">").unwrap()];
    assert_eq!(
        suit_layer.matches("<g transform=").count(),
        11,
        "L2-suit 应为 7 花阵 + 4 角标"
    );
    // 四角点数
    let rank_layer = &svg[svg.find("<g id=\"L3-rank\">").unwrap()..];
    assert_eq!(rank_layer.matches("<g transform=").count(), 4);

    // A 只出 1 个大符号 + 4 角标
    let a_heart = Face::normal(Suit::Heart, Rank::new(1).unwrap());
    let svg = render_face(&theme, a_heart, None).unwrap();
    let suit_layer = &svg[svg.find("<g id=\"L2-suit\">").unwrap()..];
    let suit_layer = &suit_layer[..suit_layer.find("<g id=\"L3-rank\">").unwrap()];
    assert_eq!(suit_layer.matches("<g transform=").count(), 5);
}

#[test]
fn classic_requires_pip_spec() {
    let src = theme();
    let mut meta = src.meta().clone();
    meta.layout = gf_deckart::Layout::Classic;
    meta.classic = None;
    let assets: std::collections::HashMap<String, String> = meta
        .asset_paths()
        .iter()
        .map(|p| (p.clone(), src.asset(p).to_string()))
        .collect();
    let err = Theme::from_parts(meta, assets, "test:classic-bad").unwrap_err();
    assert_eq!(err.code(), "DK-THEME-INVALID");
}

#[test]
fn rendering_is_deterministic() {
    let theme = theme();
    let face = Face::normal(Suit::Club, Rank::new(13).unwrap());
    let a = render_face(&theme, face, level(3)).unwrap();
    let b = render_face(&theme, face, level(3)).unwrap();
    assert_eq!(a, b, "同样输入必须逐字节一致");
}
