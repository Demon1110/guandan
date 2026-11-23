//! L1 校验入口（占位，后续提交补全）。

use crate::contract::{Level, Validation};
use crate::ruleset::RuleSetBody;

pub fn validate(_ruleset: &RuleSetBody, _levels: &[Level]) -> Validation {
    Validation::default()
}
