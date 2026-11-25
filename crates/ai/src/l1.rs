//! AI-L1（`normal` 档，§5.11）：评估 + 有限扫描，保留大牌、配合队友。
//!
//! 与 L0 的差别只在**在合法集里怎么挑**（不改变合法性，也不看其他座位的手牌）：
//!
//! - **能一把走完就出完**（候选张数 == 手牌张数）——优先结束自己的回合；
//! - **领出**：排除炸弹后在候选里取「张数最多、同张数牌力最小」的一条。多张牌型先走（顺子 /
//!   连对 / 钢板 / 三带二），单张时出最小的那张，大牌与王自然被留到后面；
//! - **跟牌**：只压「同张数、非炸弹、牌力最小」的那条；没有同型候选时，只有遇到「对手压的
//!   是大牌（J 以上）」「对手本身就出的是炸弹」「自己手牌已很少（≤8 张）」三种情形才动炸弹，
//!   其余一律 `Pass` 把这手牌留给后面；
//! - 炸弹与四王炸永远排在普通牌型之后被使用。
//!
//! 队友配合（`partner.state`）在 M1 先不落：视图白名单（§5.10）只有 `hand`、`seat.levels`、
//! `discard`、`lastPlay`、`round.level`、`round.attackingTeam`，拿不到队友剩余牌数与座位状态；
//! 要落需先扩视图白名单（属设计变更），此处不擅自引入隐藏信息。

use std::cmp::Reverse;

use gf_engine::contract::Action;
use gf_table_runtime::BotPolicy;

use crate::l0::L0Bot;
use crate::shape::{cards_key_of, count_of, is_bomb_shape, rank_of, top_rank_of};
use crate::view_scan;

/// AI-L1 评估型 bot（无状态、确定性）。
#[derive(Clone, Copy, Debug, Default)]
pub struct L1Bot;

impl BotPolicy for L1Bot {
    /// 无合法集时的兜底（与 L0 同口径），只为兼容旧入口。
    fn decide(&self, view_bytes: &[u8], seat: u8) -> Action {
        L0Bot.decide(view_bytes, seat)
    }

    fn decide_with_legal(&self, view_bytes: &[u8], seat: u8, legal: &[Action]) -> Action {
        choose(view_bytes, seat, legal)
    }
}

fn choose(view: &[u8], seat: u8, legal: &[Action]) -> Action {
    let hand_len = view_scan::hand_cards(view).len();
    let plays: Vec<&Action> = legal.iter().filter(|a| count_of(a) > 0).collect();
    if plays.is_empty() {
        return Action::Pass;
    }
    // ① 能一次出完就直接出完（同张数多条时取牌力最小）。
    if let Some(finish) = plays
        .iter()
        .filter(|a| count_of(a) == hand_len)
        .min_by_key(|a| (top_rank_of(a), cards_key_of(a)))
    {
        return (*finish).clone();
    }

    let following = view_scan::is_following(view, seat);
    if !following {
        return lead(&plays);
    }

    let last_cards = view_scan::last_play_cards(view);
    let last_is_bomb = view_scan::last_play_pattern_id(view)
        .map(|id| id.starts_with("bomb") || id == "joker-bomb")
        .unwrap_or(false);
    follow(&plays, &last_cards, last_is_bomb, hand_len)
}

/// 领出：优先多张牌型清手，同张数取牌力最小；炸弹留到最后。
fn lead(plays: &[&Action]) -> Action {
    let normal: Vec<&&Action> = plays.iter().filter(|a| !is_bomb_shape(a)).collect();
    let pool: Vec<&&Action> = if normal.is_empty() {
        plays.iter().collect()
    } else {
        normal
    };
    pool.into_iter()
        .min_by_key(|a| (Reverse(count_of(a)), top_rank_of(a), cards_key_of(a)))
        .map(|a| (*a).clone())
        .unwrap_or(Action::Pass)
}

/// 跟牌：同张数最小代价先压；必要时才动炸弹。
fn follow(plays: &[&Action], last_cards: &[u16], last_is_bomb: bool, hand_len: usize) -> Action {
    let need = last_cards.len();
    if !last_is_bomb {
        // 普通牌型：同张数、非炸弹、牌力最小的一条。
        if let Some(best) = plays
            .iter()
            .filter(|a| count_of(a) == need && !is_bomb_shape(a))
            .min_by_key(|a| (top_rank_of(a), cards_key_of(a)))
        {
            return (*best).clone();
        }
    } else {
        // 对手是炸弹：同张数的非炸弹只能是同花顺（5 张炸之上），先试它。
        if let Some(best) = plays
            .iter()
            .filter(|a| count_of(a) == need && !is_bomb_shape(a))
            .min_by_key(|a| (top_rank_of(a), cards_key_of(a)))
        {
            return (*best).clone();
        }
    }

    let last_top = last_cards.iter().copied().map(rank_of).max().unwrap_or(0);
    // J(9) 以上视为「大牌」；对手已压大牌、或自己手牌 ≤8 张（该搏一手）才动炸弹。
    let worth_bomb = last_is_bomb || last_top >= 9 || hand_len <= 8;
    if worth_bomb {
        if let Some(bomb) = plays
            .iter()
            .filter(|a| is_bomb_shape(a))
            .min_by_key(|a| (top_rank_of(a), count_of(a), cards_key_of(a)))
        {
            return (*bomb).clone();
        }
    }
    Action::Pass
}
