//! 渲染层验收：54 面全覆盖、级牌/逢人配标记、自包含性。

use std::path::PathBuf;

use gf_deckart::{
    render_all, render_back, render_face, BadgeMode, Face, LevelState, Rank, Suit, Theme,
};

fn theme_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("themes/defaults")
}

fn theme() -> Theme {
    Theme::load_dir(&theme_dir()).expect("默认主题应能加载")
}

fn level(v: u8) -> Option<LevelState> {
    Some(LevelState::new(Rank::new(v).unwrap()))
}

/// 复制一份主题目录出来改 `theme.json`，用来验各种配置分支（素材 23 个文件很小）。
fn theme_variant(tag: &str, patch: impl Fn(String) -> String) -> Theme {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("gfdeck-{tag}-{stamp}"));
    copy_dir(&theme_dir(), &dir);
    let json = patch(std::fs::read_to_string(dir.join("theme.json")).unwrap());
    std::fs::write(dir.join("theme.json"), json).unwrap();
    Theme::load_dir(&dir).expect("变体主题应能加载")
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), to).unwrap();
        }
    }
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

// ---- 角标字号倍率 ----

/// 抓某个图层里第一处 `scale(…)` 的数值。
fn layer_scale(svg: &str, layer: &str) -> f64 {
    let start = svg.find(layer).unwrap_or_else(|| panic!("缺图层 {layer}"));
    let tail = &svg[start..];
    let at = tail.find("scale(").expect("图层内应有 scale") + "scale(".len();
    let end = at + tail[at..].find(')').expect("scale 应有右括号");
    tail[at..end].parse().expect("scale 应是数字")
}

/// 抓某个图层最后一次 `translate(x y)` 的 y。
fn layer_translate(svg: &str, layer: &str) -> (f64, f64) {
    let start = svg.find(layer).unwrap_or_else(|| panic!("缺图层 {layer}"));
    let tail = &svg[start..];
    let at = tail.find("translate(").expect("图层内应有 translate") + "translate(".len();
    let end = at + tail[at..].find(')').expect("translate 应有右括号");
    let mut it = tail[at..end].split_whitespace().map(|v| {
        v.parse::<f64>()
            .unwrap_or_else(|_| panic!("translate 应有数字，得到 {v}"))
    });
    (it.next().unwrap(), it.next().unwrap())
}

#[test]
fn corner_scale_one_is_byte_identical_to_plain_render() {
    let theme = theme();
    let face = Face::normal(Suit::Spade, Rank::new(4).unwrap());
    let plain = render_face(&theme, face, level(2)).unwrap();
    let with_one = gf_deckart::render_face_opts(
        &theme,
        face,
        gf_deckart::FaceOptions {
            level: level(2),
            corner_scale: 1.0,
        },
    )
    .unwrap();
    assert_eq!(
        plain, with_one,
        "倍率 1.0 必须与旧入口逐字节一致（向后兼容）"
    );
}

#[test]
fn corner_scale_enlarges_corner_and_reflows_suit() {
    let theme = theme();
    let face = Face::normal(Suit::Spade, Rank::new(4).unwrap());
    let base = render_face(&theme, face, None).unwrap();
    let big = gf_deckart::render_face_opts(
        &theme,
        face,
        gf_deckart::FaceOptions {
            level: None,
            corner_scale: 2.4,
        },
    )
    .unwrap();

    let rank_base = layer_scale(&base, "<g id=\"L3-rank\">");
    let rank_big = layer_scale(&big, "<g id=\"L3-rank\">");
    assert!(
        (rank_big / rank_base - 2.4).abs() < 1e-6,
        "点数应放大 2.4 倍：{rank_base} → {rank_big}"
    );
    let suit_base = layer_scale(&base, "<g id=\"L2-suit\">");
    let suit_big = layer_scale(&big, "<g id=\"L2-suit\">");
    assert!(
        (suit_big / suit_base - 2.4).abs() < 1e-6,
        "角标花色应同步放大：{suit_base} → {suit_big}"
    );

    // 花色要重新排在放大后的点数下方，不能被压住
    let (_, suit_y_base) = layer_translate(&base, "<g id=\"L2-suit\">");
    let (_, suit_y_big) = layer_translate(&big, "<g id=\"L2-suit\">");
    assert!(
        suit_y_big > suit_y_base + 60.0,
        "花色 y 应随大字重排：{suit_y_base} → {suit_y_big}"
    );

    // 底框与中央花阵不跟着放大（这才是「只放大角标」的意思）
    let center_base = base.matches("scale(").count();
    let center_big = big.matches("scale(").count();
    assert_eq!(center_base, center_big, "图层结构不应变化");
    let header = "<g id=\"L1-base\">";
    let base_layer = &base[base.find(header).unwrap()..];
    let base_layer_big = &big[big.find(header).unwrap()..];
    let cut = base_layer.len().min(200);
    assert_eq!(
        &base_layer[..cut],
        &base_layer_big[..cut],
        "底面层必须原样不动"
    );
}

#[test]
fn corner_scale_survives_classic_layout() {
    let src = theme();
    let mut meta = src.meta().clone();
    meta.layout = gf_deckart::Layout::Classic;
    let assets: std::collections::HashMap<String, String> = meta
        .asset_paths()
        .iter()
        .map(|p| (p.clone(), src.asset(p).to_string()))
        .collect();
    let theme = Theme::from_parts(meta, assets, "test:classic-corner").unwrap();

    let face = Face::normal(Suit::Heart, Rank::new(10).unwrap());
    let base = render_face(&theme, face, None).unwrap();
    let big = gf_deckart::render_face_opts(
        &theme,
        face,
        gf_deckart::FaceOptions {
            level: None,
            corner_scale: 2.0,
        },
    )
    .unwrap();
    let rank_base = layer_scale(&base, "<g id=\"L3-rank\">");
    let rank_big = layer_scale(&big, "<g id=\"L3-rank\">");
    assert!(
        (rank_big / rank_base - 2.0).abs() < 1e-6,
        "四角点数应为 2 倍"
    );
}

#[test]
fn theme_declares_default_corner_scale() {
    // 客户端滑块没被手动调过时读它；CLI 不给 --corner 时也读它
    let t = theme();
    assert!(
        t.has_corner_scale(),
        "默认主题应声明 cornerScale 作为默认角标"
    );
    assert!((t.corner_scale() - 2.2).abs() < 1e-9, "默认应声明 220%");
}

#[test]
fn theme_corner_scale_is_optional_and_bounded() {
    let src = std::fs::read_to_string(theme_dir().join("theme.json")).unwrap();
    let mut base: serde_json::Value = serde_json::from_str(&src).unwrap();

    // 省略 → 主题不给意见，按素材原样（1.0）
    let obj = base.as_object_mut().unwrap();
    assert!(
        obj.remove("cornerScale").is_some(),
        "默认主题应声明 cornerScale"
    );
    let meta = gf_deckart::ThemeMeta::parse(&serde_json::to_string(&base).unwrap()).unwrap();
    assert_eq!(meta.corner_scale, None, "cornerScale 可省");

    // 越界 / 负数 / NaN 在加载期就拒（和客户端滑块 100%–300% 同一口径）
    for bad in [3.5_f64, 0.5, -2.0] {
        let mut v = base.clone();
        v["cornerScale"] = serde_json::json!(bad);
        let meta = gf_deckart::ThemeMeta::parse(&serde_json::to_string(&v).unwrap()).unwrap();
        let err = gf_deckart::Theme::from_parts(meta, std::collections::HashMap::new(), "test")
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            matches!(err, gf_deckart::DeckError::ThemeInvalid(_)) && msg.contains("cornerScale"),
            "cornerScale={bad} 应在加载期被拒，实际 {msg}"
        );
    }

    // 边界 1.0 / 3.0 可用（加载期不拦，交给渲染层）
    for ok in [gf_deckart::THEME_CORNER_MIN, gf_deckart::THEME_CORNER_MAX] {
        let mut v = base.clone();
        v["cornerScale"] = serde_json::json!(ok);
        let meta = gf_deckart::ThemeMeta::parse(&serde_json::to_string(&v).unwrap()).unwrap();
        assert_eq!(meta.corner_scale, Some(ok));
    }
}

#[test]
fn corners_config_limits_marks_in_both_layouts() {
    // 默认 compact：只有左上 + 右下（2 个角标 = 2 处 rotate(180)：右下点数与花色）
    let face = Face::normal(Suit::Diamond, Rank::new(10).unwrap());
    let svg = render_face(&theme(), face, None).unwrap();
    assert_eq!(svg.matches("rotate(180)").count(), 2, "compact 默认两角");

    // compact 只留左上：一处 rotate 都不该有
    let only_tl = theme_variant("corners-tl", |json| {
        json.replace(
            "\"cornerScale\": 2.2,",
            "\"cornerScale\": 2.2,\n  \"corners\": [\"topLeft\"],",
        )
    });
    let svg = render_face(&only_tl, face, None).unwrap();
    assert_eq!(
        svg.matches("rotate(180)").count(),
        0,
        "只留左上就没有镜像角标"
    );
    assert!(svg.contains("<g id=\"L3-rank\">"));

    // classic 默认四角：花色 3 处 + 点数 3 处旋转（左上不转）
    let classic = theme_variant("corners-classic", |json| {
        json.replace("\"layout\": \"compact\"", "\"layout\": \"classic\"")
    });
    let svg = render_face(&classic, face, None).unwrap();
    assert_eq!(svg.matches("rotate(180)").count(), 6, "classic 默认四角");

    // classic 只要左上 + 右下：旋转只剩 2 处
    let two = theme_variant("corners-classic-two", |json| {
        json.replace("\"layout\": \"compact\"", "\"layout\": \"classic\"")
            .replace(
                "\"cornerScale\": 2.2,",
                "\"cornerScale\": 2.2,\n  \"corners\": [\"topLeft\", \"bottomRight\"],",
            )
    });
    let svg = render_face(&two, face, None).unwrap();
    assert_eq!(svg.matches("rotate(180)").count(), 2, "classic 收成两角");
    // 花阵不受影响：10 的点位仍在
    assert!(svg.contains("<g id=\"L2-suit\">"));
}

#[test]
fn corners_config_is_validated() {
    let src = std::fs::read_to_string(theme_dir().join("theme.json")).unwrap();
    let base: serde_json::Value = serde_json::from_str(&src).unwrap();

    // 重复 / 超过 4 项 → 加载期报错；未知名字由 serde 直接拒
    for bad in [
        serde_json::json!(["topLeft", "topLeft"]),
        serde_json::json!([
            "topLeft",
            "topRight",
            "bottomLeft",
            "bottomRight",
            "topLeft"
        ]),
        serde_json::json!(["left"]),
    ] {
        let mut v = base.clone();
        v["corners"] = bad.clone();
        let err = match gf_deckart::ThemeMeta::parse(&serde_json::to_string(&v).unwrap()) {
            Ok(meta) => {
                gf_deckart::Theme::from_parts(meta, std::collections::HashMap::new(), "test")
                    .unwrap_err()
            }
            Err(e) => e,
        };
        assert!(
            matches!(err, gf_deckart::DeckError::ThemeInvalid(_)),
            "{bad} 应被拒，实际 {err}"
        );
    }

    // 空数组合法（= 不要任何角标），但要点数/花阵还在
    let mut v = base;
    v["corners"] = serde_json::json!([]);
    let meta = gf_deckart::ThemeMeta::parse(&serde_json::to_string(&v).unwrap()).unwrap();
    assert!(meta.corner_positions().is_empty());
}

#[test]
fn corner_scale_rejects_out_of_range() {
    let theme = theme();
    let face = Face::normal(Suit::Diamond, Rank::new(3).unwrap());
    for bad in [0.1, 0.0, -1.0, 4.0, f64::NAN] {
        let err = gf_deckart::render_face_opts(
            &theme,
            face,
            gf_deckart::FaceOptions {
                level: None,
                corner_scale: bad,
            },
        )
        .unwrap_err();
        assert!(
            matches!(err, gf_deckart::DeckError::FaceInvalid(_)),
            "倍率 {bad} 应被拒"
        );
    }
    // 边界值本身可用
    for ok in [gf_deckart::CORNER_SCALE_MIN, gf_deckart::CORNER_SCALE_MAX] {
        assert!(
            gf_deckart::render_face_opts(
                &theme,
                face,
                gf_deckart::FaceOptions {
                    level: None,
                    corner_scale: ok,
                },
            )
            .is_ok(),
            "边界 {ok} 应可用"
        );
    }
}

/// 抠出 L2-suit 里每一个 `<g transform="translate(x y)…scale(s)">` 的 (x, y, s)。
/// 花阵点位写在最前面，角标花色在最后，调用方按顺序切分。
fn suit_groups(svg: &str) -> Vec<(f64, f64, f64)> {
    let start = svg.find("<g id=\"L2-suit\">").expect("应有 L2-suit");
    let end = svg.find("<g id=\"L3-rank\">").expect("应有 L3-rank");
    let body = &svg[start..end];
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(idx) = rest.find("translate(") {
        rest = &rest[idx + "translate(".len()..];
        let close = rest.find(')').expect("translate 应有右括号");
        let coords: Vec<f64> = rest[..close]
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect();
        assert_eq!(coords.len(), 2, "translate 应是两个坐标");
        let tail = &rest[close..];
        let scale_start = tail.find("scale(").expect("组上应有 scale");
        let scale_end = tail[scale_start..].find(')').expect("scale 应有右括号");
        let s: f64 = tail[scale_start + "scale(".len()..scale_start + scale_end]
            .parse()
            .expect("scale 应是数字");
        out.push((coords[0], coords[1], s));
    }
    out
}

#[test]
fn classic_pips_clear_corner_marks_when_corners_are_huge() {
    for corner in [1.0, 2.4, 3.0] {
        let theme = theme_variant("classic-clear", |json| {
            json.replace("\"layout\": \"compact\"", "\"layout\": \"classic\"")
        });
        let face = Face::normal(Suit::Diamond, Rank::new(10).unwrap());
        let svg = gf_deckart::render_face_opts(
            &theme,
            face,
            gf_deckart::FaceOptions {
                level: None,
                corner_scale: corner,
            },
        )
        .unwrap();
        let groups = suit_groups(&svg);
        // 前 10 个是 D10 的花阵，最后 2 个是左上 / 右下的角标花色
        let pips = &groups[..10];
        let corner_suit = groups[10];

        // 角标点数放大后的实际宽度（素材 100×140，classic cornerRank scale=0.36）
        let rank_scale = 0.36 * corner;
        let rank_right = 34.0 + 100.0 * rank_scale;
        let rank_bottom = 30.0 + 140.0 * rank_scale;
        let inset = 0.10 * 500.0;

        for (i, (x, y, s)) in pips.iter().enumerate() {
            assert!(
                *x >= rank_right - 0.01,
                "{corner}x 第 {i} 个点位左缘 {x} 压到了角标（角标右缘 {rank_right}）"
            );
            assert!(*x + 100.0 * s <= 500.0 - inset + 0.01, "点位右缘越界");
            assert!(*y >= inset * (700.0 / 500.0) - 0.01, "点位顶缘越界");
            assert!(
                *y + 100.0 * s <= 700.0 - inset * (700.0 / 500.0) + 0.01,
                "点位底缘越界"
            );
        }

        // 两列不能粘在一起
        let left_x = pips[0].0 + 100.0 * pips[0].2;
        let right_x = pips[1].0;
        assert!(
            right_x - left_x >= 20.0,
            "{corner}x 左右两列间距过小：{left_x} → {right_x}"
        );

        // 角标花色必须排在放大后的点数下方
        assert!(
            corner_suit.1 >= rank_bottom + 8.0,
            "{corner}x 角标花色 y={} 仍被点数（底 {rank_bottom}）压住",
            corner_suit.1
        );
    }
}

#[test]
fn classic_pip_inset_is_validated() {
    let src = std::fs::read_to_string(theme_dir().join("theme.json")).unwrap();
    let base: serde_json::Value = serde_json::from_str(&src).unwrap();
    for (i, bad) in [0.5, -0.1].iter().enumerate() {
        let mut v = base.clone();
        v["classic"]["pipInset"] = serde_json::json!(bad);
        let dir = std::env::temp_dir().join(format!("gfdeck-pipinset-{i}"));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&theme_dir(), &dir);
        std::fs::write(
            dir.join("theme.json"),
            serde_json::to_string_pretty(&v).unwrap(),
        )
        .unwrap();
        let err = match gf_deckart::Theme::load_dir(&dir) {
            Ok(_) => panic!("pipInset {bad} 应被拒"),
            Err(e) => e,
        };
        assert!(
            matches!(err, gf_deckart::DeckError::ThemeInvalid(_)),
            "pipInset {bad} 应报 ThemeInvalid，实际 {err}"
        );
    }
}

#[test]
fn corner_suit_can_be_placed_right_of_the_rank() {
    let face = Face::normal(Suit::Diamond, Rank::new(10).unwrap());

    // 缺省 = 点数下方：compact 花色沿用素材里的 slot 坐标（x 36，y 112）
    let def = suit_groups(&render_face(&theme(), face, None).unwrap());
    assert_eq!(def[0].0, 36.0, "缺省不重排时花色留在 slot 原坐标");

    // 配成右侧：花色贴到点数右缘之外，纵向与点数居中
    let right = theme_variant("placement-right", |json| {
        json.replace(
            "\"cornerScale\": 2.2,",
            "\"cornerScale\": 2.2,\n  \"cornerSuitPlacement\": \"right\",",
        )
    });
    let svg = gf_deckart::render_face_opts(
        &right,
        face,
        gf_deckart::FaceOptions {
            level: None,
            corner_scale: 2.2,
        },
    )
    .unwrap();
    let groups = suit_groups(&svg);
    let (sx, sy, ss) = groups[0];
    let rank_scale = 0.42 * 2.2;
    let rank_right = 30.0 + 100.0 * rank_scale;
    let rank_top = 34.0;
    let rank_bottom = rank_top + 140.0 * rank_scale;
    assert!(
        sx >= rank_right + 9.0,
        "花色应排在点数右侧：{sx} vs {rank_right}"
    );
    assert!(
        sy > rank_top && sy + 100.0 * ss < rank_bottom,
        "花色应落在点数的高度范围内"
    );
    let suit_center = sy + 50.0 * ss;
    let rank_center = rank_top + 70.0 * rank_scale;
    assert!(
        (suit_center - rank_center).abs() < 1.0,
        "右侧摆放应纵向居中：花色中心 {suit_center} vs 点数中心 {rank_center}"
    );

    // classic：同样配成右侧，花阵要让开花色，不能压上去
    let classic_right = theme_variant("placement-right-classic", |json| {
        json.replace("\"layout\": \"compact\"", "\"layout\": \"classic\"")
            .replace(
                "\"cornerScale\": 2.2,",
                "\"cornerScale\": 2.2,\n  \"cornerSuitPlacement\": \"right\",",
            )
    });
    for corner in [1.0, 1.8, 2.4] {
        let svg = gf_deckart::render_face_opts(
            &classic_right,
            face,
            gf_deckart::FaceOptions {
                level: None,
                corner_scale: corner,
            },
        )
        .unwrap();
        let groups = suit_groups(&svg);
        let pips = &groups[..10];
        let (csx, _, css) = groups[10];
        let suit_right = csx + 100.0 * css;
        let pip_left = pips.iter().map(|p| p.0).fold(f64::MAX, f64::min);
        assert!(
            pip_left >= suit_right,
            "{corner}x 花阵左缘 {pip_left} 压到了右侧花色（右缘 {suit_right}）"
        );
    }
}

#[test]
fn corner_suit_placement_rejects_unknown_value() {
    let src = std::fs::read_to_string(theme_dir().join("theme.json")).unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&src).unwrap();
    for bad in ["left", "above"] {
        v["cornerSuitPlacement"] = serde_json::json!(bad);
        let err = gf_deckart::ThemeMeta::parse(&serde_json::to_string(&v).unwrap())
            .err()
            .unwrap_or_else(|| panic!("{bad} 应被拒"));
        assert!(
            matches!(err, gf_deckart::DeckError::ThemeInvalid(_)),
            "{bad} 应报 ThemeInvalid，实际 {err}"
        );
    }
}
