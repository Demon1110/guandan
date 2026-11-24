//! RuleSetBody 类型与 L1 校验器（§5.1 / §5.2 / §5.3 / §5.4）。

pub mod ladder;
pub mod model;
pub mod params;
pub mod parse;
pub mod shape_plan;
pub mod validate;

pub use model::RuleSetBody;
pub use parse::{from_yaml, SchemaError};
pub use validate::validate;
