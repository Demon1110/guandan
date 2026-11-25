//! `view_scan` 的畸形 / 截断输入：扫描器不做通用 JSON 解析，任何不完整视图都必须
//! 安全退化（返回空或 `None`），不允许 panic 或越界切片。

use gf_ai::view_scan::{discard_cards, hand_cards, has_last_play, last_play_seat};

#[test]
fn empty_and_truncated_views_degrade_to_empty() {
    for view in [
        b"".as_slice(),
        b"{}",
        b"\"hand\":{\"cards\":[",
        b"\"hand\":",
        b"\"discard\":",
    ] {
        assert!(hand_cards(view).is_empty(), "hand_cards({view:?}) 应为空");
        assert!(!has_last_play(view), "has_last_play({view:?}) 应为 false");
        assert_eq!(
            last_play_seat(view),
            None,
            "last_play_seat({view:?}) 应为 None"
        );
        assert!(
            discard_cards(view).is_empty(),
            "discard_cards({view:?}) 应为空"
        );
    }

    // `"lastPlay":` 这个键已经出现（哪怕值被截断），按 §5.12 口径就算「跟牌态」；
    // 座位号读不到则退回 None，不能默认 0。
    let truncated = b"\"lastPlay\":{\"seat\":".as_slice();
    assert!(has_last_play(truncated));
    assert_eq!(last_play_seat(truncated), None);
}

#[test]
fn hand_cards_reads_ids_and_skips_non_digits() {
    // 正常形态：逗号分隔的牌 id。
    assert_eq!(
        hand_cards(br#"{"hand":{"cards":[3,17,104]}}"#),
        vec![3, 17, 104]
    );
    // 数字之间夹了非数字 / 负数样式 → 跳过后继续；越 u16 的数值被丢弃。
    assert_eq!(
        hand_cards(br#"{"hand":{"cards":[5,"x",70000,8]}}"#),
        vec![5, 8]
    );
    // 缺少 needle → 空（不得把别处的数字当成手牌）。
    assert!(hand_cards(br#"{"discard":{"cards":[1,2,3]}}"#).is_empty());
}

#[test]
fn last_play_detection_and_seat_parsing() {
    assert!(has_last_play(br#"{"lastPlay":{"seat":2}}"#));
    assert_eq!(last_play_seat(br#"{"lastPlay":{"seat":2}}"#), Some(2));
    // 有键但座位号缺失 → None（不能默认 0）。
    assert_eq!(last_play_seat(br#"{"lastPlay":{"seat":null}}"#), None);
    // 座位号越 u8 → None。
    assert_eq!(last_play_seat(br#"{"lastPlay":{"seat":999}}"#), None);
    assert!(!has_last_play(br#"{"discard":{}}"#));
}

#[test]
fn discard_cards_reads_shared_pile() {
    assert_eq!(discard_cards(br#"{"discard":{"cards":[4,9]}}"#), vec![4, 9]);
    // 截断在 cards 数组之前 → 空。
    assert!(discard_cards(br#"{"discard":{"other":[]}}"#).is_empty());
    // 尾部没有 `]` 时，已读到的数字仍然保留（扫描器不要求闭合）。
    assert_eq!(
        discard_cards(br#"{"discard":{"cards":[12,34"#),
        vec![12, 34]
    );
}
