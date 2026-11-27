//! AI-L0（`easy` 档，§5.11）：只做合法性与最小牌。
//!
//! 决策只依赖 `view(seat)` 与房间下发的**本座位合法集**：
//! 1. 有合法集 → 取「牌力升序、同牌力按牌 id 升序」的第一条（托管同款口径，§5.10）；
//! 2. 无合法集（旧入口）→ 跟牌一律 `Pass`、领出出牌 id 最小的单张，保持 L0 的历史行为。
//!
//! 本档**不做**牌型取舍、不留牌、不配合队友；智能决策在 [`crate::L1Bot`]。

use gf_engine::contract::Action;
use gf_table_runtime::{BotContext, BotPolicy, HandPlan};

use crate::shape::{cards_key_of, count_of, is_bomb_shape, top_rank_of};

/// AI-L0 最小策略 bot（无状态、确定性）。
#[derive(Clone, Copy, Debug, Default)]
pub struct L0Bot;

impl BotPolicy for L0Bot {
    fn decide(&self, view_bytes: &[u8], seat: u8) -> Action {
        if crate::view_scan::is_following(view_bytes, seat) {
            return Action::Pass;
        }
        // 严格解析：视图漂移/截断时不能把「读不到手牌」当成空手牌直接 Pass。
        // 旧入口没有合法集可回落，只能显式 Pass；房间主路径走 `decide_with_plan`。
        let mut cards = match crate::view_scan::try_hand_cards(view_bytes) {
            Ok(cards) => cards,
            Err(_) => return Action::Pass,
        };
        cards.sort_unstable();
        match cards.first() {
            Some(&card) => Action::Play { cards: vec![card] },
            None => Action::Pass,
        }
    }

    fn decide_with_legal(&self, view_bytes: &[u8], seat: u8, legal: &[Action]) -> Action {
        // 跟牌不烧炸弹：炸弹是压场/冲刺资源，最小牌力排序会把它排在最前，
        // 只剩炸弹时宁可过牌（§5.11 与 §5.13 的配合口径）。
        let following = crate::view_scan::is_following(view_bytes, seat);
        legal
            .iter()
            .filter(|a| matches!(a, Action::Play { .. }) && count_of(a) > 0)
            .filter(|a| !(following && is_bomb_shape(a)))
            .min_by_key(|a| (top_rank_of(a), count_of(a), cards_key_of(a)))
            .cloned()
            .unwrap_or(Action::Pass)
    }

    /// 有推荐拆分时按「最小的整组」出：领出直接走拆分里最小组，
    /// 跟牌只在散张（单张组）里挑能压的，绝不把对子拆成单张。
    fn decide_with_plan(
        &self,
        view_bytes: &[u8],
        seat: u8,
        legal: &[Action],
        _ctx: &BotContext,
        plan: Option<&HandPlan>,
    ) -> Action {
        let Some(plan) = plan else {
            return self.decide_with_legal(view_bytes, seat, legal);
        };
        if !crate::view_scan::is_following(view_bytes, seat) {
            if let Some(action) = crate::plan::lead_from_plan(plan) {
                return action;
            }
            return self.decide_with_legal(view_bytes, seat, legal);
        }
        if let Some(action) = crate::plan::follow_single_from_plan(plan) {
            return action;
        }
        // 跟牌：合法集里只留「不拆组」的候选，挑不出就过牌——宁可过，不拆对子。
        // 炸弹永远不在此路径顺手打出：它是压场/冲刺资源，只有全是炸弹时才 Pass。
        legal
            .iter()
            .filter(|a| count_of(a) > 0)
            .filter(|a| !is_bomb_shape(a))
            .filter(|a| !crate::plan::splits_plan(crate::plan::action_cards(a), plan))
            .min_by_key(|a| (top_rank_of(a), count_of(a), cards_key_of(a)))
            .cloned()
            .unwrap_or(Action::Pass)
    }
}
