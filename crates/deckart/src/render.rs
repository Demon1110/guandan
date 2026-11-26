//! 四层合成：L1 底面 → L2 花色 → L3 点数 → L4 级牌/逢人配徽章。
//! 输出单文件、自包含、无外链的 SVG；图层 id 与 `data-*` 属性固定，便于测试与视觉回归。

use std::fmt::Write as _;

use crate::card::{Face, Rank, Suit};
use crate::error::DeckError;
use crate::theme::{Layout, Theme};

/// 本局级牌状态。只认服务端下发的 `levelRank`，客户端不自行推断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelState {
    pub level_rank: Rank,
}

impl LevelState {
    pub fn new(level_rank: Rank) -> LevelState {
        LevelState { level_rank }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderedFace {
    /// 稳定编码，同时是导出文件名主干
    pub code: String,
    pub svg: String,
}

/// 牌面渲染的可选参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceOptions {
    /// `None` 表示本局无级牌状态（不出 L4）
    pub level: Option<LevelState>,
    /// 角标（左上点数 + 角标花色）的尺寸倍率，`1.0` = 按主题原样。
    /// 手牌横排紧叠时只露得出一条牌宽，用它可以只放大角标、不动中央花阵与底框。
    pub corner_scale: f64,
}

impl Default for FaceOptions {
    fn default() -> Self {
        FaceOptions {
            level: None,
            corner_scale: 1.0,
        }
    }
}

/// 角标的允许区间：太小看不清、太大压到中央花阵，两端都不给。
pub const CORNER_SCALE_MIN: f64 = 0.6;
pub const CORNER_SCALE_MAX: f64 = 3.0;

/// 放大角标时，点数与角标花色之间的固定间隙（画布单位）。主题默认间距是按小字号给的，
/// 字号一放大就得重排，否则花色会被点数压住。
const CORNER_GAP: f64 = 10.0;

/// 单个牌面（角标按主题原样）。`level = None` 表示本局无级牌状态（不出 L4）。
pub fn render_face(
    theme: &Theme,
    face: Face,
    level: Option<LevelState>,
) -> Result<String, DeckError> {
    render_face_opts(
        theme,
        face,
        FaceOptions {
            level,
            corner_scale: 1.0,
        },
    )
}

/// 单个牌面，带角标尺寸倍率。
pub fn render_face_opts(
    theme: &Theme,
    face: Face,
    opts: FaceOptions,
) -> Result<String, DeckError> {
    let corner = opts.corner_scale;
    if !corner.is_finite() || !(CORNER_SCALE_MIN..=CORNER_SCALE_MAX).contains(&corner) {
        return Err(DeckError::FaceInvalid(format!(
            "corner_scale 越界（合法 {CORNER_SCALE_MIN}..={CORNER_SCALE_MAX}）"
        )));
    }
    let level = opts.level;
    let meta = theme.meta();
    let mut out = String::with_capacity(4096);
    let w = meta.canvas.width;
    let h = meta.canvas.height;

    let is_level = matches!(face, Face::Normal { rank, .. } if level.map(|l| l.level_rank == rank).unwrap_or(false));
    let is_wild = is_level
        && matches!(
            face,
            Face::Normal {
                suit: Suit::Heart,
                ..
            }
        );

    out.push_str(&header(theme, &face, level, is_level, is_wild));

    // L1 底面
    let base = fragment("base", theme.asset(&meta.layers.base))?;
    out.push_str("<g id=\"L1-base\">");
    out.push_str(base.inner);
    out.push_str("</g>\n");

    match face {
        Face::Joker { big } => {
            let joker_rel = if big {
                &meta.layers.jokers.big
            } else {
                &meta.layers.jokers.small
            };
            let joker = fragment(joker_rel, theme.asset(joker_rel))?;
            out.push_str("<g id=\"L2-joker\">");
            out.push_str(joker.inner);
            out.push_str("</g>\n");
        }
        Face::Normal { suit, rank } => match meta.layout {
            Layout::Compact => render_normal_compact(&mut out, theme, suit, rank, corner)?,
            Layout::Classic => render_normal_classic(&mut out, theme, suit, rank, corner)?,
        },
    }

    // L4 级牌徽章（通用一套，不随点数变）
    // 三选一由 textLayer.badge 决定：both（默认）/ badge-only / text-only
    if is_level {
        let text = meta.text_layer.as_ref();
        let draw_badge = text.map(|t| t.draws_badge()).unwrap_or(true);
        let draw_label = text.map(|t| t.draws_label()).unwrap_or(false);

        if draw_badge {
            let rel = &meta.badges.level;
            let badge = fragment(rel, theme.asset(rel))?;
            out.push_str("<g id=\"L4-level\">");
            out.push_str(badge.inner);
            out.push_str("</g>\n");
        }

        // L4t 文本层：textLayer.enabled + badge 模式共同决定是否出场
        if draw_label {
            if let Some(t) = text {
                if let Some(label) = t.level_label.as_ref() {
                    writeln!(
                        out,
                        "<g id=\"L4t-level-label\"><text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\" text-anchor=\"{}\">{}</text></g>",
                        num(label.x),
                        num(label.y),
                        escape(&t.font_family),
                        num(label.size),
                        label.color,
                        label.anchor,
                        escape(&label.text)
                    )
                    .ok();
                }
            }
        }
    }

    // L4b 逢人配徽章（红桃级牌）
    if is_wild {
        let rel = &meta.badges.wild;
        let badge = fragment(rel, theme.asset(rel))?;
        let slot = meta.slots.wild_badge;
        writeln!(
            out,
            "<g id=\"L4b-wild\" transform=\"translate({} {}) scale({})\">{}</g>",
            num(slot.x),
            num(slot.y),
            num(slot.scale),
            badge.inner
        )
        .ok();
    }

    let _ = (w, h);
    out.push_str("</svg>\n");
    Ok(out)
}

fn render_normal_compact(
    out: &mut String,
    theme: &Theme,
    suit: Suit,
    rank: Rank,
    corner: f64,
) -> Result<(), DeckError> {
    let meta = theme.meta();
    let palette = &meta.palette;
    let mut slot = meta.slots.corner_rank;
    let mut suit_slot = meta.slots.corner_suit;
    let center = meta.slots.center_suit;
    let w = meta.canvas.width;
    let h = meta.canvas.height;

    let suit_rel = meta.layers.suits.get(suit);
    let suit_frag = fragment(suit_rel, theme.asset(suit_rel))?;
    let ink = if suit.is_red() {
        &palette.red
    } else {
        &palette.black
    };
    let suit_inner = recolor(suit_frag.inner, ink);
    let rank_rel = theme.rank_asset(rank)?;
    let rank_frag = fragment(rank_rel, theme.asset(rank_rel))?;
    let rank_inner = recolor(rank_frag.inner, ink);

    // 角标倍率：点数与角标花色一起放大，并把花色重新排在放大后的点数下方，
    // 否则大字号下点数会压到花色上（原布局的间距是按小字号给的）。
    if (corner - 1.0).abs() > f64::EPSILON {
        slot.scale *= corner;
        suit_slot.scale *= corner;
        suit_slot.y = slot.y + rank_frag.height * slot.scale + CORNER_GAP * corner;
    }

    // L2 花色：角标 + 中央 + 右下镜像（角标区）
    writeln!(out, "<g id=\"L2-suit\">").ok();
    writeln!(
        out,
        "<g transform=\"translate({} {}) scale({})\">{}</g>",
        num(suit_slot.x),
        num(suit_slot.y),
        num(suit_slot.scale),
        suit_inner
    )
    .ok();
    writeln!(
        out,
        "<g transform=\"translate({} {}) scale({})\">{}</g>",
        num(center.x),
        num(center.y),
        num(center.scale),
        suit_inner
    )
    .ok();
    writeln!(out, "</g>").ok();

    // L3 点数：左上角标 + 右下镜像
    writeln!(out, "<g id=\"L3-rank\">").ok();
    writeln!(
        out,
        "<g transform=\"translate({} {}) scale({})\">{}</g>",
        num(slot.x),
        num(slot.y),
        num(slot.scale),
        rank_inner
    )
    .ok();
    // 右下镜像：把素材原点平移到 (W-x, H-y) 再旋转 180°
    writeln!(
        out,
        "<g transform=\"translate({} {}) rotate(180) scale({})\">{}</g>",
        num(w - slot.x),
        num(h - slot.y),
        num(slot.scale),
        rank_inner
    )
    .ok();
    writeln!(out, "</g>").ok();

    // 右下镜像的角标花色（与点数一起构成对称角标）
    writeln!(
        out,
        "<g id=\"L2-suit-mirror\" transform=\"translate({} {}) rotate(180) scale({})\">{}</g>",
        num(w - suit_slot.x),
        num(h - suit_slot.y),
        num(suit_slot.scale),
        suit_inner
    )
    .ok();
    Ok(())
}

/// `layout = classic`：四角标（字头朝牌心）+ 中央花阵。
/// 花阵点位由 `theme.json` 的 `classic.pipPositions` 给出（归一化 0..1），同一套表适配任意 canvas。
fn render_normal_classic(
    out: &mut String,
    theme: &Theme,
    suit: Suit,
    rank: Rank,
    corner_scale: f64,
) -> Result<(), DeckError> {
    let meta = theme.meta();
    let spec = meta
        .classic
        .as_ref()
        .ok_or_else(|| DeckError::ThemeInvalid("layout=classic 缺 classic 花阵规格".into()))?;
    let palette = &meta.palette;
    let cw = meta.canvas.width;
    let ch = meta.canvas.height;

    let suit_rel = meta.layers.suits.get(suit);
    let suit_frag = fragment(suit_rel, theme.asset(suit_rel))?;
    let ink = if suit.is_red() {
        &palette.red
    } else {
        &palette.black
    };
    let suit_inner = recolor(suit_frag.inner, ink);
    let rank_rel = theme.rank_asset(rank)?;
    let rank_frag = fragment(rank_rel, theme.asset(rank_rel))?;
    let rank_inner = recolor(rank_frag.inner, ink);

    // 中央花阵 / 大符号
    writeln!(out, "<g id=\"L2-suit\">").ok();
    match rank.value() {
        1 | 11..=13 => {
            let s = spec.ace_scale;
            let tx = (cw - suit_frag.width * s) / 2.0;
            let ty = (ch - suit_frag.height * s) / 2.0;
            writeln!(
                out,
                "<g transform=\"translate({} {}) scale({})\">{}</g>",
                num(tx),
                num(ty),
                num(s),
                suit_inner
            )
            .ok();
        }
        value => {
            let points = spec
                .pip_positions
                .get(rank.label())
                .ok_or_else(|| DeckError::ThemeInvalid(format!("classic 缺 {} 的花阵", value)))?;
            let s = spec.pip_scale;
            for p in points {
                let tx = p[0] * cw - suit_frag.width * s / 2.0;
                let ty = p[1] * ch - suit_frag.height * s / 2.0;
                writeln!(
                    out,
                    "<g transform=\"translate({} {}) scale({})\">{}</g>",
                    num(tx),
                    num(ty),
                    num(s),
                    suit_inner
                )
                .ok();
            }
        }
    }
    // 四角花色
    let mut corner_suit = spec.corner_suit;
    corner_suit.scale *= corner_scale;
    let suit_art = Fragment {
        inner: &suit_inner,
        width: suit_frag.width,
        height: suit_frag.height,
    };
    for corner in [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ] {
        place(out, &suit_art, corner_suit, corner, cw, ch);
    }
    writeln!(out, "</g>").ok();

    // 四角点数
    writeln!(out, "<g id=\"L3-rank\">").ok();
    let mut corner_rank = spec.corner_rank;
    corner_rank.scale *= corner_scale;
    let rank_art = Fragment {
        inner: &rank_inner,
        width: rank_frag.width,
        height: rank_frag.height,
    };
    for corner in [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ] {
        place(out, &rank_art, corner_rank, corner, cw, ch);
    }
    writeln!(out, "</g>").ok();
    Ok(())
}

#[derive(Clone, Copy)]
enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// 四角摆放：除左上外全部旋转 180°，让字头/花尖朝向牌心（标准扑克排布）。
fn place(
    out: &mut String,
    art: &Fragment<'_>,
    slot: crate::theme::Slot,
    corner: Corner,
    cw: f64,
    ch: f64,
) {
    let s = slot.scale;
    let frag_w = art.width;
    let frag_h = art.height;
    let (tx, ty, rotated) = match corner {
        Corner::TopLeft => (slot.x, slot.y, false),
        Corner::TopRight => (cw - slot.x, slot.y + frag_h * s, true),
        Corner::BottomLeft => (slot.x + frag_w * s, ch - slot.y, true),
        Corner::BottomRight => (cw - slot.x, ch - slot.y, true),
    };
    if rotated {
        writeln!(
            out,
            "<g transform=\"translate({} {}) rotate(180) scale({})\">{}</g>",
            num(tx),
            num(ty),
            num(s),
            art.inner
        )
        .ok();
    } else {
        writeln!(
            out,
            "<g transform=\"translate({} {}) scale({})\">{}</g>",
            num(tx),
            num(ty),
            num(s),
            art.inner
        )
        .ok();
    }
}

/// 牌背：独立素材，整幅铺满。
pub fn render_back(theme: &Theme) -> Result<String, DeckError> {
    let meta = theme.meta();
    let mut out = String::with_capacity(2048);
    write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" width=\"{}\" height=\"{}\" data-theme=\"{}\" data-theme-version=\"{}\" data-face=\"BACK\">\n<!-- gf-deckart {} theme={} layout={} -->\n",
        num(meta.canvas.width),
        num(meta.canvas.height),
        num(meta.canvas.width),
        num(meta.canvas.height),
        meta.id,
        meta.version,
        crate::VERSION,
        theme.fingerprint(),
        layout_key(meta.layout)
    )
    .ok();
    let back = fragment("back", theme.asset(&meta.layers.back))?;
    out.push_str("<g id=\"L1-back\">");
    out.push_str(back.inner);
    out.push_str("</g>\n</svg>\n");
    Ok(out)
}

fn header(
    theme: &Theme,
    face: &Face,
    level: Option<LevelState>,
    is_level: bool,
    is_wild: bool,
) -> String {
    let meta = theme.meta();
    let level_attr = match level {
        Some(l) => l.level_rank.label().to_string(),
        None => "none".to_string(),
    };
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" width=\"{}\" height=\"{}\" data-theme=\"{}\" data-theme-version=\"{}\" data-face=\"{}\" data-level=\"{}\" data-leveled=\"{}\" data-wild=\"{}\" data-layout=\"{}\">\n<!-- gf-deckart {} theme={} layout={} -->\n",
        num(meta.canvas.width),
        num(meta.canvas.height),
        num(meta.canvas.width),
        num(meta.canvas.height),
        meta.id,
        meta.version,
        face.code(),
        level_attr,
        is_level,
        is_wild,
        layout_key(meta.layout),
        crate::VERSION,
        theme.fingerprint(),
        layout_key(meta.layout)
    )
}

fn layout_key(layout: Layout) -> &'static str {
    match layout {
        Layout::Compact => "compact",
        Layout::Classic => "classic",
    }
}

/// 素材内的墨色占位：渲染时按花色替换成主题 palette。
fn recolor(inner: &str, ink: &str) -> String {
    inner.replace("#1B1B1B", ink).replace("#C62828", ink)
}

/// 抽取素材的 `<svg>` 内部内容与其固有尺寸。
struct Fragment<'a> {
    inner: &'a str,
    width: f64,
    height: f64,
}

fn fragment<'a>(name: &str, svg: &'a str) -> Result<Fragment<'a>, DeckError> {
    let open_end = svg
        .find('>')
        .ok_or_else(|| DeckError::AssetInvalid(format!("{name} 缺少 <svg> 起始标签结束符")))?;
    let close_start = svg
        .rfind("</svg>")
        .ok_or_else(|| DeckError::AssetInvalid(format!("{name} 缺少 </svg>")))?;
    if close_start <= open_end {
        return Err(DeckError::AssetInvalid(format!("{name} 结构异常")));
    }
    let (w, h) = viewbox_size(&svg[..open_end])
        .ok_or_else(|| DeckError::AssetInvalid(format!("{name} 的 viewBox 无法解析")))?;
    Ok(Fragment {
        inner: &svg[open_end + 1..close_start],
        width: w,
        height: h,
    })
}

fn viewbox_size(tag: &str) -> Option<(f64, f64)> {
    let idx = tag.find("viewBox=")?;
    let rest = &tag[idx + "viewBox=".len()..];
    let quote = rest.chars().next()?;
    let end = rest[1..].find(quote)? + 1;
    let body = &rest[1..end];
    let parts: Vec<f64> = body
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();
    if parts.len() == 4 {
        Some((parts[2], parts[3]))
    } else {
        None
    }
}

/// 去掉多余小数（`1.60` → `1.6`，`250` → `250`）。
fn num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// 文本层转义：只允许纯文本，不解析任何标记。
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 主题选择界面的预览：按 `theme.json` 的 `preview` 配 1..=8 张面，
/// 与牌桌共用 `render_face`，预览与实战不会漂移。
pub fn render_preview(theme: &Theme) -> Result<Vec<RenderedFace>, DeckError> {
    let (faces, level) = theme.preview()?;
    let mut out = Vec::with_capacity(faces.len());
    for face in faces {
        out.push(RenderedFace {
            code: face.code(),
            svg: render_face(theme, face, level)?,
        });
    }
    Ok(out)
}

/// 54 面 + 牌背（`render_all` 只出 54 面，牌背单独取）。
pub fn render_all(
    theme: &Theme,
    level: Option<LevelState>,
) -> Result<Vec<RenderedFace>, DeckError> {
    render_all_opts(
        theme,
        FaceOptions {
            level,
            corner_scale: 1.0,
        },
    )
}

/// 54 面全量出图，带角标尺寸倍率（出「大字角标」整包时用）。
pub fn render_all_opts(
    theme: &Theme,
    opts: FaceOptions,
) -> Result<Vec<RenderedFace>, DeckError> {
    let mut out = Vec::with_capacity(54);
    for face in crate::card::all_faces() {
        out.push(RenderedFace {
            code: face.code(),
            svg: render_face_opts(theme, face, opts)?,
        });
    }
    Ok(out)
}
