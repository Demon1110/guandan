// 牌 id 编码与引擎一致：`(suit × 13 + rank) × 2 + copy`，suit 0..3 = ♠♥♦♣，rank 0..12 = 2..A；
// 王 id 从 104 起：104/105 小王，106/107 大王（两副牌）。此文件只做纯函数，不碰 wasm。
export const RANKS = ['2', '3', '4', '5', '6', '7', '8', '9', '10', 'J', 'Q', 'K', 'A'];
export const SUITS = ['♠', '♥', '♦', '♣'];
export const JOKER_BASE = 104;

export function cardFace(id) {
  if (id >= JOKER_BASE) {
    return {
      id,
      rank: id < JOKER_BASE + 2 ? '小' : '大',
      suit: '王',
      red: id >= JOKER_BASE + 2,
      joker: true,
      rankIdx: id < JOKER_BASE + 2 ? 13 : 14,
    };
  }
  const base = Math.floor(id / 2);
  const rankIdx = base % 13;
  const suitIdx = Math.floor(base / 13);
  return {
    id,
    rank: RANKS[rankIdx],
    suit: SUITS[suitIdx],
    red: suitIdx === 1 || suitIdx === 2,
    joker: false,
    rankIdx,
    suitIdx,
  };
}

export function cardLabel(id) {
  const f = cardFace(id);
  return f.joker ? f.rank + '王' : f.rank + f.suit;
}

// 手牌排序键：大王 > 小王 > 级牌 > A > K > … > 3（同点数按花色、copy 稳定）。
// levelRank 为级牌点数标签（'2'…'A'），无级牌传 null。
export function columnKey(id, levelRank) {
  const f = cardFace(id);
  if (f.joker) return f.rankIdx === 14 ? 1000 : 999;
  if (f.rank === levelRank) return 998;
  let pos = 0;
  for (let i = 0; i < 13; i += 1) {
    if (RANKS[i] === levelRank) continue;
    if (i === f.rankIdx) return 100 + pos;
    pos += 1;
  }
  return 100;
}

export function sortHand(cards, levelRank) {
  return [...cards].sort((a, b) => {
    const ka = columnKey(a, levelRank);
    const kb = columnKey(b, levelRank);
    if (ka !== kb) return kb - ka; // 键大靠左
    const fa = cardFace(a);
    const fb = cardFace(b);
    if (fa.suitIdx !== fb.suitIdx) return (fa.suitIdx ?? 9) - (fb.suitIdx ?? 9);
    return a - b;
  });
}

// 点数标签 → deckart Rank 编号（1=A、2=2…13=K）；null → 0。只给主题渲染用。
export function rankCode(label) {
  if (!label) return 0;
  if (label === 'A') return 1;
  if (label === 'J') return 11;
  if (label === 'Q') return 12;
  if (label === 'K') return 13;
  return Number(label);
}

// 牌 id → deckart Face code（gf_theme_card 的入参）。S/H/D/C + 点数；王为 JOKER-B/S。
const CODE_SUIT = ['S', 'H', 'D', 'C'];
const CODE_RANK = ['2', '3', '4', '5', '6', '7', '8', '9', '10', 'J', 'Q', 'K', 'A'];
export function faceCode(id) {
  if (id >= JOKER_BASE) return id >= JOKER_BASE + 2 ? 'JOKER-B' : 'JOKER-S';
  const base = Math.floor(id / 2);
  return CODE_SUIT[Math.floor(base / 13)] + CODE_RANK[base % 13];
}
