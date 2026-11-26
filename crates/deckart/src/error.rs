//! 统一错误类型。对外错误文案可直接展示给用户。

#[derive(Debug, thiserror::Error)]
pub enum DeckError {
    #[error("主题配置无效: {0}")]
    ThemeInvalid(String),
    #[error("素材缺失: {0}")]
    AssetMissing(String),
    #[error("素材格式无效: {0}")]
    AssetInvalid(String),
    #[error("牌面编码无效: {0}")]
    FaceInvalid(String),
    #[error("素材包损坏: {0}")]
    PackCorrupt(String),
    #[error("素材包解密失败（密钥不符或数据被改写）")]
    PackDecrypt,
    #[error("素材包签名校验失败")]
    PackSignature,
    #[error("素材包已存在，拒绝覆盖: {0}")]
    PackExists(String),
    #[error("PNG 转换失败: {0}")]
    Png(String),
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
}

impl DeckError {
    /// 稳定错误码，供 CLI 与客户端日志检索（改动需同步文档）。
    pub fn code(&self) -> &'static str {
        match self {
            DeckError::ThemeInvalid(_) => "DK-THEME-INVALID",
            DeckError::AssetMissing(_) => "DK-ASSET-MISSING",
            DeckError::AssetInvalid(_) => "DK-ASSET-INVALID",
            DeckError::FaceInvalid(_) => "DK-FACE-INVALID",
            DeckError::PackCorrupt(_) => "DK-PACK-CORRUPT",
            DeckError::PackDecrypt => "DK-PACK-DECRYPT",
            DeckError::PackSignature => "DK-PACK-SIGNATURE",
            DeckError::PackExists(_) => "DK-PACK-EXISTS",
            DeckError::Png(_) => "DK-PNG",
            DeckError::Io(_) => "DK-IO",
            DeckError::Json(_) => "DK-JSON",
        }
    }
}
