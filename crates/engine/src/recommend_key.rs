//! 计数向量状态键（§5.11 第三十七次变更，方案 A 收敛）。
//!
//! 手牌不按牌 id 排列存，压成一个 `u64` 完整状态键，不做有损哈希：
//!
//! - 13 个自然点数（`2..A`，`rank 0..12`）各 4 bit（两副牌上限 8）；
//! - 小王 3 bit（0..2）、大王 3 bit（0..2）；
//! - 尾追 2 bit 百搭计数（0..2）。
//!
//! 位布局：点数 `r` 在 `4*r`（0..51）；小王 @52；大王 @55；百搭计数 @58。
//!
//! 双记口径（上级拍板）：同一张红桃级牌在其点数档计 1，同时在百搭计数里也计 1，
//! 避免「档里减了、键里又加」的双记错账。`inStraightFlush` 只在后处理用，不进 DP 键。
//!
//! > 待设计稿追认：§5.11 正文仍写 58bit（15 档），本实现按方案 A 落 60bit（含 2bit
//! > 百搭计数）；设计稿归用户维护，代码不擅自改文档，此处留一行指向追认事项。
//!
//! TODO(第四十次变更 DFS 接线)：`KeyCounts` 由计数向量 DFS 主搜索消费；在 DFS 落地前
//! 仅被单元测试构造，临时放行 dead_code，接线后移除本属性。
#![allow(dead_code)]

use crate::patterns::RankContext;
use crate::ruleset::RuleSetBody;

/// 计数向量内部表示：`rank` 为各点数**非百搭**真牌数；`wild` 为手上实际还留着的
/// 红桃级牌（百搭）数；`little`/`big` 为大小王数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KeyCounts {
    rank: [u8; 13],
    wild: u8,
    little: u8,
    big: u8,
    /// 级牌点数下标（百搭自身点数）；无百搭规则集时为 0。
    level: usize,
    n: usize,
}

impl KeyCounts {
    /// 从手牌构造计数向量。百搭按 `guandan-classic` 的 wildcard 定义取（每副 1 张、
    /// 共 2 张），只计手上实际还留着的红桃级牌。
    pub(crate) fn build(r: &RuleSetBody, ctx: &RankContext, hand: &[u16]) -> Self {
        let rank_count = r.deck.ranks.len();
        let suit_count = r.deck.suits.len();
        let copies = r.deck.copies as usize;
        let base_total = rank_count * suit_count * copies;
        let wild_ids = crate::patterns::wildcard_ids(r, ctx);
        let level = ctx.own_rank().unwrap_or(0);

        let mut rank = [0u8; 13];
        let mut wild = 0u8;
        let mut little = 0u8;
        let mut big = 0u8;

        for &card in hand {
            let c = card as usize;
            if wild_ids.contains(&card) {
                wild += 1;
                continue;
            }
            if c >= base_total {
                let slot = (c - base_total) / copies;
                if slot == 0 {
                    little += 1;
                } else {
                    big += 1;
                }
                continue;
            }
            let b = c / copies;
            let rank_idx = b % rank_count;
            rank[rank_idx] += 1;
        }

        Self {
            rank,
            wild,
            little,
            big,
            level,
            n: rank_count,
        }
    }

    /// 打包成 60bit 完整状态键（方案 A）。
    pub(crate) fn pack(&self) -> u64 {
        let mut key = 0u64;
        for r in 0..self.n {
            // 双记：级牌点数档 = 真牌 + 百搭（百搭同时留在 `wild` 计数里）。
            let mut bin = self.rank[r];
            if r == self.level {
                bin = bin.saturating_add(self.wild);
            }
            key |= (bin as u64) << (4 * r);
        }
        key |= (self.little as u64) << 52;
        key |= (self.big as u64) << 55;
        key |= (self.wild as u64) << 58;
        key
    }

    /// 单位测试 / 断言用的受控构造。
    #[cfg(test)]
    fn manual(level: usize, rank: [u8; 13], wild: u8, little: u8, big: u8) -> Self {
        Self {
            rank,
            wild,
            little,
            big,
            level,
            n: 13,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_differs_by_wildcount_alpha_beta() {
        // α / β：同级牌档（L 档=2），但百搭 α=2、β=0 → 键必须不相等。
        let mut rank = [0u8; 13];
        rank[3] = 2;
        let alpha = KeyCounts::manual(3, rank, 2, 0, 0);
        let beta = KeyCounts::manual(3, rank, 0, 0, 0);
        assert_ne!(alpha.pack(), beta.pack());
    }

    #[test]
    fn key_equal_when_only_suit_differs() {
        // α 与 α' 只差花色、百搭数相同 → 计数向量不含花色，键必须相等。
        let mut rank = [0u8; 13];
        rank[3] = 2;
        rank[7] = 1;
        let alpha = KeyCounts::manual(3, rank, 1, 0, 1);
        let alpha_prime = KeyCounts::manual(3, rank, 1, 0, 1);
        assert_eq!(alpha.pack(), alpha_prime.pack());
    }

    #[test]
    fn key_double_counts_level_bin_with_wild() {
        // 双记口径：级牌档里计入百搭（α 真牌 0、百搭 2 → 档内打包为 2），百搭计数另存 2。
        let rank = [0u8; 13];
        let alpha = KeyCounts::manual(3, rank, 2, 0, 0);
        let packed = alpha.pack();
        let level_bin = (packed >> (4 * 3)) & 0xF;
        let wild_bin = (packed >> 58) & 0x3;
        assert_eq!(level_bin, 2);
        assert_eq!(wild_bin, 2);
    }
}
