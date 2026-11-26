//! 主题加载与校验：`themes/<id>/theme.json` + 素材目录，或从 `.gfdeck` 包在内存里组装。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::card::{Rank, Suit};
use crate::error::DeckError;

/// 主题可声明的默认角标倍率区间，和客户端滑块 100%–300% 对齐。
/// （渲染层的 `CORNER_SCALE_MIN/MAX` 更宽，是为了让 CLI 能试 0.6 这种极端值。）
pub const THEME_CORNER_MIN: f64 = 1.0;
pub const THEME_CORNER_MAX: f64 = 3.0;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Canvas {
    pub width: f64,
    pub height: f64,
    pub corner_radius: f64,
}

/// 四花色的独立墨色（可选）。没写的花色回落到 `palette.red` / `palette.black`，
/// 所以老主题不加这一段，输出与之前逐字节一致。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuitColors {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spade: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heart: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diamond: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub club: Option<String>,
}

impl SuitColors {
    pub fn get(&self, suit: Suit) -> Option<&str> {
        match suit {
            Suit::Spade => self.spade.as_deref(),
            Suit::Heart => self.heart.as_deref(),
            Suit::Diamond => self.diamond.as_deref(),
            Suit::Club => self.club.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Palette {
    pub red: String,
    pub black: String,
    pub paper: String,
    pub level: String,
    pub wild: String,
    /// 可选：黑桃 / 红桃 / 方块 / 梅花各自的墨色（四色牌面）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suits: Option<SuitColors>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct Slot {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Slots {
    pub corner_rank: Slot,
    pub corner_suit: Slot,
    pub center_suit: Slot,
    pub wild_badge: Slot,
}

/// 可选文本层：在徽章上写字（如「级牌」）。
/// 只在目标环境有可用字体时启用；关掉即退回纯图形，输出与 old 版本逐字节一致。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextLayer {
    #[serde(default)]
    pub enabled: bool,
    /// 与 `badges.level` 素材的三选一关系：`both`（默认，素材 + 文字叠加）/
    /// `badge-only`（只画素材）/ `text-only`（只画文字，不画素材层）
    #[serde(default)]
    pub badge: BadgeMode,
    #[serde(default = "default_font_family")]
    pub font_family: String,
    /// 级牌胶囊上的文字；`badge = both|text-only` 且 `enabled` 时必须存在
    #[serde(default)]
    pub level_label: Option<LabelText>,
}

/// 级牌标记的两种素材谁出场。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BadgeMode {
    /// 只画 `badges.level`（描边/胶囊/图标），不写字 —— 适合素材自带「级牌」字形的主题
    BadgeOnly,
    /// 素材与文本层同时出场（默认）
    #[default]
    Both,
    /// 只画文本层，完全不读 `badges.level` —— 适合自带描边/底板、只需补字的主题
    TextOnly,
}

impl TextLayer {
    /// 是否画 `badges.level` 素材层。
    /// 文本层整体关闭（`enabled = false`）时退回旧行为：只画素材，不读 `text-only`。
    pub fn draws_badge(&self) -> bool {
        !self.enabled || self.badge != BadgeMode::TextOnly
    }

    /// 是否画文字层（需要 `enabled`，且 badge-only 时不画）。
    pub fn draws_label(&self) -> bool {
        self.enabled && self.badge != BadgeMode::BadgeOnly
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelText {
    pub text: String,
    /// 文字基线锚点（x 为水平锚点位置，y 为基线）
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub color: String,
    /// `start` | `middle` | `end`
    #[serde(default = "default_anchor")]
    pub anchor: String,
}

fn default_font_family() -> String {
    "'Microsoft YaHei', 'PingFang SC', 'Noto Sans SC', sans-serif".to_string()
}

fn default_anchor() -> String {
    "middle".to_string()
}

/// 主题选择界面里的「四张 2」预览：只定义**渲染哪几张面**，不存任何图片。
/// 与牌桌走同一个 `render_face`，所以预览永远不会和实战不一致。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSpec {
    /// 面编码列表（`C2`/`S2`/`H8`/`JOKER-B`…），默认四张 2、按 ♣♠♥♦ 排列
    #[serde(default = "default_preview_faces")]
    pub faces: Vec<String>,
    /// 预览时当作级牌的点数（`1..=13`）；显式写 `null` 表示纯牌面。
    /// 缺省为 2 —— 四张全变级牌，红桃 2 同时变逢人配，一屏看全三种标记。
    #[serde(default = "default_preview_level")]
    pub level_rank: Option<u8>,
}

impl Default for PreviewSpec {
    fn default() -> PreviewSpec {
        PreviewSpec {
            faces: default_preview_faces(),
            level_rank: default_preview_level(),
        }
    }
}

fn default_preview_faces() -> Vec<String> {
    ["C2", "S2", "H2", "D2"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn default_preview_level() -> Option<u8> {
    Some(2)
}

/// 花阵安全边距缺省值：牌面四周留 10% 的空白
fn default_pip_inset() -> f64 {
    0.10
}

/// `layout = classic` 的花阵规格。坐标全部归一化到 0..1（相对牌面宽高），
/// 便于同一套表复用到不同 canvas 尺寸。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassicSpec {
    pub corner_rank: Slot,
    pub corner_suit: Slot,
    /// 花阵里单个花色符号的缩放（素材 100×100 → 画布单位）
    pub pip_scale: f64,
    /// A 用一个大符号，缩放独立
    pub ace_scale: f64,
    /// 花阵安全边距（归一化，占牌宽/高比例）。点位会被压进这块内边距之内，
    /// 保证花阵四周留白；角标放大后还会自动再往里让，避免点数压住花阵。
    /// 缺省 0.10。
    #[serde(default = "default_pip_inset")]
    pub pip_inset: f64,
    /// 可选的点位覆盖表（归一化中心坐标），key 为 `A` / `2`..`10` / `J` / `Q` / `K`
    /// （`A` 也接受 `1`）。没写的点数走标准花阵，空数组表示中间留白。
    #[serde(default)]
    pub pip_positions: std::collections::BTreeMap<String, Vec<[f64; 2]>>,
    /// J/Q/K 的中央处理：`monogram`（默认，一个大花色）| `figure`（预留人物素材）
    #[serde(default)]
    pub court_style: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SuitAssets {
    pub spade: String,
    pub heart: String,
    pub diamond: String,
    pub club: String,
}

impl SuitAssets {
    pub fn get(&self, suit: Suit) -> &str {
        match suit {
            Suit::Spade => &self.spade,
            Suit::Heart => &self.heart,
            Suit::Diamond => &self.diamond,
            Suit::Club => &self.club,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RankSource {
    /// 13 个独立矢量字形，索引 0 = A。
    Vector { files: Vec<String> },
    /// 位图图集 + 坐标表（预留，v1 未实现）。
    Atlas { image: String, index: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JokerAssets {
    pub small: String,
    pub big: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Layers {
    pub base: String,
    pub suits: SuitAssets,
    pub ranks: RankSource,
    pub jokers: JokerAssets,
    pub back: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Badges {
    pub level: String,
    pub wild: String,
}

/// 角标位置。`compact` 与 `classic` 共用这一套名字，`corners` 里写哪个就在哪个角出点数+花色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CornerPos {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl CornerPos {
    pub const ALL: [CornerPos; 4] = [
        CornerPos::TopLeft,
        CornerPos::TopRight,
        CornerPos::BottomLeft,
        CornerPos::BottomRight,
    ];

    pub fn name(self) -> &'static str {
        match self {
            CornerPos::TopLeft => "topLeft",
            CornerPos::TopRight => "topRight",
            CornerPos::BottomLeft => "bottomLeft",
            CornerPos::BottomRight => "bottomRight",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    /// 左上角标 + 中央大花色（参考图口径）
    Compact,
    /// 四角标 + 中央花阵（预留）
    Classic,
}

/// 角标花色相对点数的摆放：点数**下方**（传统扑克）还是点数**右侧**。
/// 缺省 `below`；角标放大后渲染层会按这个方向重排，保证花色不会被点数压住。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CornerSuitPlacement {
    Below,
    Right,
}

impl CornerSuitPlacement {
    pub fn name(self) -> &'static str {
        match self {
            CornerSuitPlacement::Below => "below",
            CornerSuitPlacement::Right => "right",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeMeta {
    pub id: String,
    pub version: u32,
    pub name: String,
    pub license: String,
    pub canvas: Canvas,
    pub palette: Palette,
    pub layout: Layout,
    /// 角标位置（可选）：`["topLeft","bottomRight"]` 这类写法，空数组 = 一个角标都不画。
    /// 缺省跟布局走：`compact` = 左上 + 右下，`classic` = 四角都有。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corners: Option<Vec<CornerPos>>,
    /// 角标倍率（可选）：1.0 = 素材原样，2.2 = 220%。缺省即 1.0。
    /// 客户端滑块没被手动调过时用它；`gfdeck render/render-all` 没给 `--corner` 时也用它。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_scale: Option<f64>,
    /// 角标花色摆在点数下方还是右侧（可选，缺省 `below`）。两种布局共用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_suit_placement: Option<CornerSuitPlacement>,
    pub layers: Layers,
    pub badges: Badges,
    pub slots: Slots,
    /// 可选文本层；缺省即关闭（纯图形输出）
    #[serde(default)]
    pub text_layer: Option<TextLayer>,
    /// `layout = classic` 时必填
    #[serde(default)]
    pub classic: Option<ClassicSpec>,
    /// 主题选择界面的预览配置；缺省即「四张 2、带级牌态」
    #[serde(default)]
    pub preview: Option<PreviewSpec>,
}

impl ThemeMeta {
    pub fn parse(json: &str) -> Result<ThemeMeta, DeckError> {
        serde_json::from_str(json)
            .map_err(|e| DeckError::ThemeInvalid(format!("theme.json 解析失败: {e}")))
    }

    /// 实际要画哪几个角的角标：`corners` 显式给了就用它，否则按布局给默认。
    /// 顺序固定按 `CornerPos::ALL`，保证 PNG/快照可复现。
    pub fn corner_positions(&self) -> Vec<CornerPos> {
        match &self.corners {
            Some(list) => CornerPos::ALL
                .iter()
                .copied()
                .filter(|c| list.contains(c))
                .collect(),
            None => match self.layout {
                Layout::Compact => vec![CornerPos::TopLeft, CornerPos::BottomRight],
                Layout::Classic => CornerPos::ALL.to_vec(),
            },
        }
    }

    /// 角标花色摆哪：主题没写就是点数下方（传统扑克口径）。
    pub fn corner_suit_placement(&self) -> CornerSuitPlacement {
        self.corner_suit_placement
            .unwrap_or(CornerSuitPlacement::Below)
    }

    /// 某个花色该用的墨色：`palette.suits` 里写了就用它，否则按红/黑二分。
    /// 四色牌面（黑桃黑、红桃红、方块蓝、梅花绿）就靠这一段。
    pub fn suit_ink(&self, suit: Suit) -> &str {
        if let Some(c) = self.palette.suits.as_ref().and_then(|s| s.get(suit)) {
            return c;
        }
        if suit.is_red() {
            &self.palette.red
        } else {
            &self.palette.black
        }
    }

    /// 引用到的全部素材相对路径（去重、保持声明顺序）。
    pub fn asset_paths(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push = |p: &str| {
            if !out.iter().any(|x| x == p) {
                out.push(p.to_string());
            }
        };
        push(&self.layers.base);
        for suit in Suit::ALL {
            push(self.layers.suits.get(suit));
        }
        match &self.layers.ranks {
            RankSource::Vector { files } => files.iter().for_each(|f| push(f)),
            RankSource::Atlas { image, index } => {
                push(image);
                push(index);
            }
        }
        push(&self.layers.jokers.small);
        push(&self.layers.jokers.big);
        push(&self.layers.back);
        push(&self.badges.level);
        push(&self.badges.wild);
        out
    }
}

/// 已加载的主题：元数据 + 素材原文（内存里就是 "文件名 → SVG 文本"）。
#[derive(Debug, Clone)]
pub struct Theme {
    meta: ThemeMeta,
    assets: HashMap<String, String>,
    origin: String,
}

impl Theme {
    /// 从磁盘目录加载（如 `crates/deckart/themes/defaults`）。
    pub fn load_dir(dir: &Path) -> Result<Theme, DeckError> {
        let meta_path = dir.join("theme.json");
        let meta_json = std::fs::read_to_string(&meta_path)
            .map_err(|e| DeckError::ThemeInvalid(format!("读不到 {}: {e}", meta_path.display())))?;
        let meta = ThemeMeta::parse(&meta_json)?;
        let mut assets = HashMap::new();
        for rel in meta.asset_paths() {
            safe_rel(&rel)?;
            let path = dir.join(&rel);
            let text =
                std::fs::read_to_string(&path).map_err(|_| DeckError::AssetMissing(rel.clone()))?;
            assets.insert(rel, text);
        }
        Theme::from_parts(meta, assets, dir.display().to_string())
    }

    /// 从内存组装（`.gfdeck` 解包后的路径；`origin` 只用于错误消息）。
    pub fn from_parts(
        meta: ThemeMeta,
        assets: HashMap<String, String>,
        origin: impl Into<String>,
    ) -> Result<Theme, DeckError> {
        let origin = origin.into();
        validate(&meta, &assets)?;
        Ok(Theme {
            meta,
            assets,
            origin,
        })
    }

    pub fn meta(&self) -> &ThemeMeta {
        &self.meta
    }

    /// 主题选择界面要渲染的面与级牌态：读 `theme.json` 的 `preview`，
    /// 缺省为「四张 2（♣♠♥♦）+ 级牌 2」。
    pub fn preview(
        &self,
    ) -> Result<(Vec<crate::card::Face>, Option<crate::render::LevelState>), DeckError> {
        let spec = self.meta.preview.clone().unwrap_or_default();
        let mut faces = Vec::with_capacity(spec.faces.len());
        for code in &spec.faces {
            faces.push(crate::card::Face::parse(code)?);
        }
        let level = match spec.level_rank {
            Some(v) => Some(crate::render::LevelState::new(crate::card::Rank::new(v)?)),
            None => None,
        };
        Ok((faces, level))
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// 主题声明的默认角标倍率；没声明时按素材原样（1.0）。
    pub fn corner_scale(&self) -> f64 {
        self.meta.corner_scale.unwrap_or(1.0)
    }

    /// 是否在 `theme.json` 里显式声明了默认角标（客户端据此决定要不要覆盖自己的兜底值）。
    pub fn has_corner_scale(&self) -> bool {
        self.meta.corner_scale.is_some()
    }

    /// 主题指纹：`defaults@1`，写进 SVG 注释与缓存键。
    pub fn fingerprint(&self) -> String {
        format!("{}@{}", self.meta.id, self.meta.version)
    }

    pub fn asset(&self, rel: &str) -> &str {
        self.assets.get(rel).map(String::as_str).unwrap_or_default()
    }

    pub fn rank_asset(&self, rank: Rank) -> Result<&str, DeckError> {
        match &self.meta.layers.ranks {
            RankSource::Vector { files } => {
                files.get(rank.index()).map(String::as_str).ok_or_else(|| {
                    DeckError::ThemeInvalid(format!("ranks.files 缺少第 {} 项", rank.index()))
                })
            }
            RankSource::Atlas { .. } => Err(DeckError::ThemeInvalid(
                "ranks.kind=atlas 尚未实现（v1 只支持 vector 字形）".into(),
            )),
        }
    }

    pub fn back_asset(&self) -> &str {
        &self.meta.layers.back
    }
}

fn validate(meta: &ThemeMeta, assets: &HashMap<String, String>) -> Result<(), DeckError> {
    if meta.id.is_empty() || meta.id.len() > 32 {
        return Err(DeckError::ThemeInvalid("id 长度需在 1..=32".into()));
    }
    if !meta
        .id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(DeckError::ThemeInvalid(format!(
            "id 只允许小写字母/数字/短横线: {}",
            meta.id
        )));
    }
    if meta.version == 0 {
        return Err(DeckError::ThemeInvalid("version 必须 ≥ 1".into()));
    }
    if meta.canvas.width <= 0.0 || meta.canvas.height <= 0.0 {
        return Err(DeckError::ThemeInvalid("canvas 尺寸必须为正".into()));
    }
    if meta.canvas.corner_radius < 0.0
        || meta.canvas.corner_radius > meta.canvas.width.min(meta.canvas.height) / 2.0
    {
        return Err(DeckError::ThemeInvalid("cornerRadius 越界".into()));
    }
    if !meta.palette.red.starts_with('#') || !meta.palette.black.starts_with('#') {
        return Err(DeckError::ThemeInvalid("palette 颜色需为 #RRGGBB".into()));
    }
    if let Some(suits) = &meta.palette.suits {
        for (name, color) in [
            ("spade", suits.spade.as_deref()),
            ("heart", suits.heart.as_deref()),
            ("diamond", suits.diamond.as_deref()),
            ("club", suits.club.as_deref()),
        ] {
            if let Some(c) = color {
                if !is_hex_color(c) {
                    return Err(DeckError::ThemeInvalid(format!(
                        "palette.suits.{name} 需为 #RRGGBB"
                    )));
                }
            }
        }
    }
    if let Some(scale) = meta.corner_scale {
        if !scale.is_finite() || !(THEME_CORNER_MIN..=THEME_CORNER_MAX).contains(&scale) {
            return Err(DeckError::ThemeInvalid(format!(
                "cornerScale 越界（合法 {THEME_CORNER_MIN}..={THEME_CORNER_MAX}，即 100%–300%）"
            )));
        }
    }
    if let Some(list) = &meta.corners {
        if list.len() > CornerPos::ALL.len() {
            return Err(DeckError::ThemeInvalid(
                "corners 最多 4 项（topLeft/topRight/bottomLeft/bottomRight）".into(),
            ));
        }
        for (i, c) in list.iter().enumerate() {
            if list[i + 1..].contains(c) {
                return Err(DeckError::ThemeInvalid(format!(
                    "corners 里 {} 重复了",
                    c.name()
                )));
            }
        }
    }
    match &meta.layers.ranks {
        RankSource::Vector { files } if files.len() != 13 => {
            return Err(DeckError::ThemeInvalid(format!(
                "ranks.files 必须是 13 项，实际 {}",
                files.len()
            )));
        }
        RankSource::Vector { .. } => {}
        RankSource::Atlas { .. } => {}
    }
    for slot in [
        meta.slots.corner_rank,
        meta.slots.corner_suit,
        meta.slots.center_suit,
        meta.slots.wild_badge,
    ] {
        if slot.scale <= 0.0 {
            return Err(DeckError::ThemeInvalid("slot.scale 必须为正".into()));
        }
    }
    for rel in meta.asset_paths() {
        safe_rel(&rel)?;
        let text = assets
            .get(&rel)
            .ok_or_else(|| DeckError::AssetMissing(rel.clone()))?;
        if text.trim().is_empty() {
            return Err(DeckError::AssetInvalid(format!("{rel} 为空")));
        }
        check_svg(&rel, text)?;
    }

    if meta.layout == Layout::Classic && meta.classic.is_none() {
        return Err(DeckError::ThemeInvalid(
            "layout=classic 时必须提供 classic 花阵规格".into(),
        ));
    }
    if let Some(spec) = &meta.classic {
        validate_classic(spec)?;
    }
    if let Some(text) = &meta.text_layer {
        if text.enabled && text.badge != BadgeMode::BadgeOnly {
            let label = text.level_label.as_ref().ok_or_else(|| {
                DeckError::ThemeInvalid(
                    "textLayer.enabled 且 badge=both|text-only 时必须提供 levelLabel".into(),
                )
            })?;
            validate_label(label)?;
        } else if let Some(label) = &text.level_label {
            // badge-only 时文字不参与渲染，但给了就照样校验，避免留坏配置
            validate_label(label)?;
        }
    }
    if let Some(preview) = &meta.preview {
        validate_preview(preview)?;
    }
    Ok(())
}

fn validate_preview(spec: &PreviewSpec) -> Result<(), DeckError> {
    if spec.faces.is_empty() || spec.faces.len() > 8 {
        return Err(DeckError::ThemeInvalid(
            "preview.faces 需 1..=8 张牌面".into(),
        ));
    }
    for code in &spec.faces {
        crate::card::Face::parse(code).map_err(|_| {
            DeckError::ThemeInvalid(format!("preview.faces 里的 {code} 不是合法牌面"))
        })?;
    }
    if let Some(v) = spec.level_rank {
        crate::card::Rank::new(v).map_err(|_| {
            DeckError::ThemeInvalid(format!("preview.levelRank={v} 越界（合法 1..=13）"))
        })?;
    }
    Ok(())
}

fn validate_label(label: &LabelText) -> Result<(), DeckError> {
    if label.text.trim().is_empty() {
        return Err(DeckError::ThemeInvalid("levelLabel.text 不能为空".into()));
    }
    if !(0.0..=200.0).contains(&label.size) || label.size <= 0.0 {
        return Err(DeckError::ThemeInvalid(
            "levelLabel.size 需在 1..=200".into(),
        ));
    }
    if !label.color.starts_with('#') {
        return Err(DeckError::ThemeInvalid(
            "levelLabel.color 需为 #RRGGBB".into(),
        ));
    }
    if !matches!(label.anchor.as_str(), "start" | "middle" | "end") {
        return Err(DeckError::ThemeInvalid(
            "levelLabel.anchor 只允许 start/middle/end".into(),
        ));
    }
    Ok(())
}

fn validate_classic(spec: &ClassicSpec) -> Result<(), DeckError> {
    if spec.pip_scale <= 0.0 || spec.ace_scale <= 0.0 {
        return Err(DeckError::ThemeInvalid(
            "classic.pipScale/aceScale 必须为正".into(),
        ));
    }
    if !(0.0..=0.35).contains(&spec.pip_inset) {
        return Err(DeckError::ThemeInvalid(format!(
            "classic.pipInset 必须归一化到 0..0.35，实际 {}",
            spec.pip_inset
        )));
    }
    if let Some(style) = &spec.court_style {
        if !matches!(style.as_str(), "monogram" | "figure") {
            return Err(DeckError::ThemeInvalid(
                "classic.courtStyle 只允许 monogram/figure".into(),
            ));
        }
    }
    // 点位表全部可选：没写哪个点数，就按标准花阵自动摆。
    // 写了就用自己那张表（数量不限，空数组表示中间留白）。
    for (key, points) in &spec.pip_positions {
        if !matches!(
            key.as_str(),
            "A" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "10" | "J" | "Q" | "K"
        ) {
            return Err(DeckError::ThemeInvalid(format!(
                "classic.pipPositions 的键只允许 A/2..10/J/Q/K，实际 {key}"
            )));
        }
        if points.len() > 16 {
            return Err(DeckError::ThemeInvalid(format!(
                "classic.pipPositions[{key}] 最多 16 个点位，实际 {}",
                points.len()
            )));
        }
        for p in points {
            if !(0.0..=1.0).contains(&p[0]) || !(0.0..=1.0).contains(&p[1]) {
                return Err(DeckError::ThemeInvalid(format!(
                    "classic.pipPositions[{key}] 坐标必须归一化到 0..1"
                )));
            }
        }
    }
    Ok(())
}

/// `#RRGGBB`（大小写不限）。
fn is_hex_color(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}

/// 拒绝绝对路径、`..`、反斜杠与任何跳出主题目录的写法。
fn safe_rel(rel: &str) -> Result<(), DeckError> {
    if rel.is_empty() {
        return Err(DeckError::ThemeInvalid("素材路径为空".into()));
    }
    if rel.starts_with('/') || rel.contains('\\') {
        return Err(DeckError::ThemeInvalid(format!(
            "素材路径必须用 / 且为相对路径: {rel}"
        )));
    }
    if rel
        .split('/')
        .any(|seg| seg == ".." || seg == "." || seg.is_empty())
    {
        return Err(DeckError::ThemeInvalid(format!(
            "素材路径不得包含 . 或 ..: {rel}"
        )));
    }
    Ok(())
}

/// 素材必须是自包含的 `<svg>` 文档；不引外链、不引字体。
fn check_svg(rel: &str, text: &str) -> Result<(), DeckError> {
    let t = text.trim_start();
    if !t.starts_with("<svg") {
        return Err(DeckError::AssetInvalid(format!("{rel} 不是以 <svg 开头")));
    }
    if !t.trim_end().ends_with("</svg>") {
        return Err(DeckError::AssetInvalid(format!("{rel} 未以 </svg> 结束")));
    }
    if !t.contains("viewBox=") {
        return Err(DeckError::AssetInvalid(format!("{rel} 缺 viewBox")));
    }
    if t.contains("xlink:href=\"http") || t.contains("<image") {
        return Err(DeckError::AssetInvalid(format!(
            "{rel} 引用了外部资源（禁止外链）"
        )));
    }
    Ok(())
}

/// 主题根目录（默认 crate 内的 `themes/`，可用 `GF_DECKART_THEMES` 覆盖）。
/// 主题源与本 crate 放一起，仓库根目录不再留 `themes/`。
pub fn themes_root() -> PathBuf {
    std::env::var_os("GF_DECKART_THEMES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("themes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_with(pips: &[(&str, Vec<[f64; 2]>)]) -> ClassicSpec {
        let mut map = std::collections::BTreeMap::new();
        for (k, v) in pips {
            map.insert(k.to_string(), v.clone());
        }
        ClassicSpec {
            corner_rank: Slot {
                x: 30.0,
                y: 30.0,
                scale: 0.4,
            },
            corner_suit: Slot {
                x: 36.0,
                y: 100.0,
                scale: 0.25,
            },
            pip_scale: 0.8,
            ace_scale: 1.6,
            pip_inset: 0.1,
            pip_positions: map,
            court_style: None,
        }
    }

    #[test]
    fn classic_pips_are_optional() {
        assert!(validate_classic(&spec_with(&[])).is_ok());
        assert!(validate_classic(&spec_with(&[
            ("A", vec![[0.5, 0.5]]),
            ("10", Vec::new()),
            ("Q", vec![[0.4, 0.3], [0.6, 0.7]])
        ]))
        .is_ok());
    }

    #[test]
    fn classic_pips_reject_bad_keys_and_coords() {
        assert!(validate_classic(&spec_with(&[("Z", vec![[0.5, 0.5]])])).is_err());
        assert!(validate_classic(&spec_with(&[("7", vec![[1.2, 0.5]])])).is_err());
        let many: Vec<[f64; 2]> = (0..17).map(|i| [0.1, i as f64 / 20.0]).collect();
        assert!(validate_classic(&spec_with(&[("7", many)])).is_err());
    }

    #[test]
    fn hex_colors_need_seven_chars() {
        assert!(is_hex_color("#1b1b1b"));
        assert!(is_hex_color("#ABC123"));
        assert!(!is_hex_color("#1b1b1"));
        assert!(!is_hex_color("1b1b1b"));
        assert!(!is_hex_color("#GGGGGG"));
    }
}
