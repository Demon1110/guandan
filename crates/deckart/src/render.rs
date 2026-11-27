//! 四层合成：L1 底面 → L2 花色 → L3 点数 → L4 级牌/逢人配徽章。
//! 输出单文件、自包含、无外链的 SVG；图层 id 与 `data-*` 属性固定，便于测试与视觉回归。

use std::fmt::Write as _;

use crate::card::{Face, Rank, Suit};
use crate::error::DeckError;
use crate::theme::{CornerPos, CornerSuitPlacement, Layout, Theme};

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

/// classic 花阵左右两列之间的最小视觉间距（画布单位）。角标放大到极限时，
/// 花阵宁可整体缩一点，也要留出这道缝，否则两列点位会粘在一起。
const PIP_GAP: f64 = 26.0;

/// 没在 `classic.pipPositions` 里声明的点数，按标准扑克花阵自动摆点位。
/// 返回 `None` 的只有 A/J/Q/K：它们默认走中央一个大符号。
/// 坐标是归一化的 (x, y)，x = 0.3 / 0.7 两列，y 在 0.26..0.74 之间。
pub fn auto_pips(rank: Rank) -> Option<Vec<[f64; 2]>> {
    let v = rank.value();
    if v == 1 || v >= 11 {
        return None;
    }
    if v <= 3 {
        let ys = match v {
            2 => vec![0.34, 0.66],
            _ => vec![0.28, 0.50, 0.72],
        };
        return Some(ys.into_iter().map(|y| [0.5, y]).collect());
    }

    let mut pts = Vec::with_capacity(v as usize);
    let rows = v / 2;
    let top = 0.26;
    let bottom = 0.74;
    let y_at = |i: u8| top + (bottom - top) * i as f64 / (rows - 1) as f64;
    for i in 0..rows {
        pts.push([0.3, y_at(i)]);
        pts.push([0.7, y_at(i)]);
    }
    if v % 2 == 1 {
        // 5 居中，7/9 夹在第二、三排之间，读起来更接近实体牌
        let y = match v {
            5 => 0.5,
            7 => 0.375,
            _ => 0.5,
        };
        pts.push([0.5, y]);
    }
    Some(pts)
}

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
pub fn render_face_opts(theme: &Theme, face: Face, opts: FaceOptions) -> Result<String, DeckError> {
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
    let mut slot = meta.slots.corner_rank;
    let mut suit_slot = meta.slots.corner_suit;
    let center = meta.slots.center_suit;
    let w = meta.canvas.width;
    let h = meta.canvas.height;

    let suit_rel = meta.layers.suits.get(suit);
    let suit_frag = fragment(suit_rel, theme.asset(suit_rel))?;
    let ink = meta.suit_ink(suit);
    let suit_inner = recolor(suit_frag.inner, ink);
    let rank_rel = theme.rank_asset(rank)?;
    let rank_frag = fragment(rank_rel, theme.asset(rank_rel))?;
    let rank_inner = recolor(rank_frag.inner, ink);

    // 角标倍率：点数与角标花色一起放大，并把花色重新排在放大后的点数下方，
    // 否则大字号下点数会压到花色上（原布局的间距是按小字号给的）。
    if (corner - 1.0).abs() > f64::EPSILON {
        slot.scale *= corner;
        suit_slot.scale *= corner;
    }
    // 花色摆点数下方还是右侧（`cornerSuitPlacement`）。下方只在放大后重排（1.0 保持素材原样），
    // 右侧是配置驱动的，任何时候都按点数的实际右缘/底缘摆。
    match meta.corner_suit_placement() {
        CornerSuitPlacement::Below if (corner - 1.0).abs() > f64::EPSILON => {
            suit_slot.y = slot.y + rank_frag.height * slot.scale + CORNER_GAP * corner;
        }
        CornerSuitPlacement::Below => {}
        CornerSuitPlacement::Right => {
            // 右侧摆放：贴着点数的右缘，纵向居中对齐点数（中央没有花阵，不用担心挤压）
            suit_slot.x = slot.x + rank_frag.width * slot.scale + CORNER_GAP * corner;
            suit_slot.y =
                slot.y + (rank_frag.height * slot.scale - suit_frag.height * suit_slot.scale) / 2.0;
        }
    }

    // L2 花色：角标 + 中央 + 右下镜像（角标区）
    let suit_art = Fragment {
        inner: &suit_inner,
        width: suit_frag.width,
        height: suit_frag.height,
    };
    let rank_art = Fragment {
        inner: &rank_inner,
        width: rank_frag.width,
        height: rank_frag.height,
    };
    let corners = meta.corner_positions();
    writeln!(out, "<g id=\"L2-suit\">").ok();
    // 右下角标有自己的镜像组（与点数成对），先跳过，最后单独写
    for corner in corners
        .iter()
        .copied()
        .filter(|c| *c != CornerPos::BottomRight)
    {
        place(out, &suit_art, suit_slot, corner, w, h);
    }
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

    // L3 点数：按 corners 配置逐个角出，左上正写、其余旋转 180°
    writeln!(out, "<g id=\"L3-rank\">").ok();
    for corner in corners.iter().copied() {
        place(out, &rank_art, slot, corner, w, h);
    }
    writeln!(out, "</g>").ok();

    // 右下镜像的角标花色（与点数一起构成对称角标）
    if corners.contains(&CornerPos::BottomRight) {
        writeln!(
            out,
            "<g id=\"L2-suit-mirror\" transform=\"translate({} {}) rotate(180) scale({})\">{}</g>",
            num(w - suit_slot.x),
            num(h - suit_slot.y),
            num(suit_slot.scale),
            suit_inner
        )
        .ok();
    }
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
    let cw = meta.canvas.width;
    let ch = meta.canvas.height;

    let suit_rel = meta.layers.suits.get(suit);
    let suit_frag = fragment(suit_rel, theme.asset(suit_rel))?;
    let ink = meta.suit_ink(suit);
    let suit_inner = recolor(suit_frag.inner, ink);
    let rank_rel = theme.rank_asset(rank)?;
    let rank_frag = fragment(rank_rel, theme.asset(rank_rel))?;
    let rank_inner = recolor(rank_frag.inner, ink);

    // 角标：点数放大后把花色重排到放大后的点数下方（与 compact 同一套规则），
    // 否则大字会直接压住花色，读不出什么花色。下方 / 右侧由 `cornerSuitPlacement` 决定。
    let mut corner_rank = spec.corner_rank;
    let mut corner_suit = spec.corner_suit;
    if (corner_scale - 1.0).abs() > f64::EPSILON {
        corner_rank.scale *= corner_scale;
        corner_suit.scale *= corner_scale;
    }
    match meta.corner_suit_placement() {
        CornerSuitPlacement::Below if (corner_scale - 1.0).abs() > f64::EPSILON => {
            corner_suit.y =
                corner_rank.y + rank_frag.height * corner_rank.scale + CORNER_GAP * corner_scale;
        }
        CornerSuitPlacement::Below => {}
        CornerSuitPlacement::Right => {
            // 右侧摆放靠点数本身占位，花色到这儿的间距不再跟着倍率涨（否则花阵没地方站）
            let gap = CORNER_GAP;
            corner_suit.x = corner_rank.x + rank_frag.width * corner_rank.scale + gap;
            corner_suit.y = corner_rank.y
                + (rank_frag.height * corner_rank.scale - suit_frag.height * corner_suit.scale)
                    / 2.0;
        }
    }

    // 中央内容：`classic.pipPositions` 里写了这个点数就用你配的摆放，
    // 没写就回退到标准花阵（A/J/Q/K 默认画一个中央大符号）。
    let custom = spec
        .pip_positions
        .get(rank.label())
        .or_else(|| match rank.value() {
            1 => spec.pip_positions.get("1"),
            _ => None,
        });
    let points = custom.cloned().or_else(|| auto_pips(rank));

    writeln!(out, "<g id=\"L2-suit\">").ok();
    match points {
        None => {
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
        Some(points) => {
            // 花阵安全区：先按 pipInset 留出内边距，再按角标实际宽度继续往里让，
            // 保证放大的点数/花色不会压到最近的点位。
            let corner_right = (corner_rank.x + rank_frag.width * corner_rank.scale)
                .max(corner_suit.x + suit_frag.width * corner_suit.scale);
            let corner_gap = match meta.corner_suit_placement() {
                CornerSuitPlacement::Below => CORNER_GAP * corner_scale,
                CornerSuitPlacement::Right => CORNER_GAP,
            };
            let inset_x = (spec.pip_inset * cw)
                .max(corner_right + corner_gap)
                .min(cw * 0.42);
            let inset_y = spec.pip_inset * ch;
            // 角标特别大时左右两列会被挤到一起：此时等比缩小花阵，宁可小一点也不让点位互相压住
            let two_col = points.iter().any(|p| (p[0] - 0.5).abs() > 0.1);
            let mut s = spec.pip_scale;
            if two_col {
                // 两列中心分别在 inset_x + 半宽 与 cw - inset_x - 半宽，两列间距 = cw - 2*inset_x - 符号宽
                let cap = (cw - 2.0 * inset_x - PIP_GAP) / (2.0 * suit_frag.width);
                s = s.min(cap.max(0.15));
            }
            let half_w = suit_frag.width * s / 2.0;
            let half_h = suit_frag.height * s / 2.0;
            let min_x = inset_x + half_w;
            let min_y = inset_y + half_h;
            let max_y = (ch - inset_y - half_h).max(min_y);
            for p in points {
                // 左右对称收拢：哪一列离牌边近就按哪一列让开，避免花阵整体偏到一边
                let dx = p[0].min(1.0 - p[0]) * cw;
                let ceiling = (cw * 0.5).max(min_x);
                let dx = dx.clamp(min_x, ceiling);
                let cx = if p[0] < 0.5 { dx } else { cw - dx };
                let cy = (p[1] * ch).clamp(min_y, max_y);
                let tx = cx - half_w;
                let ty = cy - half_h;
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
    let suit_art = Fragment {
        inner: &suit_inner,
        width: suit_frag.width,
        height: suit_frag.height,
    };
    let corners = meta.corner_positions();
    for corner in corners.iter().copied() {
        place(out, &suit_art, corner_suit, corner, cw, ch);
    }
    writeln!(out, "</g>").ok();

    // 四角点数
    writeln!(out, "<g id=\"L3-rank\">").ok();
    let rank_art = Fragment {
        inner: &rank_inner,
        width: rank_frag.width,
        height: rank_frag.height,
    };
    for corner in corners.iter().copied() {
        place(out, &rank_art, corner_rank, corner, cw, ch);
    }
    writeln!(out, "</g>").ok();
    Ok(())
}

/// 四角摆放：除左上外全部旋转 180°，让字头/花尖朝向牌心（标准扑克排布）。
fn place(
    out: &mut String,
    art: &Fragment<'_>,
    slot: crate::theme::Slot,
    corner: CornerPos,
    cw: f64,
    ch: f64,
) {
    let s = slot.scale;
    let frag_w = art.width;
    let frag_h = art.height;
    let (tx, ty, rotated) = match corner {
        CornerPos::TopLeft => (slot.x, slot.y, false),
        CornerPos::TopRight => (cw - slot.x, slot.y + frag_h * s, true),
        CornerPos::BottomLeft => (slot.x + frag_w * s, ch - slot.y, true),
        CornerPos::BottomRight => (cw - slot.x, ch - slot.y, true),
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
    // 只接受成对引号；按字符边界切，避免多字节引号（如全角引号）触发切片 panic。
    if quote != '"' && quote != '\'' {
        return None;
    }
    let after = &rest[quote.len_utf8()..];
    let end = after.find(quote)?;
    let body = &after[..end];
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
pub fn render_all_opts(theme: &Theme, opts: FaceOptions) -> Result<Vec<RenderedFace>, DeckError> {
    let mut out = Vec::with_capacity(54);
    for face in crate::card::all_faces() {
        out.push(RenderedFace {
            code: face.code(),
            svg: render_face_opts(theme, face, opts)?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_pips_covers_two_to_ten() {
        for v in 2u8..=10 {
            let rank = Rank::new(v).unwrap();
            let pts = auto_pips(rank).unwrap_or_else(|| panic!("{rank:?} 应有自动花阵"));
            assert_eq!(pts.len(), v as usize, "{} 的点位数不对", rank.label());
            for p in &pts {
                assert!((0.0..=1.0).contains(&p[0]) && (0.0..=1.0).contains(&p[1]));
            }
        }
    }

    #[test]
    fn viewbox_parses_ascii_quotes_and_ignores_multibyte() {
        assert_eq!(
            viewbox_size("<svg viewBox=\"0 0 250 350\">"),
            Some((250.0, 350.0))
        );
        assert_eq!(
            viewbox_size("<svg viewBox='0,0,10,20'>"),
            Some((10.0, 20.0))
        );
        // 多字节引号不能拿来切片：不 panic，按「无法解析」处理。
        assert_eq!(viewbox_size("<svg viewBox=“0 0 250 350”>"), None);
    }

    #[test]
    fn auto_pips_leaves_court_cards_to_big_symbol() {
        for v in [1u8, 11, 12, 13] {
            assert!(auto_pips(Rank::new(v).unwrap()).is_none());
        }
    }
}
