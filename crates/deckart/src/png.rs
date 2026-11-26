//! SVG → PNG（resvg + tiny-skia）。仅用于「目标环境不支持 SVG」的场景：
//! PixiJS 纹理、导出预览、CI 视觉回归。默认输出仍是 SVG。

#![cfg(feature = "png")]

use std::sync::OnceLock;

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{Options, Tree};

use crate::error::DeckError;

/// 复用一份加载了系统字体的解析选项：文本层开启时（级牌「级牌」字样）才能光栅化出字。
/// 素材本身不依赖字体，只有可选文本层需要；系统无中文字体时该层退化为空白，不影响牌面。
fn options() -> &'static Options<'static> {
    static OPTS: OnceLock<Options<'static>> = OnceLock::new();
    OPTS.get_or_init(|| {
        let mut opt = Options::default();
        opt.fontdb_mut().load_system_fonts();
        opt
    })
}

/// 把单文件 SVG 光栅化。`scale = dpr × faceSize / canvasWidth`。
pub fn svg_to_png(svg: &str, scale: f64) -> Result<Vec<u8>, DeckError> {
    if !(0.0..=16.0).contains(&scale) || scale <= 0.0 {
        return Err(DeckError::Png(format!("scale 越界: {scale}")));
    }
    let tree =
        Tree::from_str(svg, options()).map_err(|e| DeckError::Png(format!("SVG 解析失败: {e}")))?;
    let size = tree.size();
    let w = (size.width() as f64 * scale).ceil() as u32;
    let h = (size.height() as f64 * scale).ceil() as u32;
    if w == 0 || h == 0 {
        return Err(DeckError::Png("目标尺寸为 0".into()));
    }
    if w > 8192 || h > 8192 {
        return Err(DeckError::Png(format!("目标尺寸过大: {w}x{h}")));
    }
    let mut pixmap =
        Pixmap::new(w, h).ok_or_else(|| DeckError::Png(format!("分配位图失败: {w}x{h}")))?;
    resvg::render(
        &tree,
        Transform::from_scale(scale as f32, scale as f32),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .map_err(|e| DeckError::Png(format!("PNG 编码失败: {e}")))
}
