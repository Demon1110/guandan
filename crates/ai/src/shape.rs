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
