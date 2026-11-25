//! AI-L0（`easy` 档，§5.11）：只做合法性与最小牌。
//!
//! 决策只依赖 `view(seat)` 与房间下发的**本座位合法集**：
//! 1. 有合法集 → 取「牌力升序、同牌力按牌 id 升序」的第一条（托管同款口径，§5.10）；
//! 2. 无合法集（旧入口）→ 跟牌一律 `Pass`、领出出牌 id 最小的单张，保持 L0 的历史行为。
//!
//! 本档**不做**牌型取舍、不留牌、不配合队友；智能决策在 [`crate::L1Bot`]。

use gf_engine::contract::Action;
use gf_table_runtime::BotPolicy;

use crate::shape::{cards_key_of, count_of, top_rank_of};

/// AI-L0 最小策略 bot（无状态、确定性）。
#[derive(Clone, Copy, Debug, Default)]
pub struct L0Bot;

impl BotPolicy for L0Bot {
    fn decide(&self, view_bytes: &[u8], seat: u8) -> Action {
        if crate::view_scan::is_following(view_bytes, seat) {
            return Action::Pass;
        }
        let mut cards = crate::view_scan::hand_cards(view_bytes);
        cards.sort_unstable();
        match cards.first() {
            Some(&card) => Action::Play { cards: vec![card] },
            None => Action::Pass,
        }
    }

    fn decide_with_legal(&self, view_bytes: &[u8], seat: u8, legal: &[Action]) -> Action {
        let _ = (view_bytes, seat);
        legal
            .iter()
            .filter(|a| matches!(a, Action::Play { .. }) && count_of(a) > 0)
            .min_by_key(|a| (top_rank_of(a), count_of(a), cards_key_of(a)))
            .cloned()
            .unwrap_or(Action::Pass)
    }
}
