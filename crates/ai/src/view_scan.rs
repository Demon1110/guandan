//! 只读扫描 `view(seat)` 的 canonical JSON 字节。
//!
//! 只认 view 白名单里与决策相关的两处：本座位 `hand` 区的牌 id 列表、`lastPlay` 是否存在。
//! 不做通用 JSON 解析（视图格式由 `gf_engine::view` 固定，键序由规则集 `view.fields` 定）。

/// 视图扫描失败：**只用于区分「格式漂移/截断」与「真的没有该字段」**。
///
/// 旧调用方继续用 [`hand_cards`] 的 `Vec` 兼容签名；bot 的关键路径改用
/// [`try_hand_cards`]，避免把「解析不到手牌」误当成「手牌为空」而直接 Pass。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanError {
    /// 找不到 `"hand":{"cards":[` 锚点。
    MissingHandAnchor,
    /// 找到锚点但列表没有闭合 `]`。
    UnterminatedHandList,
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingHandAnchor => f.write_str("找不到 hand.cards 锚点"),
            Self::UnterminatedHandList => f.write_str("hand.cards 列表未闭合"),
        }
    }
}

/// 严格版 [`hand_cards`]：锚点缺失/列表截断都返回错误，不再静默给空 Vec。
pub fn try_hand_cards(view: &[u8]) -> Result<Vec<u16>, ScanError> {
    let needle = b"\"hand\":{\"cards\":[";
    let start = view
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + needle.len())
        .ok_or(ScanError::MissingHandAnchor)?;
    let (cards, closed) = parse_u16_list_checked(&view[start..]);
    if !closed {
        return Err(ScanError::UnterminatedHandList);
    }
    Ok(cards)
}

/// 取 `hand` 区牌 id 列表（按视图里的原序返回，视图内已按牌 id 升序）。
///
/// 兼容旧签名：解析失败返回空 Vec；需要区分「空手牌」与「格式漂移」的调用方
/// 必须用 [`try_hand_cards`]。
pub fn hand_cards(view: &[u8]) -> Vec<u16> {
    let needle = b"\"hand\":{\"cards\":[";
    let Some(start) = view
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + needle.len())
    else {
        return Vec::new();
    };
    parse_u16_list(&view[start..])
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

/// `discard.cards`：公共弃牌堆（已出的所有牌 id）。无该字段时为空。
pub fn discard_cards(view: &[u8]) -> Vec<u16> {
    let anchor = b"\"discard\":";
    let Some(pos) = view.windows(anchor.len()).position(|w| w == anchor) else {
        return Vec::new();
    };
    let rest = &view[pos..];
    let needle = b"\"cards\":[";
    let Some(start) = rest
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + needle.len())
    else {
        return Vec::new();
    };
    parse_u16_list(&rest[start..])
}

/// `lastPlay.patternId`：上一手牌型 id（`bomb-4` / `joker-bomb` / `straight-5` …）。
pub fn last_play_pattern_id(view: &[u8]) -> Option<String> {
    let anchor = b"\"lastPlay\":";
    let pos = view.windows(anchor.len()).position(|w| w == anchor)?;
    let rest = &view[pos..];
    let needle = b"\"patternId\":\"";
    let start = rest
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + needle.len())?;
    let mut end = start;
    while end < rest.len() && rest[end] != b'"' {
        end += 1;
    }
    std::str::from_utf8(&rest[start..end])
        .ok()
        .map(str::to_owned)
}

/// `lastPlay.cards`：上一手的牌 id 列表（无 `lastPlay` 时为空）。
///
/// 只从 `lastPlay` 起点之后找 `"cards":[`，不会误取 `hand` 或 `discard` 的同名数组。
pub fn last_play_cards(view: &[u8]) -> Vec<u16> {
    let anchor = b"\"lastPlay\":";
    let Some(pos) = view.windows(anchor.len()).position(|w| w == anchor) else {
        return Vec::new();
    };
    let rest = &view[pos..];
    let needle = b"\"cards\":[";
    let Some(start) = rest
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + needle.len())
    else {
        return Vec::new();
    };
    parse_u16_list(&rest[start..])
}

/// 从 `[` 之后的字节里读到 `]` 为止，取所有十进制整数。
fn parse_u16_list(bytes: &[u8]) -> Vec<u16> {
    parse_u16_list_checked(bytes).0
}

/// 同 [`parse_u16_list`]，额外返回是否读到了闭合 `]`。
fn parse_u16_list_checked(bytes: &[u8]) -> (Vec<u16>, bool) {
    let mut out = Vec::new();
    let mut cursor = 0;
    let mut closed = false;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b']' => {
                closed = true;
                break;
            }
            b'0'..=b'9' => {
                let begin = cursor;
                while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                    cursor += 1;
                }
                if let Ok(card) = std::str::from_utf8(&bytes[begin..cursor])
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
    (out, closed)
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

    #[test]
    fn strict_hand_scan_distinguishes_empty_from_format_drift() {
        let empty = br#"{"hand":{"cards":[]}}"#;
        assert_eq!(try_hand_cards(empty), Ok(Vec::new()));

        let drifted = br#"{"seat":0}"#;
        assert_eq!(
            try_hand_cards(drifted),
            Err(ScanError::MissingHandAnchor),
            "锚点缺失不能伪装成空手牌"
        );

        let truncated = br#"{"hand":{"cards":[3,7"#;
        assert_eq!(
            try_hand_cards(truncated),
            Err(ScanError::UnterminatedHandList),
            "列表截断必须报错，不能只返回读到的前缀"
        );
    }
}
