// 入口：加载 wasm → 加载主题皮肤（可选）→ 建 Game → 建 Phaser 实例。
import { Game } from './game.js';
import { H, TableScene, W } from './scenes/TableScene.js';
import { Skin } from './skin.js';
import { WasmBridge } from './wasm.js';

async function boot() {
  const status = document.getElementById('boot-status');
  try {
    const wasm = new WasmBridge();
    await wasm.load();
    status.textContent = 'wasm 已加载 · 正在载入主题…';

    const game = new Game(wasm);
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

    // 主题异步加载：失败不影响牌桌（回退字卡）。
    // 初始主题：?theme= > localStorage > 默认 squabby。
    // fetch 是异步的，返回时 Phaser 的 create() 早已跑完，textures 可用。
    const themeKey = Skin.initialThemeKey();
    skin.load(scene, themeKey).then((r) => {
      if (r.ok) scene.setThemeLabel?.(r.label);
    });

    // 调试与自动化冒烟用：挂到 window 便于脚本驱动。
    window.__gf = { wasm, game, skin, scene };
    status.style.display = 'none';
  } catch (err) {
    status.textContent = `加载失败：${err.message}`;
    status.style.color = '#ff9a8a';
    console.error(err);
  }
}

boot();
