// UI 控制层：开局弹框（新局/继续 + bot 难度）、左上角设置（主题/手牌/牌面）。
// 纯 DOM overlay，通过 window.__gf 调 Game/Scene；不侵入 Phaser canvas。
import { THEME_ORDER } from './skin.js';

const SAVE_KEY = 'gf-phaser-save';
const CFG_KEY = 'gf-phaser-cfg';

function $(id) { return document.getElementById(id); }

// 读取本地存档 / 配置（隐私模式下 localStorage 不可用则忽略）
function readJSON(key) {
  try { const s = localStorage.getItem(key); return s ? JSON.parse(s) : null; }
  catch { return null; }
}
function writeJSON(key, val) {
  try { localStorage.setItem(key, JSON.stringify(val)); } catch { /* 忽略 */ }
}

export function hasSave() {
  const s = readJSON(SAVE_KEY);
  return !!(s && s.v === 1 && Array.isArray(s.actions));
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

export function setupUI({ wasm, game, skin, scene, onStart }) {
  const cfg = readJSON(CFG_KEY) ?? {};
  const saved = readJSON(SAVE_KEY);

  // ---- 开局弹框 ----
  const startOverlay = $('start-overlay');
  const startMode = bindChoices('start-mode', 'new');
  const startBot = bindChoices('start-bot', saved?.botLevel ?? 2);
  const continueBtn = $('continue-btn');
  continueBtn.disabled = !hasSave();

  $('start-go').addEventListener('click', () => {
    const mode = startMode.value;
    const botLevel = Number(startBot.value);
    startOverlay.classList.remove('show');
    $('settings-btn').style.display = 'block';
    if (mode === 'continue' && hasSave()) {
      const ok = game.resume(readJSON(SAVE_KEY));
      if (!ok.ok) {
        // 恢复失败则回退到新局
        game.start({ botLevel });
      }
    } else {
      const hi = (Math.random() * 0xffffffff) >>> 0;
      const lo = (Math.random() * 0xffffffff) >>> 0;
      game.start({ seedHi: hi, seedLo: lo, humanSeat: 0, level: 2, botLevel });
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

  // ---- 自动存档：每次 game change 且不在发牌阶段时写盘 ----
  game.on('change', () => {
    if (game.phase === 'idle' || game.phase === 'dealing') return;
    writeJSON(SAVE_KEY, game.save());
  });

  // 首次启动：显示开局弹框
  startOverlay.classList.add('show');

  // 调试接口
  window.__gfUI = {
    startMode, startBot, setTheme, setHand, setSize, setTribute,
    hasSave, openStart: () => startOverlay.classList.add('show'),
    SAVE_KEY, CFG_KEY,
  };
}
