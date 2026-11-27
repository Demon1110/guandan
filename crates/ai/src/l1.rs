//! AI-L1（`normal` 档，§5.11）：评估 + 有限扫描，保留大牌、配合队友。
//!
//! 与 L0 的差别只在**在合法集里怎么挑**（不改变合法性，也不看对手手牌）：
//!
//! - **能一把走完就出完**（候选张数 == 手牌张数）——优先结束自己的回合；
//! - **领出**：排除炸弹后在候选里取「张数最多、同张数牌力最小」的一条。多张牌型先走（顺子 /
//!   连对 / 钢板 / 三带二），单张时出最小的那张，大牌与王自然被留到后面；
//! - **跟牌**：只压「同张数、非炸弹、牌力最小」的那条；没有同型候选时，只有遇到「对手压的
//!   是大牌（J 以上）」「对手本身就出的是炸弹」「自己手牌已很少（≤8 张）」三种情形才动炸弹，
//!   其余一律 `Pass` 把这手牌留给后面；
//! - **配合队友**：上一手是搭档出的（`BotContext::partner`，§5.11 的 `partner.state`）→
//!   默认 `Pass` 把牌权留给搭档；搭档这手不是控制牌且自己已进入 ≤8 张收尾区时，用最小
//!   同型非炸弹牌廉价反压；剩 ≤4 张能一把走完时直接接管。
//! - 炸弹与四王炸永远排在普通牌型之后被使用。

use std::cmp::Reverse;

use gf_engine::contract::Action;
use gf_table_runtime::{BotContext, BotPolicy, HandPlan};

use crate::l0::L0Bot;
use crate::shape::{
    bomb_count_in_hand, cards_key_of, count_of, is_bomb_shape, is_controller, rank_of, top_rank_of,
};
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
        choose(view_bytes, seat, legal, &BotContext::default(), false, None)
    }

    fn decide_with_ctx(
        &self,
        view_bytes: &[u8],
        seat: u8,
        legal: &[Action],
        ctx: &BotContext,
    ) -> Action {
        choose(view_bytes, seat, legal, ctx, false, None)
    }

    fn decide_with_plan(
        &self,
        view_bytes: &[u8],
        seat: u8,
        legal: &[Action],
        ctx: &BotContext,
        plan: Option<&HandPlan>,
    ) -> Action {
        choose(view_bytes, seat, legal, ctx, false, plan)
    }
}

/// L1/L2 共用的取舍逻辑。`memory = true` 时启用 L2 的记牌与炸弹时机（见 [`crate::L2Bot`]）。
///
/// `plan` 是 `recommend` 的整手拆分（房间按本座位手牌算好传进来）：
/// 领出照它的最小组走，跟牌避开拆组的候选，保证「先走小牌、不拆对子」。
pub(crate) fn choose(
    view: &[u8],
    seat: u8,
    legal: &[Action],
    ctx: &BotContext,
    memory: bool,
    plan: Option<&HandPlan>,
) -> Action {
    let hand = match view_scan::try_hand_cards(view) {
        Ok(hand) => hand,
        // 视图格式漂移时不要把「没解析到手牌」当成空手牌；退回合法集里最小的
        // 非空动作，至少不让 bot 因读不到手牌而静默 Pass。
        Err(_) => {
            return legal
                .iter()
                .filter(|a| count_of(a) > 0)
                .min_by_key(|a| (top_rank_of(a), count_of(a), cards_key_of(a)))
                .cloned()
                .unwrap_or(Action::Pass);
        }
    };
    let hand_len = hand.len();
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
        // 领出照推荐拆分的最小组走：整组一次出完，牌力最小的先走。
        if let Some(action) = plan.and_then(crate::plan::lead_from_plan) {
            return action;
        }
        return lead(&plays, view, &hand, memory);
    }

    // ② 配合队友：上一手是搭档出的，默认让牌；但搭档这手压不住场（不是控制牌）
    //    且自己已进入收尾区（≤8 张）时，用最小同型牌廉价反压，别把牌权白送对手。
    if let Some(partner) = ctx.partner {
        if view_scan::last_play_seat(view) == Some(partner) && hand_len > 4 {
            let last_cards = view_scan::last_play_cards(view);
            let partner_controls = memory
                && !last_cards.is_empty()
                && is_controller(
                    &view_scan::discard_cards(view),
                    &hand,
                    &Action::Play {
                        cards: last_cards.clone(),
                    },
                );
            if partner_controls || hand_len > 8 {
                return Action::Pass;
            }
            let cheap = same_shape(&plays, last_cards.len(), plan)
                .into_iter()
                .filter(|a| !is_bomb_shape(a))
                .min_by_key(|a| (top_rank_of(a), cards_key_of(a)))
                .cloned();
            if let Some(action) = cheap {
                return action;
            }
            return Action::Pass;
        }
    }

    let last_cards = view_scan::last_play_cards(view);
    let last_is_bomb = view_scan::last_play_pattern_id(view)
        .map(|id| id.starts_with("bomb") || id == "joker-bomb")
        .unwrap_or(false);
    follow(&plays, view, &hand, &last_cards, last_is_bomb, memory, plan)
}

/// 领出：优先多张牌型清手，同张数取牌力最小；炸弹留到最后。
///
/// 记牌档位（`memory`）额外把「出手即无人能压」的控制牌留到后面：先出能被压的，
/// 控制牌留作收尾牌权。
fn lead(plays: &[&Action], view: &[u8], hand: &[u16], memory: bool) -> Action {
    let normal: Vec<&Action> = plays
        .iter()
        .copied()
        .filter(|a| !is_bomb_shape(a))
        .collect();
    let pool: Vec<&Action> = if normal.is_empty() {
        plays.to_vec()
    } else {
        normal
    };
    if memory {
        let discarded = view_scan::discard_cards(view);
        if let Some(best) = pool
            .iter()
            .copied()
            .filter(|a| !is_controller(&discarded, hand, a))
            .min_by_key(|a| (Reverse(count_of(a)), top_rank_of(a), cards_key_of(a)))
        {
            return best.clone();
        }
    }
    pool.into_iter()
        .min_by_key(|a| (Reverse(count_of(a)), top_rank_of(a), cards_key_of(a)))
        .cloned()
        .unwrap_or(Action::Pass)
}

/// 跟牌：同张数最小代价先压；必要时才动炸弹。有推荐拆分时只留「不拆组」的候选。
fn follow(
    plays: &[&Action],
    view: &[u8],
    hand: &[u16],
    last_cards: &[u16],
    last_is_bomb: bool,
    memory: bool,
    plan: Option<&HandPlan>,
) -> Action {
    let need = last_cards.len();
    let candidates = same_shape(plays, need, plan);

    if !last_is_bomb {
        if let Some(best) = candidates
            .iter()
            .copied()
            .min_by_key(|a| (top_rank_of(a), cards_key_of(a)))
        {
            // 记牌档位：自己手牌不多时，若某条同型候选是「无人能压」的控制牌，先用它锁牌权。
            if memory && hand.len() <= 10 {
                let discarded = view_scan::discard_cards(view);
                if let Some(ctrl) = candidates
                    .iter()
                    .copied()
                    .filter(|a| is_controller(&discarded, hand, a))
                    .min_by_key(|a| (top_rank_of(a), cards_key_of(a)))
                {
                    return ctrl.clone();
                }
            }
            return best.clone();
        }
        // 清单里全是拆组的候选（典型：对子被拆成单张）：改从散张里挑一张能压的。
        if let Some(action) = plan.and_then(crate::plan::follow_single_from_plan) {
            return action;
        }
    } else if let Some(best) = candidates
        .iter()
        .copied()
        .min_by_key(|a| (top_rank_of(a), cards_key_of(a)))
    {
        // 对手是炸弹：同张数的非炸弹只能是同花顺（5 张炸之上），先试它。
        return best.clone();
    }

    let last_top = last_cards.iter().copied().map(rank_of).max().unwrap_or(0);
    // J(9) 以上视为「大牌」；对手已压大牌、或自己手牌 ≤8 张（该搏一手）才动炸弹。
    let mut worth_bomb = last_is_bomb || last_top >= 9 || hand.len() <= 8;
    if memory {
        // 记牌档位：按自己手上的炸弹数决定是否值得开炸。
        // 两个以上炸弹 → 更愿意（对手压 8 以上就开）；只剩一个且手牌还厚 → 留住。
        let bombs = bomb_count_in_hand(hand);
        if bombs >= 2 {
            worth_bomb = last_is_bomb || last_top >= 7 || hand.len() <= 8;
        } else if bombs == 1 && hand.len() > 14 && last_top < 11 {
            worth_bomb = false;
        }
    }
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

/// 同张数的非炸弹候选；给了推荐拆分就优先留「整组出」的，全被拆组时退回全量。
fn same_shape<'a>(plays: &[&'a Action], need: usize, plan: Option<&HandPlan>) -> Vec<&'a Action> {
    let all: Vec<&Action> = plays
        .iter()
        .copied()
        .filter(|a| count_of(a) == need && !is_bomb_shape(a))
        .collect();
    match plan {
        Some(p) => {
            let kept: Vec<&Action> = all
                .iter()
                .copied()
                .filter(|a| !crate::plan::splits_plan(crate::plan::action_cards(a), p))
                .collect();
            if kept.is_empty() {
                all
            } else {
                kept
            }
        }
        None => all,
    }
}
