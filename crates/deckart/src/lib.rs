//! 掼蛋牌面主题工具包（骨架，待确认后实现）。
//!
//! 目标：把「一套素材」按 `{suit, rank, levelRank}` 动态合成 54 张牌面，
//! 并以加密素材包（`.gfdeck`）形式分发；换肤只换素材与徽章，不改规则、不改协议。
//!
//! 规划中的模块（见 README）：
//! - `card`   牌面语义类型（Face / Suit / Rank / Joker），与 engine 的 card id 解耦
//! - `theme`  主题目录加载、theme.json 解析与校验
//! - `render` 四层合成（底面 → 花色 → 点数 → 级牌/逢人配徽章），输出 SVG
//! - `png`    SVG → PNG（resvg），仅在环境不支持 SVG 时使用
//! - `pack`   `.gfdeck` 素材包打包/解包/校验（含加密与签名）
//! - `error`  统一错误类型与错误码

#![forbid(unsafe_code)]

/// crate 版本，写进生成的 SVG 注释里便于追溯。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
