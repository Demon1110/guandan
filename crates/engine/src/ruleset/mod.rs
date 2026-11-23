//! RuleSetBody 类型与 L1 校验器（§5.1 / §5.2 / §5.3 / §5.4）。
//! 本模块在后续提交中补全；当前为骨架占位。

pub mod validate;

/// RuleSet 草稿态本体（不含发布态注入的 `version` 键，§5.1 第 378 行）。
#[derive(Clone, Debug, Default)]
pub struct RuleSetBody {
    _placeholder: (),
}
