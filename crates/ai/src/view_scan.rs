//! 只读扫描 `view(seat)` 的 canonical JSON 字节。
//!
//! 只认 view 白名单里与决策相关的两处：本座位 `hand` 区的牌 id 列表、`lastPlay` 是否存在。
//! 不做通用 JSON 解析（视图格式由 `gf_engine::view` 固定，键序由规则集 `view.fields` 定）。

/// 取 `hand` 区牌 id 列表（按视图里的原序返回，视图内已按牌 id 升序）。
pub fn hand_cards(view: &[u8]) -> Vec<u16> {
    let needle = b"\"hand\":{\"cards\":[";
    let Some(start) = view
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + needle.len())
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut cursor = start;
    while cursor < view.len() {
        match view[cursor] {
            b']' => break,
            b'0'..=b'9' => {
                let begin = cursor;
                while cursor < view.len() && view[cursor].is_ascii_digit() {
                    cursor += 1;
                }
                if let Ok(card) = std::str::from_utf8(&view[begin..cursor])
                    .unwrap_or("")
                    .parse::<u16>()
                {
                    out.push(card);
                }
                continue;
            }
            _ => cursor += 1,
        }
    }
    out
}

/// 本回合是否已有人出过牌（`lastPlay` 键存在即为「跟牌态」，§5.12 第 819 行）。
pub fn has_last_play(view: &[u8]) -> bool {
    let needle = b"\"lastPlay\"";
    view.windows(needle.len()).any(|w| w == needle)
}

/// `lastPlay.seat`：上一手出牌座位（视图里有 `lastPlay` 时才返回 `Some`）。
pub fn last_play_seat(view: &[u8]) -> Option<u8> {
    let needle = b"\"lastPlay\":{\"seat\":";
    let start = view.windows(needle.len()).position(|w| w == needle)? + needle.len();
    let mut end = start;
    while end < view.len() && view[end].is_ascii_digit() {
        end += 1;
    }
    if end == start {
        return None;
    }
    std::str::from_utf8(&view[start..end]).ok()?.parse().ok()
}

/// 本座位当前是否「跟牌」（要压过别人的牌）。
///
/// 关键口径：**回合被全员过牌清空后，引擎把出牌权交回最后出牌人，`lastPlay` 仍保留**
/// （用于视图展示）。所以 `lastPlay.seat == 自己` 时其实是领出，不是跟牌；
/// 只有 `lastPlay` 来自他座时才是跟牌。这条只看视图即可判定，不需要引擎状态。
pub fn is_following(view: &[u8], seat: u8) -> bool {
    last_play_seat(view).is_some_and(|s| s != seat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_hand_and_last_play_flag() {
        let view = br#"{"hand":{"cards":[3,7,11]},"seat.levels":[0,0],"lastPlay":{"seat":2,"patternId":"single","cards":[42]},"round.level":0}"#;
        assert_eq!(hand_cards(view), vec![3, 7, 11]);
        assert!(has_last_play(view));
        assert_eq!(last_play_seat(view), Some(2));
        // 他座出牌 → 跟牌态。
        assert!(is_following(view, 0));
        // 自己出牌且回合已清空交回 → 是领出，不是跟牌。
        assert!(!is_following(view, 2));
    }

    #[test]
    fn empty_hand_and_no_last_play() {
        let view = br#"{"hand":{"cards":[]},"seat.levels":[0,0],"round.level":0}"#;
        assert!(hand_cards(view).is_empty());
        assert!(!has_last_play(view));
        assert_eq!(last_play_seat(view), None);
        assert!(!is_following(view, 0));
    }
}
