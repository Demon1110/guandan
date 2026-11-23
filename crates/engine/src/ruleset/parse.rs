//! 反序列化入口：区分「构造失败」与「校验不通过」（§2.4 gate 第 4 项、§5.4 L1）。
//!
//! - [`from_yaml`]：构造入口。反序列化失败（缺必填键 / 类型不符 / 未知键 / 未知枚举值 /
//!   未知原语）返回 [`SchemaError`]，错误码固定 `RS_SCHEMA_INVALID`，带 `location` 与 `message`；
//! - [`crate::ruleset::validate::validate`]：接受已构造的 `&RuleSetBody`，返回 [`Validation`]。

use crate::contract::ErrorCode;
use crate::ruleset::model::RuleSetBody;

/// 构造期错误：无法产出合法 `RuleSetBody`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaError {
    pub code: ErrorCode,
    pub location: String,
    pub message: String,
}

impl SchemaError {
    pub fn schema_invalid(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::RsSchemaInvalid,
            location: location.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} @ {}: {}", self.code.code_str(), self.location, self.message)
    }
}

impl std::error::Error for SchemaError {}

fn map_yaml_error(e: serde_yaml::Error) -> SchemaError {
    let location = match e.location() {
        Some(loc) => format!("line {} col {}", loc.line(), loc.column()),
        None => "<root>".to_owned(),
    };
    SchemaError {
        code: ErrorCode::RsSchemaInvalid,
        location,
        message: e.to_string(),
    }
}

/// 从草稿态 YAML 文本构造 `RuleSetBody`（不含发布态注入的 `version` 键）。
pub fn from_yaml(src: &str) -> Result<RuleSetBody, SchemaError> {
    let value: serde_yaml::Value = serde_yaml::from_str(src).map_err(map_yaml_error)?;
    let body: RuleSetBody = serde_yaml::from_value(value).map_err(map_yaml_error)?;
    Ok(body)
}
