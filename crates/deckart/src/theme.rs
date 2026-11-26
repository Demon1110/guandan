//! 主题加载与校验：`themes/<id>/theme.json` + 素材目录，或从 `.gfdeck` 包在内存里组装。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::card::{Rank, Suit};
use crate::error::DeckError;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Canvas {
    pub width: f64,
    pub height: f64,
    pub corner_radius: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Palette {
    pub red: String,
    pub black: String,
    pub paper: String,
    pub level: String,
    pub wild: String,
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
    #[serde(default = "default_font_family")]
    pub font_family: String,
    /// 级牌胶囊上的文字；`enabled` 时必须存在
    #[serde(default)]
    pub level_label: Option<LabelText>,
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
    /// 2..10 的花阵点位（归一化中心坐标），key 为点数标签
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    /// 左上角标 + 中央大花色（参考图口径）
    Compact,
    /// 四角标 + 中央花阵（预留）
    Classic,
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
    pub layers: Layers,
    pub badges: Badges,
    pub slots: Slots,
    /// 可选文本层；缺省即关闭（纯图形输出）
    #[serde(default)]
    pub text_layer: Option<TextLayer>,
    /// `layout = classic` 时必填
    #[serde(default)]
    pub classic: Option<ClassicSpec>,
}

impl ThemeMeta {
    pub fn parse(json: &str) -> Result<ThemeMeta, DeckError> {
        serde_json::from_str(json)
            .map_err(|e| DeckError::ThemeInvalid(format!("theme.json 解析失败: {e}")))
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
    /// 从磁盘目录加载（`themes/defaults`）。
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

    pub fn origin(&self) -> &str {
        &self.origin
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
        if text.enabled {
            let label = text.level_label.as_ref().ok_or_else(|| {
                DeckError::ThemeInvalid("textLayer.enabled 时必须提供 levelLabel".into())
            })?;
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
        }
    }
    Ok(())
}

fn validate_classic(spec: &ClassicSpec) -> Result<(), DeckError> {
    if spec.pip_scale <= 0.0 || spec.ace_scale <= 0.0 {
        return Err(DeckError::ThemeInvalid(
            "classic.pipScale/aceScale 必须为正".into(),
        ));
    }
    if let Some(style) = &spec.court_style {
        if !matches!(style.as_str(), "monogram" | "figure") {
            return Err(DeckError::ThemeInvalid(
                "classic.courtStyle 只允许 monogram/figure".into(),
            ));
        }
    }
    for value in 2u8..=10 {
        let key = Rank::new(value)?.label().to_string();
        let Some(points) = spec.pip_positions.get(&key) else {
            return Err(DeckError::ThemeInvalid(format!(
                "classic.pipPositions 缺 {key} 的点位表"
            )));
        };
        if points.len() != value as usize {
            return Err(DeckError::ThemeInvalid(format!(
                "classic.pipPositions[{key}] 必须是 {value} 个点位，实际 {}",
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

/// 主题根目录（默认 `themes/`，可用 `GF_DECKART_THEMES` 覆盖）。
pub fn themes_root() -> PathBuf {
    std::env::var_os("GF_DECKART_THEMES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("themes"))
}
