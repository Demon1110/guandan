// 四色同花顺推荐的最小回归：直接从 app.js 里取纯函数片段，不启动浏览器。
// 重点盯「红桃级牌补位」和「同一手牌落进两个窗口」两件事。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { runInNewContext } from 'node:vm';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, 'app.js'), 'utf8');
const start = source.indexOf('// 该花色手牌里所有');
const end = source.indexOf('// 该花色「可用的牌池」');
assert(start >= 0 && end > start, '找不到 flushWindows 源码片段');
const snippet = source.slice(start, end);

function makeHelpers(levelIdx) {
  const context = {
    JOKER_BASE: 104,
    levelRankIndex: () => levelIdx,
    suitIndexOf: (id) => {
      if (id >= 104) return id < 106 ? 4 : 5;
      return Math.floor(Math.floor(id / 2) / 13);
    },
    rankIndexOf: (id) => (id >= 104 ? -1 : Math.floor(id / 2) % 13),
    cardLabel: (id) => {
      const ranks = ['2', '3', '4', '5', '6', '7', '8', '9', '10', 'J', 'Q', 'K', 'A'];
      const suits = ['♠', '♥', '♦', '♣'];
      if (id >= 104) return id < 106 ? '小王' : '大王';
      const base = Math.floor(id / 2);
      return ranks[base % 13] + suits[Math.floor(base / 13)];
    },
    flushWindows: null,
    straightFlushSuit: null,
  };
  runInNewContext(`${snippet}\nglobalThis.flushWindows = flushWindows;\nglobalThis.straightFlushSuit = straightFlushSuit;`, context);
  return context;
}

const labels = (option) => option.cards.map((id) => {
  const ranks = ['2', '3', '4', '5', '6', '7', '8', '9', '10', 'J', 'Q', 'K', 'A'];
  const suits = ['♠', '♥', '♦', '♣'];
  const base = Math.floor(id / 2);
  return ranks[base % 13] + suits[Math.floor(base / 13)];
});

// 截图牌例：级牌 6，红配 6♥ + 5♣6♣7♣8♣。
// 红配可以当 4 也可以当 9，落到同一组牌时只保留 5-9 这一手（牌力更大）。
const level6 = makeHelpers(4);
const wildHand = level6.flushWindows(3, [34, 84, 86, 88, 90]);
assert.equal(wildHand.length, 1, '红配补位应只给出一组去重后的方案');
assert.equal(wildHand[0].start, 3);
assert.equal(wildHand[0].wild, 1);
assert.equal(labels(wildHand[0]).join(' '), '5♣ 6♣ 7♣ 8♣ 6♥');
assert.equal(level6.straightFlushSuit(wildHand[0].cards), 3, '理牌组应识别为梅花');

// 没有红配时，四张连续不能凑成同花顺；补一张 9♣ 后才出现 5-9。
assert.equal(level6.flushWindows(3, [84, 86, 88, 90]).length, 0);
const natural = level6.flushWindows(3, [84, 86, 88, 90, 92]);
assert.equal(natural.length, 1);
assert.equal(natural[0].wild, 0);
assert.equal(labels(natural[0]).join(' '), '5♣ 6♣ 7♣ 8♣ 9♣');

// A-2-3-4-5 轮子也要能列出（A♣ = 102，旧实现用 id >= 100 会把它漏掉）。
const wheel = level6.flushWindows(3, [78, 80, 82, 84, 102]);
assert.equal(wheel.length, 1);
assert.equal(wheel[0].start, 12);
assert.equal(wheel[0].wheel, true);
assert.equal(labels(wheel[0]).join(' '), 'A♣ 2♣ 3♣ 4♣ 5♣');

console.log(JSON.stringify({
  ok: true,
  wild: labels(wildHand[0]),
  natural: labels(natural[0]),
  wheel: labels(wheel[0]),
}, null, 2));
