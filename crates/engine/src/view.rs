//! canonical view 编码与 `viewHash`（§5.12 第 809–829 行、第 836 行）。
//!
//! 编码六条固定规则：
//! ① 顶层对象，键序 = `view.fields` 白名单序（不是字母序、不是 map 迭代序）；
//! ② 单行、无空白、无缩进；
//! ③ 键名与枚举字面值只用 ASCII，键名不重复；
//! ④ 数组定序：牌 id 升序、seat 升序；
//! ⑤ 字符串 UTF-8、不做 NFC 归一化；
//! ⑥ 不写 `null`：字段缺值直接不出现；空区写 `"cards":[]` 且键不省略。
//!
//! 序列化自写 canonical 序列化器，禁止 `serde_json::Value` 拼装（§5.12 第 835 行）。

use crate::ruleset::model::ViewField;
use std::collections::BTreeMap;

/// `lastPlay` 视图值（§5.12 字段级编码表）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewLastPlay {
    pub seat: u8,
    pub pattern_id: String,
    pub cards: Vec<u16>,
}

/// 视图投影数据：键为 zoneId；`seat_levels` 为 `None` 时该键不出现。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewData {
    pub zones: BTreeMap<String, Vec<u16>>,
    pub seat_levels: Option<Vec<u8>>,
    pub last_play: Option<ViewLastPlay>,
}

fn escape_json_string(out: &mut Vec<u8>, s: &str) {
    out.push(b'"');
    for ch in s.chars() {
        match ch {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            c if (c as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes());
            }
            c => out.extend_from_slice(c.to_string().as_bytes()),
        }
    }
    out.push(b'"');
}

fn write_card_ids(out: &mut Vec<u8>, cards: &[u16]) {
    let mut sorted = cards.to_vec();
    sorted.sort_unstable();
    out.push(b'[');
    for (i, &card) in sorted.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.extend_from_slice(card.to_string().as_bytes());
    }
    out.push(b']');
}

/// 按白名单序把 [`ViewData`] 编码为 canonical JSON bytes。
pub fn encode_canonical(fields: &[ViewField], data: &ViewData) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(b'{');
    let mut first = true;

    let push_pair = |out: &mut Vec<u8>, key: &str, first: &mut bool| {
        if !*first {
            out.push(b',');
        }
        *first = false;
        escape_json_string(out, key);
        out.push(b':');
    };

    for field in fields {
        match field {
            ViewField::Zone(zone) => {
                let cards = data.zones.get(&zone.0).cloned().unwrap_or_default();
                push_pair(&mut out, &zone.0, &mut first);
                out.push(b'{');
                out.extend_from_slice(b"\"cards\":");
                write_card_ids(&mut out, &cards);
                out.push(b'}');
            }
            ViewField::SeatLevels => {
                let Some(levels) = &data.seat_levels else {
                    continue; // 无队伍级牌状态时该键不出现（§5.12 第 818 行）
                };
                push_pair(&mut out, "seat.levels", &mut first);
                // 按队伍序号升序——顺序即队索引，不按值排序（§5.12 第 818 行）。
                out.push(b'[');
                for (i, &level) in levels.iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    out.extend_from_slice(level.to_string().as_bytes());
                }
                out.push(b']');
            }
            ViewField::LastPlay => {
                let Some(last) = &data.last_play else {
                    continue; // 本回合无人出过牌时该键不出现（§5.12 第 819 行）
                };
                push_pair(&mut out, "lastPlay", &mut first);
                out.push(b'{');
                out.extend_from_slice(b"\"seat\":");
                out.extend_from_slice(last.seat.to_string().as_bytes());
                out.push(b',');
                out.extend_from_slice(b"\"patternId\":");
                escape_json_string(&mut out, &last.pattern_id);
                out.push(b',');
                out.extend_from_slice(b"\"cards\":");
                write_card_ids(&mut out, &last.cards);
                out.push(b'}');
            }
        }
    }

    out.push(b'}');
    out
}

/// `viewHash(seat)` = BLAKE3-256(canonical view bytes + `seat` u8 LE + `roomSeq` u64 LE)
/// 取前 8 字节（§5.12 第 804–807 行）。
pub fn view_hash(seat: u8, room_seq: u64, view_bytes: &[u8]) -> [u8; 8] {
    let mut msg = Vec::from(view_bytes);
    msg.extend_from_slice(&seat.to_le_bytes());
    msg.extend_from_slice(&room_seq.to_le_bytes());
    let hash = blake3::hash(&msg);
    let mut out = [0u8; 8];
    out.copy_from_slice(&hash.as_bytes()[..8]);
    out
}

/// 前 8 字节 → 16 个十六进制字符（§5.12 第 837 行）。
pub fn present_8(hash: &[u8; 8]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_zone_keeps_empty_cards_array() {
        let fields = vec![ViewField::Zone(crate::contract::ZoneId("hand".to_owned()))];
        let data = ViewData::default();
        let bytes = encode_canonical(&fields, &data);
        assert_eq!(bytes, br#"{"hand":{"cards":[]}}"#);
    }

    /// P0 回归：`seat.levels` 按队索引保序，值互换 canonical bytes 与 viewHash 都必须不同
    /// （§5.12 第 818 行）。
    #[test]
    fn seat_levels_order_is_significant() {
        let fields = vec![ViewField::SeatLevels];
        let d1 = ViewData {
            seat_levels: Some(vec![2, 12]),
            ..Default::default()
        };
        let d2 = ViewData {
            seat_levels: Some(vec![12, 2]),
            ..Default::default()
        };
        let b1 = encode_canonical(&fields, &d1);
        let b2 = encode_canonical(&fields, &d2);
        assert_ne!(b1, b2);
        assert_ne!(view_hash(0, 1, &b1), view_hash(0, 1, &b2));
    }
}
