//! 掼蛋牌面主题工具包。
//!
//! 一套素材（1 张白底模板 + 4 个花色 + 13 个点数字形 + 大王/小王 + 牌背 + 2 个徽章 = 23 个文件）
//! 按 `{suit, rank, levelRank}` 四层合成 54 个牌面，输出自包含 SVG；需要时转 PNG。
//!
//! 三条硬约束：
//! 1. 级牌高亮与逢人配标记是**独立叠加层**，不烘进模板底图；
//! 2. `themeId + themeVersion` 管主题缓存失效，**不进** `viewSchemaVersion`；
//! 3. 与 `stateHash` / 协议 / 引擎零耦合：只接收语义牌面与服务端下发的 `levelRank`。
//!
//! ```
//! use gf_deckart::{render_face, Face, LevelState, Rank, Suit, Theme};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../themes/defaults");
//! let theme = Theme::load_dir(&dir)?;
//! let face = Face::normal(Suit::Spade, Rank::new(8)?);
//! let svg = render_face(&theme, face, Some(LevelState::new(Rank::new(8)?)))?;
//! assert!(svg.contains("data-leveled=\"true\""));
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

pub mod card;
pub mod error;
pub mod pack;
pub mod render;
pub mod theme;

#[cfg(feature = "png")]
pub mod png;

pub use card::{all_faces, Face, Rank, Suit};
pub use error::DeckError;
pub use render::{
    render_all, render_all_opts, render_back, render_face, render_face_opts, render_preview,
    FaceOptions, LevelState, RenderedFace, CORNER_SCALE_MAX, CORNER_SCALE_MIN,
};
pub use theme::{
    BadgeMode, ClassicSpec, LabelText, Layout, PreviewSpec, TextLayer, Theme, ThemeMeta,
};

/// crate 版本，写进生成的 SVG 注释里便于追溯。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
