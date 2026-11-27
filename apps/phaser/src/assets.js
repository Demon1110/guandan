// 素材清单：所有装饰性图片素材的替换入口。
// 当前 mode='programmatic' 表示用程序化绘制占位（零外部依赖、file:// 可双击播放）。
// 后期按 UI 设计图替换：把对应项改为 { mode:'image', url:'assets/xxx.png' }，
// TableScene 会通过 resolveAsset() 自动改用 this.add.image() 贴图，无需改渲染逻辑。

export const ASSETS = {
  // 桌面分区背景（海天 → 沙滩）
  seaTop: { mode: 'programmatic', color: 0x8fd0e8 },
  seaMid: { mode: 'programmatic', color: 0x6fc3d8 },
  seaLow: { mode: 'programmatic', color: 0x63b6c4 },
  sandTop: { mode: 'programmatic', color: 0xd9c9a3 },
  sandMid: { mode: 'programmatic', color: 0xe8d8b4 },
  sandLow: { mode: 'programmatic', color: 0xdfcca2 },

  // 头像框：自己 / bot
  avatarSelf: { mode: 'programmatic', glyph: '我', color: 0xf0c751, text: '#3b2a10' },
  avatarBot: { mode: 'programmatic', glyph: '掼', color: 0xf0c751, text: '#3b2a10' },

  // 级牌徽标 / 倍数牌（牌背走 gfdeck 主题纹理，不再自画）
  levelBadge: { mode: 'programmatic', color: 0xf3a017 },
  multiplier: { mode: 'programmatic', color: 0x27424a },

  // 倒计时环：正常 / 告急
  countdownOk: { mode: 'programmatic', color: 0xf3c34a },
  countdownLow: { mode: 'programmatic', color: 0xe24b4a },

  // 中央场次水印
  watermark: { mode: 'programmatic', text: '掼蛋 · 单机练习场', sub: '经典场 · 底分 800' },
};

// 渲染层统一入口：返回 { kind:'image', url } 或 { kind:'prog', spec }。
export function resolveAsset(key) {
  const spec = ASSETS[key];
  if (!spec) return { kind: 'prog', spec: {} };
  if (spec.mode === 'image' && spec.url) return { kind: 'image', url: spec.url };
  return { kind: 'prog', spec };
}
