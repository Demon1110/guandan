//! 按 `recommend` 的手组牌出手：先出最小的组，不把对子拆成单张。
//!
//! `recommend` 把一整手牌拆成互不重叠的若干组（对子、三张、顺子、同花顺、炸弹…），
//! 这是「最快走掉」的最优拆分。bot 拿它做两件事：
//!
//! - **领出**：出牌力最小的那一手（整组一次出完，散张按单张出），而不是零碎地拆单张；
//! - **跟牌**：避开「只取了某个推荐组里一部分牌」的候选，把对子/顺子整组留着；
//!   单张跟牌优先用房间挑好的散张（`plan.follow_single`），挑不出就过牌。
//!
//! 牌力一律用 `HandPlan` 里引擎算好的 `power`（级牌提升、王最大），
//! 不按原始点数猜——那会把级牌当成小牌，出的手会被房间判非法。
//!
//! 拆分与合法集同源（本座位手牌 + 桌面公开信息），只读、确定性，不进 `stateHash`。

use gf_engine::contract::Action;
use gf_table_runtime::HandPlan;

/// 取动作里的牌（`Pass` 视为空）。
pub(crate) fn action_cards(action: &Action) -> &[u16] {
    match action {
        Action::Play { cards } => cards,
        Action::TributeReturn { .. } => &[],
        Action::Pass => &[],
    }
}

fn play(cards: Vec<u16>) -> Action {
    Action::Play { cards }
}

/// 炸弹 / 王炸：领出与跟牌的普通路径都排在最后考虑。
pub(crate) fn is_special(pattern_id: &str) -> bool {
    pattern_id.starts_with("bomb") || pattern_id == "joker-bomb"
}

/// 这一手是否把某个推荐组拆开（只取了组里的部分牌）。
///
/// 「对子拆成单张」正是这里要拦住的：一张牌单独出，而它在拆分里属于某个对子/顺子。
pub(crate) fn splits_plan(candidate: &[u16], plan: &HandPlan) -> bool {
    plan.groups.iter().any(|group| {
        let share = candidate
            .iter()
            .filter(|card| group.cards.contains(card))
            .count();
        share > 0 && share < group.cards.len()
    })
}

/// 领出：牌力最小的那一「手」—— 整组的牌一次出完，散张按单张出；
/// 只有整手都是炸弹时才动炸弹里最小的那一组。
pub(crate) fn lead_from_plan(plan: &HandPlan) -> Option<Action> {
    let mut units: Vec<(u32, Vec<u16>)> = Vec::new();
    for group in plan
        .groups
        .iter()
        .filter(|group| !is_special(&group.pattern_id) && !group.cards.is_empty())
    {
        units.push((group.power, group.cards.clone()));
    }
    for single in &plan.singles {
        units.push((single.power, vec![single.card]));
    }
    if units.is_empty() {
        // 手上只剩炸弹：先出最小的炸弹，盘面才有牌权。
        for group in plan.groups.iter().filter(|group| !group.cards.is_empty()) {
            units.push((group.power, group.cards.clone()));
        }
    }
    // 牌力升序；同牌力优先张数多的（一次走掉更多），再按牌 id 定序。
    units.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(b.1.len().cmp(&a.1.len()))
            .then(a.1.cmp(&b.1))
    });
    units.into_iter().next().map(|(_, cards)| play(cards))
}

/// 跟单张的兜底：房间已经把「散张里最小的、真能压过上家的」那张算好了。
///
/// 散张本来就没成型，出它不拆任何组；对子、顺子留着整组走。
pub(crate) fn follow_single_from_plan(plan: &HandPlan) -> Option<Action> {
    plan.follow_single.map(|card| play(vec![card]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gf_table_runtime::HandPlanCard;

    /// 牌 id 编码 `(suit×13+rank)×2+copy`；rank 0 = 2，rank 1 = 3，rank 5 = 7。
    const C_2S: u16 = 0; // 2♠
    const C_3S: u16 = 2; // 3♠
    const C_3S2: u16 = 3; // 3♠ 的第二副本
    const C_7H: u16 = 36; // 7♥

    fn pair_of_3() -> gf_table_runtime::HandPlanGroup {
        gf_table_runtime::HandPlanGroup {
            pattern_id: "pair".to_owned(),
            cards: vec![C_3S, C_3S2],
            power: 1,
        }
    }

    fn loose(card: u16, power: u32) -> HandPlanCard {
        HandPlanCard { card, power }
    }

    fn plan(groups: Vec<gf_table_runtime::HandPlanGroup>, singles: Vec<HandPlanCard>) -> HandPlan {
        HandPlan {
            groups,
            singles,
            single_count: 0,
            follow_single: None,
        }
    }

    #[test]
    fn lead_takes_smallest_unit_and_never_splits_a_pair() {
        // 散张 2♠（牌力 0）比对子 3（牌力 1）小：先走散张。
        let p = plan(vec![pair_of_3()], vec![loose(C_2S, 0)]);
        assert_eq!(lead_from_plan(&p), Some(Action::Play { cards: vec![C_2S] }));

        // 散张是 7♥（牌力 5）：先出整张对子，而不是把对子拆成一张 3 打出。
        let p = plan(vec![pair_of_3()], vec![loose(C_7H, 5)]);
        assert_eq!(
            lead_from_plan(&p),
            Some(Action::Play {
                cards: vec![C_3S, C_3S2]
            })
        );
    }

    #[test]
    fn lead_falls_back_to_bomb_only_when_nothing_else_is_left() {
        let p = plan(
            vec![gf_table_runtime::HandPlanGroup {
                pattern_id: "bomb-4".to_owned(),
                cards: vec![0, 1, 2, 3],
                power: 1,
            }],
            vec![],
        );
        assert_eq!(
            lead_from_plan(&p),
            Some(Action::Play {
                cards: vec![0, 1, 2, 3]
            })
        );
    }

    #[test]
    fn splits_plan_flags_only_partial_use_of_a_group() {
        let p = plan(vec![pair_of_3()], vec![loose(C_2S, 0)]);
        assert!(splits_plan(&[C_3S], &p), "从对子里抽一张就是拆组");
        assert!(!splits_plan(&[C_3S, C_3S2], &p), "整组拿走不算拆");
        assert!(!splits_plan(&[C_2S], &p), "散张本来就不在任何组里");
    }

    #[test]
    fn follow_single_uses_the_card_picked_by_the_room() {
        let mut p = plan(vec![pair_of_3()], vec![loose(C_2S, 0)]);
        // 房间在散张里挑不到能压过 7♥ 的，就只给 None：宁可过牌，也不拆对子。
        assert_eq!(follow_single_from_plan(&p), None);
        p.follow_single = Some(C_2S);
        assert_eq!(
            follow_single_from_plan(&p),
            Some(Action::Play { cards: vec![C_2S] })
        );
    }
}
