// 入口：加载 wasm → 强制加载 gfdeck 主题 → 建 Game → 建 Phaser 实例。
// 主题是硬依赖：加载失败不进入游戏、不回退自画字卡。
import { Game } from './game.js';
import { H, TableScene, W } from './scenes/TableScene.js';
import { Skin } from './skin.js';
import { ReplayStore } from './store.js';
import { setupUI } from './ui.js';
import { WasmBridge } from './wasm.js';

async function boot() {
  const status = document.getElementById('boot-status');
  try {
    const wasm = new WasmBridge();
    await wasm.load();
    status.textContent = 'wasm 已加载 · 正在载入主题…';

    // 回放存档：动作日志 + 场次元信息存 IndexedDB，由 Game 在每手落盘、恢复时重演。
    const store = new ReplayStore();
    const game = new Game(wasm, store);
    const skin = new Skin(wasm);
    const scene = new TableScene(game, skin);

    new window.Phaser.Game({
      type: window.Phaser.AUTO,
      width: W,
      height: H,
      parent: 'game-root',
      backgroundColor: '#0b3f2a',
      scale: {
        mode: window.Phaser.Scale.FIT,
        autoCenter: window.Phaser.Scale.CENTER_BOTH,
      },
      scene: [scene],
    });

    // 主题强制加载：必须读取一个 gfdeck 主题，失败则中止，不再回退自画字卡。
    // 初始主题：?theme= > localStorage > 默认 squabby。
    // fetch 是异步的，返回时 Phaser 的 create() 早已跑完，textures 可用。
    const themeKey = Skin.initialThemeKey();
    const themeReady = skin.load(scene, themeKey);
    const themeResult = await themeReady;
    if (!themeResult.ok) {
      // 主题不可用：牌桌无法渲染，明确报错并停止（不建开局弹框、不挂调试入口）。
      throw new Error(`gfdeck 主题加载失败（${themeResult.error ?? '未知原因'}）：本游戏需要有效的 .gfdeck 主题包`);
    }
    scene.setThemeLabel?.(themeResult.label);
    // 牌堆改用 gfdeck 牌背纹理（buildTable 时纹理尚未登记，这里补建）。
    scene.refreshDeckStack();
    // 存到 scene 上：发牌前可 await，保证第一张飞牌起就是主题牌面。
    scene.themeReady = Promise.resolve(themeResult);

    // 调试与自动化冒烟用：挂到 window 便于脚本驱动。
    window.__gf = { wasm, game, skin, scene, store };

    // 开局弹框 / 设置面板：不再自动开局，由用户选择新局或继续。
    await setupUI({ wasm, game, skin, scene, store });
    status.style.display = 'none';
  } catch (err) {
    status.textContent = `加载失败：${err.message}`;
    status.style.color = '#ff9a8a';
    console.error(err);
  }
}

boot();
