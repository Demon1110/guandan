//! 牌面语义类型。与 engine 的 card id（`(suit*rank_count+rank)*copies+copy`）解耦：
//! 调用方负责把牌 id 反解成这里的 `Face`，本 crate 只认语义。

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 花色。顺序固定为 ♠ ♥ ♦ ♣，与 engine 的 suit 索引 0..=3 一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Suit {
    Spade = 0,
    Heart = 1,
    Diamond = 2,
    Club = 3,
}

impl Suit {
    pub const ALL: [Suit; 4] = [Suit::Spade, Suit::Heart, Suit::Diamond, Suit::Club];

    pub fn index(self) -> usize {
        self as usize
    }

    /// 主题素材里的目录键。
    pub fn key(self) -> &'static str {
        match self {
            Suit::Spade => "spade",
            Suit::Heart => "heart",
            Suit::Diamond => "diamond",
            Suit::Club => "club",
        }
    }

    /// 牌面编码里的单字母前缀。
    pub fn code(self) -> char {
        match self {
            Suit::Spade => 'S',
            Suit::Heart => 'H',
            Suit::Diamond => 'D',
            Suit::Club => 'C',
        }
    }

    /// 红桃 / 方块用红墨，其余用黑墨。
    pub fn is_red(self) -> bool {
        matches!(self, Suit::Heart | Suit::Diamond)
    }

    pub fn from_code(c: char) -> Option<Suit> {
        match c.to_ascii_uppercase() {
            'S' => Some(Suit::Spade),
            'H' => Some(Suit::Heart),
            'D' => Some(Suit::Diamond),
            'C' => Some(Suit::Club),
            _ => None,
        }
    }
}

/// 点数 1..=13（1=A、11=J、12=Q、13=K）。掼蛋里 2 不是天然最大，大小由 `levelRank` 决定，故此处不做排序语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rank(u8);

impl Rank {
    pub const ACE: Rank = Rank(1);
    pub const TWO: Rank = Rank(2);
    pub const KING: Rank = Rank(13);

    pub fn new(value: u8) -> Result<Rank, crate::error::DeckError> {
        if (1..=13).contains(&value) {
            Ok(Rank(value))
        } else {
            Err(crate::error::DeckError::FaceInvalid(format!(
                "点数越界: {value}"
            )))
        }
    }

    pub fn value(self) -> u8 {
        self.0
    }

    /// `ranks/` 目录里的素材序号（0=A … 12=K）。
    pub fn index(self) -> usize {
        (self.0 - 1) as usize
    }

    pub fn label(self) -> &'static str {
        const LABELS: [&str; 13] = [
            "A", "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K",
        ];
        LABELS[self.index()]
    }

    pub fn from_label(text: &str) -> Option<Rank> {
        let upper = text.trim().to_ascii_uppercase();
        (1u8..=13)
            .map(Rank)
            .find(|r| r.label().eq_ignore_ascii_case(&upper))
            .or_else(|| upper.parse::<u8>().ok().and_then(|v| Rank::new(v).ok()))
    }
}

impl Serialize for Rank {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(self.0)
    }
}

impl<'de> Deserialize<'de> for Rank {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Rank, D::Error> {
        let v = u8::deserialize(d)?;
        Rank::new(v).map_err(serde::de::Error::custom)
    }
}

/// 一个牌面。`Joker` 不分花色、不参与级牌判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Normal { suit: Suit, rank: Rank },
    Joker { big: bool },
}

impl Face {
    pub fn normal(suit: Suit, rank: Rank) -> Face {
        Face::Normal { suit, rank }
    }

    /// 稳定编码：`H8` / `S10` / `JOKER-B` / `JOKER-S`，同时用作导出文件名。
    pub fn code(&self) -> String {
        match self {
            Face::Normal { suit, rank } => format!("{}{}", suit.code(), rank.label()),
            Face::Joker { big: true } => "JOKER-B".to_string(),
            Face::Joker { big: false } => "JOKER-S".to_string(),
        }
    }

    pub fn parse(text: &str) -> Result<Face, crate::error::DeckError> {
        let raw = text.trim();
        let upper = raw.to_ascii_uppercase().replace(' ', "");
        match upper.as_str() {
            "JOKER-B" | "BJ" | "BIGJOKER" | "JOKER-BIG" | "JK-B" | "大王" => {
                return Ok(Face::Joker { big: true })
            }
            "JOKER-S" | "SMALLJOKER" | "JOKER-SMALL" | "JK-S" | "小王" => {
                return Ok(Face::Joker { big: false })
            }
            _ => {}
        }
        let mut chars = raw.chars();
        let suit_char = chars
            .next()
            .ok_or_else(|| crate::error::DeckError::FaceInvalid("空牌面编码".into()))?;
        let suit = Suit::from_code(suit_char).ok_or_else(|| {
            crate::error::DeckError::FaceInvalid(format!("花色无效: {suit_char}"))
        })?;
        let rank_text: String = chars.collect();
        if rank_text.is_empty() {
            return Err(crate::error::DeckError::FaceInvalid(format!(
                "缺少点数: {raw}"
            )));
        }
        let rank = Rank::from_label(&rank_text).ok_or_else(|| {
            crate::error::DeckError::FaceInvalid(format!("点数无效: {rank_text}"))
        })?;
        Ok(Face::Normal { suit, rank })
    }
}

/// 一副牌的 54 个面（不含牌背）：♠♥♦♣ × A..K，再加大王、小王。
pub fn all_faces() -> Vec<Face> {
    let mut out = Vec::with_capacity(54);
    for suit in Suit::ALL {
        for value in 1u8..=13 {
            out.push(Face::Normal {
                suit,
                rank: Rank::new(value).expect("1..=13 恒有效"),
            });
        }
    }
    out.push(Face::Joker { big: false });
    out.push(Face::Joker { big: true });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_code_roundtrip() {
        for face in all_faces() {
            let code = face.code();
            let parsed = Face::parse(&code).expect("应能解析自身编码");
            assert_eq!(parsed, face, "编码 {code} 往返失败");
        }
    }

    #[test]
    fn face_parse_is_case_insensitive() {
        assert_eq!(
            Face::parse("h10").unwrap(),
            Face::Normal {
                suit: Suit::Heart,
                rank: Rank::new(10).unwrap()
            }
        );
        assert_eq!(Face::parse("jk-b").unwrap(), Face::Joker { big: true });
        assert_eq!(Face::parse("小王").unwrap(), Face::Joker { big: false });
    }

    #[test]
    fn face_parse_rejects_garbage() {
        for bad in ["", "X9", "H0", "H14", "H", "99"] {
            assert!(Face::parse(bad).is_err(), "{bad} 不应被接受");
        }
    }

    #[test]
    fn rank_boundaries() {
        assert!(Rank::new(0).is_err());
        assert!(Rank::new(14).is_err());
        assert_eq!(Rank::new(1).unwrap().label(), "A");
        assert_eq!(Rank::new(13).unwrap().label(), "K");
        assert_eq!(Rank::new(11).unwrap().index(), 10);
    }

    #[test]
    fn all_faces_has_54() {
        let faces = all_faces();
        assert_eq!(faces.len(), 54);
        assert_eq!(
            faces
                .iter()
                .filter(|f| matches!(f, Face::Joker { .. }))
                .count(),
            2
        );
    }
}
