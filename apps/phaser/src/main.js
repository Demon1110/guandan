// 入口：加载 wasm → 建 Game → 建 Phaser 实例。
import { Game } from './game.js';
import { H, TableScene, W } from './scenes/TableScene.js';
import { WasmBridge } from './wasm.js';

async function boot() {
  const status = document.getElementById('boot-status');
  try {
    const wasm = new WasmBridge();
    await wasm.load();
    status.textContent = 'wasm 已加载';

    const game = new Game(wasm);
    const scene = new TableScene(game);

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

    // 调试与自动化冒烟用：挂到 window 便于脚本驱动。
    window.__gf = { wasm, game, scene };
    status.style.display = 'none';
  } catch (err) {
    status.textContent = `加载失败：${err.message}`;
    status.style.color = '#ff9a8a';
    console.error(err);
  }
}

boot();
