//! AI-L2（`hard` 档，§5.11）：AI-L1 加记牌与炸弹时机。
//!
//! 只在 L1 的取舍上加两件事，输入仍是 `view(seat)` + 本座位合法集 + 搭档状态：
//!
//! - **记牌**（`table.history` 的可见替代）：用 `discard` 公共弃牌堆统计各点数已出张数，
//!   推出「还差几张更大的牌」；领出时把「出手即无人能压」的控制牌留到后面，自己手牌不多时
//!   先甩控制牌锁牌权；
//! - **炸弹时机**（`bomb.count`）：按自己手上的炸弹个数决定开炸门槛——两个以上更愿意开，
//!   只剩一个且手牌还厚就留住。
//!
//! 严格说这是「从公开信息推演」，不是真的读对手手牌：`discard` 是所有座位都能看到的区，
//! 不构成信息隔离泄漏（§3.3）。

use gf_engine::contract::Action;
use gf_table_runtime::{BotContext, BotPolicy, HandPlan};

use crate::l0::L0Bot;
use crate::l1::choose;

/// AI-L2 记牌型 bot（无状态、确定性）。
#[derive(Clone, Copy, Debug, Default)]
pub struct L2Bot;

impl BotPolicy for L2Bot {
    fn decide(&self, view_bytes: &[u8], seat: u8) -> Action {
        L0Bot.decide(view_bytes, seat)
    }

    fn decide_with_legal(&self, view_bytes: &[u8], seat: u8, legal: &[Action]) -> Action {
        choose(view_bytes, seat, legal, &BotContext::default(), true, None)
    }

    fn decide_with_ctx(
        &self,
        view_bytes: &[u8],
        seat: u8,
        legal: &[Action],
        ctx: &BotContext,
    ) -> Action {
        choose(view_bytes, seat, legal, ctx, true, None)
    }

    fn decide_with_plan(
        &self,
        view_bytes: &[u8],
        seat: u8,
        legal: &[Action],
        ctx: &BotContext,
        plan: Option<&HandPlan>,
    ) -> Action {
        choose(view_bytes, seat, legal, ctx, true, plan)
    }
}
