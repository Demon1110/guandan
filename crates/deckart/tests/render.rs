//! 渲染层验收：54 面全覆盖、级牌/逢人配标记、自包含性。

use std::path::PathBuf;

use gf_deckart::{
    render_all, render_back, render_face, BadgeMode, Face, LevelState, Rank, Suit, Theme,
};

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
    let text = theme.meta().text_layer.as_ref();
    let expect_badge = text.map(|t| t.draws_badge()).unwrap_or(true);
    let expect_label = text.map(|t| t.draws_label()).unwrap_or(false);
    for f in render_all(&theme, level(8)).unwrap() {
        let occurrences = f.svg.matches("http").count();
        assert_eq!(
            occurrences, 1,
            "{} 只允许 xmlns 里的 http 命名空间，不得引外链",
            f.code
        );
        assert!(!f.svg.contains("<image"), "{} 不得内嵌位图", f.code);
        let is_level = f.svg.contains("data-leveled=\"true\"");
        assert_eq!(
            f.svg.contains("<g id=\"L4-level\">"),
            is_level && expect_badge,
            "{} 的素材层应与 badge 模式一致",
            f.code
        );
        assert_eq!(
            f.svg.contains("<text"),
            is_level && expect_label,
            "{} 的文本层应与 badge 模式一致",
            f.code
        );
        if expect_label {
            assert!(
                !f.svg.contains("font-family=\"\"") && !f.svg.contains("&#"),
                "{} 文本层字段不得为空或转义异常",
                f.code
            );
        }
        // 无论开关如何，都不得引用外部字体文件
        assert!(
            !f.svg.contains("@font-face") && !f.svg.contains(".ttf") && !f.svg.contains(".woff"),
            "{} 不得外链字体",
            f.code
        );
    }
}

/// 以 defaults 素材派生一个只改 textLayer 的主题变体。
fn variant_label_mode(mode: Option<BadgeMode>) -> Theme {
    let src = theme();
    let mut meta = src.meta().clone();
    match mode {
        None => meta.text_layer = None,
        Some(m) => {
            meta.text_layer.as_mut().unwrap().badge = m;
            meta.text_layer.as_mut().unwrap().enabled = true;
        }
    }
    let assets: std::collections::HashMap<String, String> = meta
        .asset_paths()
        .iter()
        .map(|p| (p.clone(), src.asset(p).to_string()))
        .collect();
    Theme::from_parts(meta, assets, "test:badge-mode").unwrap()
}

#[test]
fn badge_modes_pick_layers() {
    let h8 = Face::normal(Suit::Heart, Rank::new(8).unwrap());
    let h9 = Face::normal(Suit::Heart, Rank::new(9).unwrap());

    // both：素材 + 文字
    let both = render_face(&variant_label_mode(Some(BadgeMode::Both)), h8, level(8)).unwrap();
    assert!(both.contains("<g id=\"L4-level\">"));
    assert!(both.contains("<g id=\"L4t-level-label\">"));
    assert!(
        both.contains("<g id=\"L4b-wild\""),
        "逢人配层不受 badge 模式影响"
    );

    // badge-only：只有素材
    let badge_only = render_face(
        &variant_label_mode(Some(BadgeMode::BadgeOnly)),
        h8,
        level(8),
    )
    .unwrap();
    assert!(badge_only.contains("<g id=\"L4-level\">"));
    assert!(!badge_only.contains("<text"));

    // text-only：只有文字，不读素材层
    let text_only =
        render_face(&variant_label_mode(Some(BadgeMode::TextOnly)), h8, level(8)).unwrap();
    assert!(!text_only.contains("<g id=\"L4-level\">"));
    assert!(text_only.contains("<g id=\"L4t-level-label\">"));
    assert!(text_only.contains("级牌"));

    // 非级牌：三种模式都不出 L4 任何一层
    for mode in [
        None,
        Some(BadgeMode::Both),
        Some(BadgeMode::BadgeOnly),
        Some(BadgeMode::TextOnly),
    ] {
        let svg = render_face(&variant_label_mode(mode), h9, level(8)).unwrap();
        assert!(
            !svg.contains("<g id=\"L4-level\">"),
            "{mode:?} 非级牌不应有素材层"
        );
        assert!(!svg.contains("<text"), "{mode:?} 非级牌不应有文字");
    }
}

#[test]
fn text_only_requires_label() {
    let src = theme();
    let mut meta = src.meta().clone();
    meta.text_layer.as_mut().unwrap().badge = BadgeMode::TextOnly;
    meta.text_layer.as_mut().unwrap().level_label = None;
    let assets: std::collections::HashMap<String, String> = meta
        .asset_paths()
        .iter()
        .map(|p| (p.clone(), src.asset(p).to_string()))
        .collect();
    let err = Theme::from_parts(meta, assets, "test:text-only-bad").unwrap_err();
    assert_eq!(err.code(), "DK-THEME-INVALID");

    // badge-only 时缺 label 合法（文字本就不出场）
    let mut meta = src.meta().clone();
    meta.text_layer.as_mut().unwrap().badge = BadgeMode::BadgeOnly;
    meta.text_layer.as_mut().unwrap().level_label = None;
    let assets: std::collections::HashMap<String, String> = meta
        .asset_paths()
        .iter()
        .map(|p| (p.clone(), src.asset(p).to_string()))
        .collect();
    Theme::from_parts(meta, assets, "test:badge-only-ok").expect("badge-only 缺 label 应合法");
}

#[test]
fn text_layer_can_be_switched_off() {
    let src = theme();
    let h8 = Face::normal(Suit::Heart, Rank::new(8).unwrap());

    let mut meta = src.meta().clone();
    meta.text_layer = None;
    let assets: std::collections::HashMap<String, String> = meta
        .asset_paths()
        .iter()
        .map(|p| (p.clone(), src.asset(p).to_string()))
        .collect();
    let no_text = Theme::from_parts(meta, assets, "test:no-text").unwrap();
    let svg = render_face(&no_text, h8, level(8)).unwrap();
    assert!(svg.contains("<g id=\"L4-level\">"), "徽章仍在");
    assert!(!svg.contains("<text"), "关掉文本层后不应再有 text");

    // 整体关闭时即便 badge=text-only 也退回「只画素材」，不能变成什么都不画
    let mut meta = src.meta().clone();
    let t = meta.text_layer.as_mut().unwrap();
    t.enabled = false;
    t.badge = BadgeMode::TextOnly;
    t.level_label = None;
    let assets: std::collections::HashMap<String, String> = meta
        .asset_paths()
        .iter()
        .map(|p| (p.clone(), src.asset(p).to_string()))
        .collect();
    let off_textonly = Theme::from_parts(meta, assets, "test:off-textonly").unwrap();
    let svg = render_face(&off_textonly, h8, level(8)).unwrap();
    assert!(svg.contains("<g id=\"L4-level\">"), "关闭后应退化为素材层");
    assert!(!svg.contains("<text"));
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
fn preview_defaults_to_four_twos() {
    // defaults/theme.json 显式配了 preview；把它清掉应当落到内置缺省
    let src = theme();
    let mut meta = src.meta().clone();
    meta.preview = None;
    let assets: std::collections::HashMap<String, String> = meta
        .asset_paths()
        .iter()
        .map(|p| (p.clone(), src.asset(p).to_string()))
        .collect();
    let theme = Theme::from_parts(meta, assets, "test:preview-default").unwrap();

    let out = gf_deckart::render_preview(&theme).unwrap();
    let codes: Vec<&str> = out.iter().map(|r| r.code.as_str()).collect();
    assert_eq!(codes, vec!["C2", "S2", "H2", "D2"], "默认四张 2、♣♠♥♦ 顺序");
    for r in &out {
        assert!(
            r.svg.contains("data-leveled=\"true\""),
            "{} 在 levelRank=2 下应为级牌",
            r.code
        );
    }
    let h2 = out.iter().find(|r| r.code == "H2").unwrap();
    assert!(h2.svg.contains("<g id=\"L4b-wild\""), "红桃 2 应是逢人配");
    let s2 = out.iter().find(|r| r.code == "S2").unwrap();
    assert!(!s2.svg.contains("<g id=\"L4b-wild\""), "黑桃 2 不是逢人配");
}

#[test]
fn preview_follows_theme_config() {
    let src = theme();
    let mut meta = src.meta().clone();
    meta.preview = Some(gf_deckart::PreviewSpec {
        faces: vec!["H8".into(), "S10".into()],
        level_rank: Some(8),
    });
    let assets: std::collections::HashMap<String, String> = meta
        .asset_paths()
        .iter()
        .map(|p| (p.clone(), src.asset(p).to_string()))
        .collect();
    let cfg_theme = Theme::from_parts(meta, assets, "test:preview-cfg").unwrap();

    let out = gf_deckart::render_preview(&cfg_theme).unwrap();
    assert_eq!(out.len(), 2);
    assert!(
        out[0].svg.contains("data-leveled=\"true\""),
        "H8 应命中级牌"
    );
    assert!(
        out[1].svg.contains("data-leveled=\"false\""),
        "S10 不应是级牌"
    );

    // levelRank = null → 纯牌面
    let src = theme();
    let mut meta = src.meta().clone();
    meta.preview = Some(gf_deckart::PreviewSpec {
        faces: vec!["H8".into()],
        level_rank: None,
    });
    let assets: std::collections::HashMap<String, String> = meta
        .asset_paths()
        .iter()
        .map(|p| (p.clone(), src.asset(p).to_string()))
        .collect();
    let plain = Theme::from_parts(meta, assets, "test:preview-plain").unwrap();
    let out = gf_deckart::render_preview(&plain).unwrap();
    assert!(out[0].svg.contains("data-level=\"none\""));
    assert!(!out[0].svg.contains("<g id=\"L4-level\">"));
}

#[test]
fn preview_rejects_bad_config() {
    let src = theme();
    for bad in [
        gf_deckart::PreviewSpec {
            faces: vec!["ZZ".into()],
            level_rank: Some(2),
        },
        gf_deckart::PreviewSpec {
            faces: vec![],
            level_rank: Some(2),
        },
        gf_deckart::PreviewSpec {
            faces: vec!["C2".into()],
            level_rank: Some(14),
        },
    ] {
        let mut meta = src.meta().clone();
        meta.preview = Some(bad);
        let assets: std::collections::HashMap<String, String> = meta
            .asset_paths()
            .iter()
            .map(|p| (p.clone(), src.asset(p).to_string()))
            .collect();
        let err = Theme::from_parts(meta, assets, "test:preview-bad").unwrap_err();
        assert_eq!(err.code(), "DK-THEME-INVALID");
    }
}

#[test]
fn rendering_is_deterministic() {
    let theme = theme();
    let face = Face::normal(Suit::Club, Rank::new(13).unwrap());
    let a = render_face(&theme, face, level(3)).unwrap();
    let b = render_face(&theme, face, level(3)).unwrap();
    assert_eq!(a, b, "同样输入必须逐字节一致");
}
