//! 牌与动作的轻量形状判定（AI 侧，不依赖引擎内部判定链）。
//!
//! 只用于**排序与取舍**（选哪一条合法动作），不用于判定合法性——合法性由房间用引擎
//! 的 `legal_actions` 保证。因此这里允许近似：用「张数 + 是否同点数」识别炸弹，
//! 不处理逢人配补位造成的「结构炸弹但点数不同」的少数情形（那类候选只会被当作普通牌
//! 参与排序，不会产生非法动作）。

use gf_engine::contract::Action;

/// 牌面点数：`0..=12` 对应 `2..A`，小王 `13`、大王 `14`。
///
/// 与引擎牌 id 编码一致：`id = (suit × 13 + rank) × 2 + copy`；王 id ≥ 104。
pub fn rank_of(card: u16) -> u16 {
    if card >= 104 {
        if card < 106 {
            13
        } else {
            14
        }
    } else {
        (card / 2) % 13
    }
}

fn is_joker(card: u16) -> bool {
    card >= 104
}

/// 动作的牌列表（`Pass` 为空）。
pub fn cards_of(action: &Action) -> &[u16] {
    match action {
        Action::Play { cards } => cards,
        Action::Pass => &[],
    }
}

/// 动作张数。
pub fn count_of(action: &Action) -> usize {
    cards_of(action).len()
}

/// 动作的顶牌点数（排序用；`Pass` 记 0）。
pub fn top_rank_of(action: &Action) -> u16 {
    cards_of(action)
        .iter()
        .copied()
        .map(rank_of)
        .max()
        .unwrap_or(0)
}

/// 牌 id 序列（同点数下的稳定次序）。
pub fn cards_key_of(action: &Action) -> Vec<u16> {
    let mut v = cards_of(action).to_vec();
    v.sort_unstable();
    v
}

/// 结构上是否为炸弹：4–8 张同点数，或四张王（两小王 + 两大王）。
///
/// 同花顺（5 张同花色连牌）不在此列——它不是炸弹，按 `legal_actions` 的牌型阶梯另行压牌。
pub fn is_bomb_shape(action: &Action) -> bool {
    let cards = cards_of(action);
    if cards.len() < 4 {
        return false;
    }
    if cards.len() == 4 && cards.iter().all(|c| is_joker(*c)) {
        return true;
    }
    if cards.len() > 8 {
        return false;
    }
    let first = rank_of(cards[0]);
    cards.iter().all(|c| !is_joker(*c) && rank_of(*c) == first)
}

/// 该点数在整副牌里的总张数（两副牌：非王每点 8 张，大小王各 2 张）。
pub fn total_of_rank(rank: u16) -> usize {
    if rank >= 13 {
        2
    } else {
        8
    }
}

/// 记牌：按点数统计 `discard`（公共弃牌堆）里已出的张数，返回长度 15 的计数（下标即点数）。
///
/// 这是 AI-L2「记牌」的唯一数据来源——`discard` 是公开信息，不含任何人的手牌。
pub fn dealt_rank_counts(discarded: &[u16]) -> [u16; 15] {
    let mut counts = [0u16; 15];
    for &card in discarded {
        counts[rank_of(card) as usize] += 1;
    }
    counts
}

/// 记牌：估算「除自己手牌外，场上还剩多少张比 `rank` 大的牌」。
///
/// `discarded` 为公共弃牌堆，`hand` 为自己的手牌。返回 0 即：这手牌出去以后没人能压
/// （自己手里更大的牌不算对手的）。级牌/逢人配不参与张数统计，只按自然点数近似。
pub fn remaining_bigger(discarded: &[u16], hand: &[u16], rank: u16) -> usize {
    let dealt = dealt_rank_counts(discarded);
    let mut remain = 0usize;
    for r in (rank + 1)..=14u16 {
        remain += total_of_rank(r).saturating_sub(dealt[r as usize] as usize);
    }
    let own_bigger = hand.iter().filter(|c| rank_of(**c) > rank).count();
    remain.saturating_sub(own_bigger)
}

/// 记牌：这条动作出手后是否「无人能压」（顶牌点数是当前场上最大）。
pub fn is_controller(discarded: &[u16], hand: &[u16], action: &Action) -> bool {
    if cards_of(action).is_empty() {
        return false;
    }
    remaining_bigger(discarded, hand, top_rank_of(action)) == 0
}

/// 手牌里的炸弹个数（按点数 ≥4 张计，四王另算）。用于 AI-L2 的炸弹时机判断。
pub fn bomb_count_in_hand(hand: &[u16]) -> usize {
    if hand.is_empty() {
        return 0;
    }
    let mut by_rank = [0u8; 15];
    for &card in hand {
        by_rank[rank_of(card) as usize] += 1;
    }
    let same_rank = by_rank.iter().filter(|&&n| n >= 4).count();
    let jokers = by_rank[13] + by_rank[14];
    same_rank + usize::from(jokers >= 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(cards: Vec<u16>) -> Action {
        Action::Play { cards }
    }

    #[test]
    fn rank_matches_engine_encoding() {
        // suit 0 rank 0 copy 0 → id 0 → "2"；rank 12 → id 24 → "A"。
        assert_eq!(rank_of(0), 0);
        assert_eq!(rank_of(24), 12);
        assert_eq!(rank_of(104), 13);
        assert_eq!(rank_of(106), 14);
    }

    #[test]
    fn bomb_shape_detection() {
        // 四张 5：suit0 rank3 → 6/7，suit1 rank3 → 32/33。
        assert!(is_bomb_shape(&play(vec![6, 7, 32, 33])));
        // 四王炸（两小王 + 两大王）。
        assert!(is_bomb_shape(&play(vec![104, 105, 106, 107])));
        // 三张同点不是炸弹。
        assert!(!is_bomb_shape(&play(vec![6, 7, 32])));
        // 五张不同点不是炸弹。
        assert!(!is_bomb_shape(&play(vec![6, 7, 32, 33, 10])));
    }

    #[test]
    fn pass_is_empty() {
        assert_eq!(count_of(&Action::Pass), 0);
        assert_eq!(top_rank_of(&Action::Pass), 0);
        assert!(!is_bomb_shape(&Action::Pass));
    }
}
