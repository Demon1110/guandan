// UI 控制层：开局弹框（新局/继续 + bot 难度）、左上角设置（主题/手牌/牌面）。
// 纯 DOM overlay，通过 window.__gf 调 Game/Scene；不侵入 Phaser canvas。
// 存档走 IndexedDB（动作日志 + 场次元信息），localStorage 只留配置与旧档迁移兜底。
import { rankCode } from './cards.js';
import { THEME_ORDER } from './skin.js';
import { matchIdOf } from './store.js';

const LEGACY_SAVE_KEY = 'gf-phaser-save';
const CFG_KEY = 'gf-phaser-cfg';

function $(id) { return document.getElementById(id); }

// 读取本地配置 / 旧存档（隐私模式下 localStorage 不可用则忽略）
function readJSON(key) {
  try { const s = localStorage.getItem(key); return s ? JSON.parse(s) : null; }
  catch { return null; }
}
function writeJSON(key, val) {
  try { localStorage.setItem(key, JSON.stringify(val)); } catch { /* 忽略 */ }
}

/** 是否有可续的存档：IndexedDB 优先，旧 localStorage 存档作为迁移兜底。 */
export async function hasSave(game) {
  return !!(await game.hasReplaySave()) || !!readJSON(LEGACY_SAVE_KEY);
}

/**
 * 旧版 localStorage 存档一次性迁移到 IndexedDB：
 * 老格式只有当前局动作、没有 seq/roundNo/哈希基线，按 roundNo=1 写入，
 * 之后新动作继续追加到同一场次，刷新即可按动作日志回放。
 */
async function migrateLegacy(store) {
  const old = readJSON(LEGACY_SAVE_KEY);
  if (!old || old.v !== 1 || !Array.isArray(old.actions) || !old.actions.length) return null;
  const meta = {
    matchId: matchIdOf(old.seedHi, old.seedLo, old.humanSeat ?? 0),
    seedHi: old.seedHi >>> 0,
    seedLo: old.seedLo >>> 0,
    humanSeat: old.humanSeat ?? 0,
    humanMask: 1 << (old.humanSeat ?? 0),
    botLevel: old.botLevel ?? 2,
    level: old.level ?? 2,
    lastHash: '',
    lastRoomSeq: old.actions.length,
    lastRoundNo: 1,
    updatedAt: Date.now(),
  };
  await store.putMatch(meta);
  for (let i = 0; i < old.actions.length; i += 1) {
    const a = old.actions[i];
    await store.appendAction(meta.matchId, {
      seq: i + 1,
      roundNo: 1,
      seat: a.seat,
      pass: !!a.pass,
      tributeReturn: !!a.tributeReturn,
      patternId: a.patternId ?? '',
      cards: a.cards ?? [],
    }, null);
  }
  try { localStorage.removeItem(LEGACY_SAVE_KEY); } catch { /* 忽略 */ }
  return { meta, actions: old.actions };
}

// 绑定一组选项按钮的选择状态，返回当前选值。
function bindChoices(containerId, initial) {
  const box = $(containerId);
  let value = initial;
  const buttons = [...box.querySelectorAll('button[data-v]')];
  const paint = () => buttons.forEach((b) => b.classList.toggle('sel', b.dataset.v === String(value)));
  buttons.forEach((b) => b.addEventListener('click', () => { value = b.dataset.v; paint(); }));
  paint();
  return {
    get value() { return value; },
    set(v) { value = String(v); paint(); },
    disable(v, disabled) {
      const b = buttons.find((x) => x.dataset.v === v);
      if (b) b.disabled = disabled;
    },
  };
}

export async function setupUI({ wasm, game, skin, scene, store, onStart }) {
  await store.open();

  // 检测 IndexedDB 存档；没有就把旧 localStorage 存档迁过来。
  let replay = await game.hasReplaySave();
  if (!replay) replay = await migrateLegacy(store);

  const cfg = readJSON(CFG_KEY) ?? {};

  // ---- 开局弹框 ----
  const startOverlay = $('start-overlay');
  const startMode = bindChoices('start-mode', 'new');
  const startBot = bindChoices('start-bot', replay?.meta?.botLevel ?? 2);
  const continueBtn = $('continue-btn');
  continueBtn.disabled = !replay;
  const note = $('save-note');
  if (note) {
    note.textContent = replay
      ? `检测到存档：${replay.actions.length} 条动作 · 第 ${replay.meta.lastRoundNo ?? 1} 局，选「继续上一局」按动作回放续牌。`
      : '没有可续的存档，将开新局。';
  }

  function startNew(botLevel) {
    const hi = (Math.random() * 0xffffffff) >>> 0;
    const lo = (Math.random() * 0xffffffff) >>> 0;
    game.start({ seedHi: hi, seedLo: lo, humanSeat: 0, level: 2, botLevel });
  }

  $('start-go').addEventListener('click', async () => {
    const mode = startMode.value;
    const botLevel = Number(startBot.value);
    startOverlay.classList.remove('show');
    $('settings-btn').style.display = 'block';
    if (mode === 'continue' && replay) {
      const r = await game.resumeFromDb(replay.meta.matchId, async () => {
        // 恢复跳过发牌动画：必须先把当前级牌的 54 张主题纹理预热，否则同步渲染手牌会抛错。
        await scene.themeReady;
        await scene.skin.warmup(rankCode(game.levelRankLabel()));
      });
      if (!r.ok) {
        // 哈希自证失败 / 回放失败：绝不带着错误状态继续，明确告知并回退新局。
        console.warn('[replay] 恢复失败，回退新局：', r.reason);
        startNew(botLevel);
      }
    } else {
      startNew(botLevel);
    }
    onStart?.();
  });

  // ---- 设置弹框 ----
  const settingsOverlay = $('settings-overlay');
  const setTheme = bindChoices('set-theme', skin.themeId ?? THEME_ORDER[0]);
  const setHand = bindChoices('set-hand', cfg.handStyle ?? 'cols');
  const setSize = bindChoices('set-size', String(cfg.cardScale ?? 1));
  const setTribute = bindChoices('set-tribute', String(cfg.tributeSeconds ?? 15));
  // 还贡限时启动时即应用（读档/新局都受影响），无需等设置面板点完成。
  scene.setTributeSeconds(Number(cfg.tributeSeconds ?? 15));

  // 应用一次配置（手牌方式/牌面大小/还贡限时）到场景
  function applyCfg() {
    scene.setHandStyleDirect(setHand.value);
    scene.setCardScale(Number(setSize.value));
    scene.setTributeSeconds(Number(setTribute.value));
    writeJSON(CFG_KEY, {
      handStyle: setHand.value,
      cardScale: Number(setSize.value),
      tributeSeconds: Number(setTribute.value),
    });
  }

  // 打开时快照当前生效状态，供"取消"回滚（主题是即时切换的，必须记下来）。
  let snapshot = null;
  $('settings-btn').addEventListener('click', () => {
    snapshot = {
      theme: skin.themeId ?? THEME_ORDER[0],
      hand: scene.handStyle ?? 'cols',
      size: String(scene.cardScale ?? 1),
      tribute: String(scene.tributeSeconds ?? 15),
    };
    setTheme.set(snapshot.theme);
    setHand.set(snapshot.hand);
    setSize.set(snapshot.size);
    setTribute.set(snapshot.tribute);
    settingsOverlay.classList.add('show');
  });
  // 完成：应用当前选择并关闭。
  $('settings-close').addEventListener('click', async () => {
    await scene.setThemeById(setTheme.value);
    applyCfg();
    settingsOverlay.classList.remove('show');
  });
  // 取消：回滚到打开前的状态并关闭（不写盘，保留原配置）。
  $('settings-cancel').addEventListener('click', async () => {
    if (snapshot) {
      await scene.setThemeById(snapshot.theme);
      setHand.set(snapshot.hand);
      setSize.set(snapshot.size);
      setTribute.set(snapshot.tribute);
      scene.setHandStyleDirect(snapshot.hand);
      scene.setCardScale(Number(snapshot.size));
      scene.setTributeSeconds(Number(snapshot.tribute));
      writeJSON(CFG_KEY, {
        handStyle: snapshot.hand,
        cardScale: Number(snapshot.size),
        tributeSeconds: Number(snapshot.tribute),
      });
    }
    settingsOverlay.classList.remove('show');
  });

  // 主题按钮即时生效（点击即切，避免只有关面板才生效造成困惑）
  const themeBox = $('set-theme');
  themeBox.querySelectorAll('button[data-v]').forEach((b) => {
    b.addEventListener('click', async () => {
      if (b.dataset.v !== skin.themeId) await scene.setThemeById(b.dataset.v);
    });
  });

  // 首次启动：显示开局弹框
  startOverlay.classList.add('show');

  // 调试接口
  window.__gfUI = {
    startMode, startBot, setTheme, setHand, setSize, setTribute,
    hasSave: () => hasSave(game),
    openStart: () => startOverlay.classList.add('show'),
    get replay() { return replay; },
    SAVE_KEY: LEGACY_SAVE_KEY, CFG_KEY,
  };
}
