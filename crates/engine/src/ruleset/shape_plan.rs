//! 载入期 `ShapePlan` 编译（§5.12，第三十三次变更）。
//!
//! `pattern.compose` 与 8 个预置形状都在 RuleSet 载入期编译为本模块的 `ShapePlan`：
//! 判断器被折叠成枚举分支 + 位掩码 / 窗口表所需的参数；运行时不持有 predicate 列表、
//! 不做动态分派、不做字符串比较。`ShapePlan` 是载入期派生产物，不进 `stateHash`、
//! 不影响 `viewHash`。
//!
//! 8 个既有原语到预置形状的等价映射（§5.2 「现有 8 个牌型原语」段）：
//! - `same-rank{n}` ≡ `rank.same{n}`；
//! - `straight{n}` ≡ `rank.contiguous{n, 1}`（`n = 5` 等价 `wheel: true`）；
//! - `consecutive-pairs{n}` ≡ `rank.contiguous{n, 2}`；
//! - `consecutive-triples{n}` ≡ `rank.contiguous{n, 3}`；
//! - `straight-flush{n}` ≡ `rank.contiguous{n, 1}` + `sameSuit: true`；
//! - `bomb{n}` ≡ `rank.same{n}`；
//! - `joker-bomb` ≡ `joker.only{2, 2}`；
//! - `triple-pair` ≡ `rank.same{3}` + `attach{2, pair}`。

use crate::ruleset::params::{AttachParams, AttachUnit, ComposeShape, PrimitiveParams};

/// 附件编译产物（主牌之外携带的附件）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachPlan {
    pub count: u8,
    pub unit: AttachUnit,
    pub same_rank: bool,
}

impl From<&AttachParams> for AttachPlan {
    fn from(a: &AttachParams) -> Self {
        Self {
            count: a.count,
            unit: a.unit,
            same_rank: a.same_rank,
        }
    }
}

/// 炸弹 / 同花顺族角色（压制比较用，§5.13 阶梯口径不变）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BombRole {
    /// `rank.same{4..=8}`：普通炸弹，族内按 `bombOrder: count-then-rank`。
    SameRankBomb,
    /// `rank.contiguous{step=1, sameSuit=true}`：同花顺（插入型）。
    Flush,
    /// `joker.only`：王炸。
    JokerBomb,
    /// 非炸弹族。
    Other,
}

/// 编译后的形状计划；运行期判定只按枚举分支走，无 predicate 列表 / 动态分派。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapePlan {
    /// `rank.same{count}` + 可选附件。
    RankSame {
        count: u8,
        attach: Option<AttachPlan>,
    },
    /// `rank.contiguous{steps, step}` + 同花开关 + 轮子开关。
    RankContiguous {
        steps: u8,
        step: u8,
        flush: bool,
        wheel: bool,
    },
    /// `suit.same`：全部牌同一花色（王不得入内）。
    SuitSame,
    /// `joker.only{big, small}`：全部牌都是王。
    JokerOnly { big: u8, small: u8 },
}

impl ShapePlan {
    /// 把原语参数编译成 `ShapePlan`；v2 预留 / 未启用 / 不收录变体返回 `None`。
    pub fn compile(params: &PrimitiveParams) -> Option<ShapePlan> {
        match params {
            PrimitiveParams::SameRank { count } => Some(ShapePlan::RankSame {
                count: *count,
                attach: None,
            }),
            PrimitiveParams::Bomb { count } => Some(ShapePlan::RankSame {
                count: *count,
                attach: None,
            }),
            PrimitiveParams::Straight { length, .. } => Some(ShapePlan::RankContiguous {
                steps: *length,
                step: 1,
                flush: false,
                wheel: *length == 5,
            }),
            PrimitiveParams::ConsecutivePairs { pairs } => Some(ShapePlan::RankContiguous {
                steps: *pairs,
                step: 2,
                flush: false,
                wheel: false,
            }),
            PrimitiveParams::ConsecutiveTriples { triples } => Some(ShapePlan::RankContiguous {
                steps: *triples,
                step: 3,
                flush: false,
                wheel: false,
            }),
            PrimitiveParams::StraightFlush { length } => Some(ShapePlan::RankContiguous {
                steps: *length,
                step: 1,
                flush: true,
                wheel: false,
            }),
            PrimitiveParams::JokerBomb => Some(ShapePlan::JokerOnly { big: 2, small: 2 }),
            PrimitiveParams::TriplePair => Some(ShapePlan::RankSame {
                count: 3,
                attach: Some(AttachPlan {
                    count: 2,
                    unit: AttachUnit::Pair,
                    same_rank: true,
                }),
            }),
            PrimitiveParams::Compose {
                shape,
                attach,
                same_suit,
                wheel,
                ..
            } => match shape {
                ComposeShape::RankSame { count } => Some(ShapePlan::RankSame {
                    count: *count,
                    attach: attach.as_ref().map(AttachPlan::from),
                }),
                ComposeShape::RankContiguous { steps, step } => Some(ShapePlan::RankContiguous {
                    steps: *steps,
                    step: *step,
                    flush: *same_suit,
                    wheel: *wheel,
                }),
                ComposeShape::SuitSame => Some(ShapePlan::SuitSame),
                ComposeShape::JokerOnly { big, small } => Some(ShapePlan::JokerOnly {
                    big: *big,
                    small: *small,
                }),
            },
            PrimitiveParams::Reserved { .. } => None,
            _ => None,
        }
    }

    /// 规范化形状指纹：同一 RuleSet 内重复即 L1 `RS_SCHEMA_INVALID`（§5.2 身份与唯一性）。
    ///
    /// `maxCards` 是张数上限而非判断器结构，不进指纹；附件的张数 / 单位 / 同点开关进指纹。
    pub fn fingerprint(&self) -> String {
        match self {
            ShapePlan::RankSame { count, attach } => match attach {
                Some(a) => format!(
                    "rs|{count}|a|{}|{}|{}",
                    a.count,
                    unit_str(a.unit),
                    a.same_rank
                ),
                None => format!("rs|{count}|-"),
            },
            ShapePlan::RankContiguous {
                steps,
                step,
                flush,
                wheel,
            } => format!("rc|{steps}|{step}|{flush}|{wheel}"),
            ShapePlan::SuitSame => "ss".to_owned(),
            ShapePlan::JokerOnly { big, small } => format!("jo|{big}|{small}"),
        }
    }

    /// 成型张数（主牌 + 附件）；`suit.same` 这类无固定张数的形状返回 `None`。
    pub fn exact_total(&self) -> Option<usize> {
        match self {
            ShapePlan::RankSame { count, attach } => {
                let main = *count as usize;
                // `attach.count` 即附件张数；`unit` 只描述附件是单张还是对子形态，
                // 不再乘单位倍数（三带二：主 3 + 附件 2 = 5）。
                let extra = attach.as_ref().map(|a| a.count as usize).unwrap_or(0);
                Some(main + extra)
            }
            ShapePlan::RankContiguous { steps, step, .. } => Some(*steps as usize * *step as usize),
            ShapePlan::JokerOnly { big, small } => Some((*big + *small) as usize),
            ShapePlan::SuitSame => None,
        }
    }

    /// 炸弹 / 同花顺族角色（压制比较；族口径与 §5.10 阶梯一致，未新增族）。
    pub fn bomb_role(&self) -> BombRole {
        match self {
            ShapePlan::RankSame {
                count,
                attach: None,
            } if (4..=8).contains(count) => BombRole::SameRankBomb,
            ShapePlan::RankContiguous { flush: true, .. } => BombRole::Flush,
            ShapePlan::JokerOnly { .. } => BombRole::JokerBomb,
            _ => BombRole::Other,
        }
    }
}

fn unit_str(unit: AttachUnit) -> &'static str {
    match unit {
        AttachUnit::Single => "single",
        AttachUnit::Pair => "pair",
    }
}
