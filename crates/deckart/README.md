# gf-deckart · 牌面主题工具包

把「一套素材」按 `{suit, rank, levelRank}` 动态合成 54 张牌面（52 张 + 大小王）+ 牌背，输出 SVG（默认），必要时转 PNG。
主题以目录区分，可打包成带加密防护的 `.gfdeck` 素材包分发。

**已实现并通过 28 个测试**（状态与用法见第 13 节；决策见第 12 节）。

---

## 1. 定位与边界

| 做 | 不做 |
| --- | --- |
| 分层合成牌面、级牌/逢人配徽章、主题加载与校验 | 牌型判定、逢人配展开（属 `crates/engine`，`recognize` 枚举 13 种替代角色） |
| 素材包打包/解包/加密/签名/完整性校验 | 协议帧、`viewSchemaVersion` 定义（属 `crates/protocol`） |
| SVG 输出与 PNG 转换 | 牌桌布局、动画、点击判定（属 `apps/client`） |
| 素材许可与元数据登记 | 主题市场、用户自定义皮肤（v1 不做） |

三条既有约束（`docs/开发计划.md` M2 段，2026-11-24 裁定）必须遵守：

1. PixiJS 8 不原生渲染 SVG → 必须能 rasterize，缓存键 `(themeId, themeVersion, dpr, faceSize)`；
2. 牌面只表现自然牌面，**级牌高亮与逢人配标记必须是独立语义叠加层**，不得烘进模板底图；
3. `themeId + themeVersion` 管主题缓存失效，**不得并入 `viewSchemaVersion`**（后者是服务端白名单版本）。

本 crate 与 `stateHash` / 协议 / 引擎零耦合：输入只有牌面语义与服务端下发的 `levelRank`，输出只有图。

---

## 2. 素材清单（一套 = 23 个文件）

| # | 层 | 素材 | 数量 | 说明 |
| --- | --- | --- | --- | --- |
| L1 | 底面 | `base.svg` | 1 | 白底圆角矩形模板，含内描边与纸面阴影；所有牌共用 |
| L2 | 花色 | `suits/{spade,heart,diamond,club}.svg` | 4 | 纯符号，不带底色；用于角标与中央 |
| L3 | 点数 | `ranks/*` | 13 | `A,2..10,J,Q,K` 的字形；形态二选一：矢量（`1.svg`..`13.svg`）或图集（`ranks.png` + `ranks.json` 坐标表） |
| L4 | 级牌徽章 | `badges/level.svg` | 1 | 红色描边/发光边框 + 右上胶囊（胶囊内三条白色小三角作图标；「级牌」二字由可选文本层画），**通用一套，不随点数变** |
| L4 | 逢人配徽章 | `badges/wild.svg` | 1 | 红桃小徽章，叠在 level 之上 |
| — | 王 | `jokers/{small,big}.svg` | 2 | 独立素材，与级牌无关（王永远不是级牌） |
| — | 牌背 | `back.svg` | 1 | 独立素材 |

合计 1+4+13+2+1+1+2+1 = **23**。54 张牌 = 这 23 个素材的组合，不做整张贴图。

> 掼蛋两副牌（`copies=2`）共 108 张实体牌，但**只有 54 个面**：多副复用同一面，副本号不参与渲染。

---

## 3. 四层合成模型

```
L1 底面模板（裁圆角） → L2 花色符号 → L3 点数/角标 → L4 级牌徽章（+ 逢人配徽章）
```

判定（**全部来自服务端数据，客户端不自行推断**）：

- `isLevel = rank == levelRank`（且不是王）
- `isWild  = isLevel && suit == HEART`（红桃级牌 = 逢人配）
- `levelRank` 由 view 的 `round.level`（本局局级牌档位）与 `round.attackingTeam`（打级队）解析后下发，见设计稿第二十二次变更；断线重连与中途入座也能拿到，不靠客户端推理

级牌与逢人配只改变 L4 的叠加，**素材永远一套**：升级后 2→3、3→4 只是 `levelRank` 变，不新增素材、不重新出图。

### 布局（`layout` 由 `theme.json` 指定）

- `compact`：左上角标（点数 + 花色竖排）+ 中央大花色 + 右下角标镜像（参考图口径）
- `classic`：四角标 + 中央花阵（真实扑克排布，见下文「花阵规范」），A/J/Q/K 走中央大符号 + 四角标

两种布局共用同一套 23 个素材与同一套图层 id，只换 `layout` 与花阵配置即可切换。

### 素材固有尺寸与坐标规范

交付牌面 SVG 时**画布固定 500×700（5:7）**，所有子素材的固有尺寸与放置规则如下（新增主题照此产出，代码只认 `theme.json`）：

| 素材 | 固有宽高 | 在牌面上的用法 | 摆放锚点 |
| --- | --- | --- | --- |
| 牌面输出 | 500 × 700（5:7） | `viewBox="0 0 500 700"`、`width/height=500/700`；转 PNG 只调 `scale` | — |
| `base.svg` | 500 × 700 | 整幅覆盖，圆角 36（`canvas.cornerRadius`）+ 内描边 | 原点 `(0,0)` |
| `suits/*.svg` | 100 × 100 | 角标小花、中央大花、classic 花阵单元 | 左上角 + `scale` |
| `ranks/*.svg` | 100 × 140（5:7） | 角标点数、classic 四角点数 | 左上角 + `scale`；`stroke-width=18` 保证缩到 0.40 倍仍清晰 |
| `jokers/*.svg` | 500 × 700 | 整幅独立图（王冠 + 星形，纯图形不写字） | 原点 |
| `back.svg` | 500 × 700 | 整幅牌背 | 原点 |
| `badges/level.svg` | 500 × 700 | 描边/发光整幅层 + 右上胶囊（胶囊 `x 248..484`、`y 16..78`） | 原点 |
| `badges/wild.svg` | 120 × 120 | 逢人配小徽章 | 左上角 + `scale` |

放置语义（`theme.json` 的 `slots`）：`x/y` 是**素材左上角**，实际尺寸 = 固有宽高 × `scale`。
例：`centerSuit = {x:155, y:242, scale:1.9}` → 100×100 放大到 190px，左上角落在 (155,242)，即中心 (250,337)。
`classic` 的四角用同一语义，额外自动旋转：右上 `translate(W-x, y+h·s)`、左下 `translate(x+w·s, H-y)`、右下 `translate(W-x, H-y)`，三者统一 `rotate(180)` 让字头朝牌心。

> 改素材尺寸必须同步本表与 `theme.json`；只改配色/形状则不用动配置。

### 花阵规范（`layout = classic`）

`theme.json` 的 `classic` 块就是完整规范，代码里没有硬编码点位：

```json
"classic": {
  "cornerRank": { "x": 34, "y": 30, "scale": 0.40 },
  "cornerSuit": { "x": 38, "y": 98, "scale": 0.30 },
  "pipScale": 0.60,
  "aceScale": 1.70,
  "courtStyle": "monogram",
  "pipPositions": {
    "2": [[0.5, 0.18], [0.5, 0.82]],
    "3": [[0.5, 0.16], [0.5, 0.5], [0.5, 0.84]],
    ……
    "10": [[0.28, 0.14], [0.72, 0.14], [0.5, 0.26], [0.28, 0.38], [0.72, 0.38], [0.28, 0.62], [0.72, 0.62], [0.5, 0.74], [0.28, 0.86], [0.72, 0.86]]
  }
}
```

- `pipPositions` 是**归一化中心坐标**（0..1），数量必须等于点数（2→2 个、10→10 个），否则加载期报 `DK-THEME-INVALID`；
- 因为归一化，换画布尺寸（如以后 600×840）只改 `canvas`，点位表不用重写；
- A 用 `aceScale` 单个大符号；J/Q/K 的 `courtStyle = "monogram"` 也是单个大符号，`"figure"` 是为将来人物素材留的 schema 位（当前不支持）；
- 四角标无条件绘制，花阵只影响中央。

### 可选文本层（`textLayer`）

`badges.level` 与文本层是两个独立的东西：前者是素材（描边/胶囊/图标），后者是字。`textLayer.badge` 决定谁出场，三选一：

```json
"textLayer": {
  "enabled": true,
  "badge": "both",
  "fontFamily": "'Microsoft YaHei', 'PingFang SC', 'Noto Sans SC', sans-serif",
  "levelLabel": { "text": "级牌", "x": 404, "y": 58, "size": 28, "color": "#FFFFFF", "anchor": "middle" }
}
```

| `textLayer` | `enabled` | `badge` | `L4-level` 素材层 | `L4t-level-label` 文字层 | 适用场景 |
| --- | --- | --- | --- | --- | --- |
| 缺省 | — | — | 画 | 不画 | 纯图形、零字体依赖 |
| 有 | `false` | 任意（含 text-only） | 画 | 不画 | 临时关掉文字，行为回到旧版 |
| 有 | `true` | `badge-only` | 画 | 不画 | 素材里已经画了「级牌」字形的主题 |
| 有 | `true` | `both`（默认） | 画 | 画 | 胶囊由素材给、字由文本层补（defaults） |
| 有 | `true` | `text-only` | **不画** | 画 | 主题自带上色底板/描边，只需要补字 |

几点约束与坑：

- `enabled` 是总开关：关掉时无论 `badge` 填什么，都退回「只画素材」，不会出现「级牌完全没标记」的空档；
- `badge = both` / `text-only` 且 `enabled` 时必须给 `levelLabel`（否则加载期 `DK-THEME-INVALID`）；`badge-only` 时 `levelLabel` 可省，写了也会照常校验；
- `text-only` 只读文本层，**不再画 `badges.level`（连同它的描边和胶囊）**。defaults 的 `levelLabel` 是白色字、位置固定在胶囊里，单独用会变成白底白字——真要用 `text-only`，主题必须自己提供底板（或把 `color` 改成与纸面对比的深色）；
- 逢人配徽章 `L4b-wild` 是独立层，三种模式都照常出现，不受 `badge` 影响；
- 文字层嵌入的是纯文本（XML 转义，不解析任何标记）；转 PNG 时加载系统字体（`fontdb::load_system_fonts`），系统缺中文字体则该层退化成空白，牌面其余部分不受影响；
- Web/WebView 直接渲染 SVG 时由系统字体兜底，不引入 `@font-face`、不外链字体文件。

### 牌背（当前形态）

`back.svg` 是一整幅独立素材，直接铺满，不叠任何层。目前的图案偏素，等后续主题提供更复杂的前后背景时，再在同一素材位上替换，接口不变。

---

## 4. 输入契约

本 crate **不解析 engine 的 card id**（`(suit*rank_count+rank)*copies+copy` 依赖 RuleSet）。调用方（`crates/client-wasm`）负责把牌 id 反解成语义字段：

```rust
pub enum Suit { Spade, Heart, Diamond, Club }
pub enum Rank { Ace, Two, .., Ten, Jack, Queen, King }   // 或 1..=13 的索引
pub enum Face { Normal { suit: Suit, rank: Rank }, Joker { big: bool } }

pub struct LevelState { pub level_rank: Rank }           // 服务端下发

pub fn render(theme: &Theme, face: Face, level: Option<LevelState>) -> SvgString
```

`level: None` 表示本局无级牌状态（`score-only` 局），此时不出 L4。

---

## 5. 主题目录

```
themes/
├── defaults/                  # 内置主题（源目录，入 git）
│   ├── theme.json
│   ├── base.svg
│   ├── suits/{spade,heart,diamond,club}.svg
│   ├── ranks/{1..13}.svg      # 或 ranks.png + ranks.json
│   ├── jokers/{small,big}.svg
│   ├── back.svg
│   └── badges/{level,wild}.svg
└── emerald.gfdeck             # 打包产物（分发用，可加密）
```

### `theme.json`（现行 schema）

```json
{
  "id": "defaults",
  "version": 2,
  "name": "默认（深绿）",
  "license": "CC0-1.0",
  "canvas": { "width": 500, "height": 700, "cornerRadius": 36 },
  "palette": { "red": "#C62828", "black": "#1B1B1B", "paper": "#FFFFFF", "level": "#E53935", "wild": "#D32F2F" },
  "layout": "compact",
  "layers": {
    "base": "base.svg",
    "suits": { "spade": "suits/spade.svg", … },
    "ranks": { "kind": "vector", "files": ["ranks/1.svg", "…", "ranks/13.svg"] },
    "jokers": { "small": "jokers/small.svg", "big": "jokers/big.svg" },
    "back": "back.svg"
  },
  "badges": { "level": "badges/level.svg", "wild": "badges/wild.svg" },
  "slots": {
    "cornerRank": { "x": 30, "y": 34, "scale": 0.42 },
    "cornerSuit": { "x": 36, "y": 112, "scale": 0.34 },
    "centerSuit": { "x": 155, "y": 242, "scale": 1.90 },
    "wildBadge": { "x": 26, "y": 560, "scale": 0.60 }
  },
  "textLayer": { "enabled": true, "badge": "both", "fontFamily": "…", "levelLabel": { "text": "级牌", "x": 404, "y": 58, "size": 28, "color": "#FFFFFF", "anchor": "middle" } },
  "classic": { "cornerRank": {…}, "cornerSuit": {…}, "pipScale": 0.60, "aceScale": 1.70, "courtStyle": "monogram", "pipPositions": {…} }
}
```
`textLayer` 与 `classic` 都是可选块（缺省用默认值/关闭），完整字段含义见第 3 节的尺寸规范、花阵规范与文本层说明。
`version` 是主题版本号，用来让客户端缓存失效：加了文本层与 classic 配置后 defaults 已从 1 升到 2。

`ranks.kind = "atlas"` 时改为 `{ "kind": "atlas", "image": "ranks.png", "index": "ranks.json" }`，`index` 给每字形的 `{x,y,w,h,advance}`，兼容后续位图字库。
校验规则（L1 级）：`id` 只允许 `[a-z0-9-]`、`version ≥ 1`、`id+version` 唯一、引用的素材文件必须存在且非空、素材路径不得越出主题目录（禁 `..`、禁绝对路径）。

---

## 6. 输出约定（SVG）

```xml
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 500 700" width="500" height="700"
     data-theme="defaults" data-theme-version="2" data-face="H8" data-level="8"
     data-leveled="true" data-wild="true" data-layout="compact">
  <g id="L1-base">…</g>
  <g id="L2-suit">…</g>
  <g id="L3-rank">…</g>
  <g id="L4-level">…</g>          <!-- 仅 isLevel -->
  <g id="L4t-level-label">…</g>   <!-- 仅 isLevel 且 textLayer.enabled -->
  <g id="L4b-wild">…</g>          <!-- 仅 isWild -->
</svg>
```

- 图层 id 固定，便于客户端/测试/AI 视觉回归定位
- `data-*` 属性携带生死判定结果，方便 E2E 断言「这张确实是级牌」
- 生成器写一行注释 `<!-- gf-deckart 0.1.0 theme=defaults@1 -->`
- 不引外部字体、不引外链、不用 CSS 变量（保证单文件可独立渲染）

**默认全用 SVG。** 只有目标环境不支持 SVG（PixiJS 纹理、某些导出场景）才转 PNG：`to_png(svg, scale)` 走 `resvg`，`scale = dpr × faceSize/500`。

---

## 7. `.gfdeck` 包格式（草案）

```
+0   magic      "GFDK" (4B)
+4   u16        format version (=1)
+6   u16        flags: bit0 zstd 压缩 / bit1 加密 / bit2 签名
+8   u32        header_len
+12  header     JSON（明文）：themeId、themeVersion、license、entries[]
+..  payload    entries 顺序排列，每条独立密文
+..  签名        Ed25519(header || payload)（flags.bit2 时存在）
```

`header.entries[i] = { name, offset, len, sha256, nonce }`，每条用 AEAD（AES-256-GCM 或 ChaCha20-Poly1305）单独加密，好处：可按需只解一张牌用到的素材，不必整包解密进内存。

### 加密方案（待选，见第 12 节问题 1）

| 方案 | 做法 | 挡住谁 | 代价 |
| --- | --- | --- | --- |
| A 内置密钥 | 密钥编译期注入（常量拆分 + 混淆），AES-256-GCM | 直接解包/改后缀看素材的人 | 逆向前端仍可提取；离线可用 |
| B 双模式 | 离线走内置密钥；登录后服务端按会话下发 `themeKey` 优先使用 | 逆向门槛明显抬高 | 需服务端配合；离线包必须仍可读 |
| C 仅混淆 | 自定义容器 + XOR/置换，不引入 AEAD | 同上但更弱 | 实现最简、体积最小 |

诚实说明：**客户端必然持有解密能力**，「防破解」只能提高门槛、不能绝对防护。B 是在不牺牲离线的前提下性价比最高的做法。签名（Ed25519）解决的是「包被篡改」，与加密正交，建议无论如何都做。

---

## 8. CLI（草案）

```powershell
gfdeck render --theme themes/defaults --face H8 --level 8 -o out/H8.svg
gfdeck render-all --theme themes/defaults --level 8 -o out/          # 54 张 + 牌背
gfdeck png out/H8.svg --scale 2 -o out/H8@2x.png
gfdeck pack   themes/defaults -o themes/defaults.gfdeck --encrypt --key-file k.bin
gfdeck unpack themes/defaults.gfdeck -o /tmp/decoded --key-file k.bin
gfdeck verify themes/defaults.gfdeck                                 # 摘要 + 签名 + schema 校验
gfdeck info   themes/defaults.gfdeck                                 # themeId/version/条目/许可
```

`render-all` 产 55 个文件（54 面 + 1 牌背），并写 `manifest.json`（face → 文件 → sha256）。

---

## 9. 测试与验收口径

| 项 | 判据 |
| --- | --- |
| 面覆盖 | 54 面 + 牌背全部可渲染，无缺素材、无未闭合标签 |
| 分层断言 | `data-level` / `data-wild` 与输入一致；王永远不是级牌；红桃级牌同时有 level + wild 两层 |
| 快照 | `defaults` 主题 55 张 SVG 文本哈希固定（改素材必须显式更新快照） |
| 包往返 | `pack → unpack → render` 产出与直接读目录逐字节一致 |
| 坏包 | 截断/改一字节/换 themeId/签名不符/路径穿越 全部拒绝并给明确错误码 |
| 性能 | 单张合成 P99 < 1 ms（native），55 张 < 20 ms；包加载（含解密）< 10 ms |
| 覆盖率 | 并入 workspace 后按现行门禁（行 ≥90% + 区域 ≥90%） |

---

## 10. 依赖计划

- 必选：`serde` / `serde_json`（theme.json、包 header）、`thiserror`
- 加密：`aes-gcm` 或 `chacha20poly1305`（按方案定）、`sha2`、`ed25519-dalek`（签名）
- 压缩：`zstd`（flags.bit0）
- PNG：`resvg` + `tiny-skia`（仅 CLI/服务端，**不进 wasm**；客户端走浏览器/WebView 的光栅化）
- 不引：任何 SVG 解析/重排库（素材是我们自己产的，按字符串模板拼接即可，避免 `usvg` 的解析成本与不确定性）

---

## 11. 目录

```
crates/deckart/
├── Cargo.toml        并入根 workspace；feature `png` 隔离 resvg
├── README.md         本文
├── src/
│   ├── lib.rs        对外导出与约束说明
│   ├── card.rs       牌面语义（Suit / Rank / Face / all_faces）
│   ├── theme.rs      theme.json 解析、素材加载与校验
│   ├── render.rs     四层合成、级牌/逢人配叠加
│   ├── pack.rs       .gfdeck 打包/解包/加密/签名
│   ├── png.rs        SVG → PNG（feature = png）
│   ├── error.rs      统一错误码 DK-*
│   └── bin/gfdeck.rs CLI
└── tests/
    ├── render.rs     54 面 + 徽章 + 自包含性
    └── pack.rs       往返 / 错钥 / 篡改 / 签名 / 坏包
```

主题源目录在仓库根：`themes/defaults/`（theme.json + 23 个素材）。

---

## 12. 已确认决策（2026-11-26，逐条确认）

| # | 项 | 结论 |
| --- | --- | --- |
| 1 | 加密防护 | **A 内置密钥 + Ed25519 签名**：AES-256-GCM 逐条加密，密钥拆两段常量运行时拼接；包头 `keyId` 为方案 B（服务端下发 `themeKey`）预留 |
| 2 | defaults 素材 | **自绘纯矢量**：23 个文件全为手写 SVG，零外部字体、零位图依赖 |
| 3 | workspace | **并入根 members**，同时 `scripts/coverage.ps1` 排除本 crate，不拉低引擎覆盖率分母 |
| 4 | 布局 | **compact**：左上角标（点数 + 花色竖排）+ 中央大花色 + 右下镜像角标 |
| 5 | 文本层 | **可选层，`theme.json` 开关**：`enabled` 总开关 + `badge` 三态（`both` 素材与文字叠加 / `badge-only` 只画素材 / `text-only` 只画文字），关掉即回到零 `<text>` 的纯图形；素材本身始终不依赖字体 |
| 6 | `classic` 规范 | **现在定死**：画布 500×700、8 类子素材固有尺寸表、slot 左上角锚点语义、花阵归一化点位表（数量必须等于点数）；字段全进 `theme.json`，代码不硬编码 |
| 7 | 牌背 | **保持现状**：整幅 `back.svg` 直接铺满，后续主题给复杂前后背景时只换素材 |

方案 A 的诚实边界：客户端必然持有解密能力，A 只能挡住「改后缀直接看素材」，挡不住逆向提取；Ed25519 签名解决的是「包被篡改」，与保密正交。接方案 B 时只换 `PackOptions.key_id` 与会话密钥，包格式不变。

## 13. 实现状态与用法

### 已实现

- `card`：牌面语义类型，编码 `H8` / `S10` / `JOKER-B` / `JOKER-S`
- `theme`：目录加载 + schema 校验（id 规则、13 字形、路径穿越、外链/字体/位图禁用）
- `render`：四层合成、级牌与逢人配判定、固定图层 id 与 `data-*` 属性；`compact` 与 `classic` 两种布局；可选文本层（`L4t-level-label`）
- `pack`：`.gfdeck` 打包 / 解密 / 签名 / 摘要校验 / 边界校验，`open()` 可直接在内存里用主题
- `png`：SVG → PNG（`--features png`，不进 wasm）
- `gfdeck` CLI：`render` / `render-all` / `pack` / `unpack` / `info` / `verify` / `png`

### 测试（27 个单测 + 1 个 doctest，全绿）

```powershell
cd crates\deckart
cargo test                 # 5 lib + 11 render + 11 pack + 1 doctest
cargo test --features png  # 额外可用 PNG 子命令
```

覆盖：54 面编码唯一且闭合、级牌只在匹配点数命中、红桃级牌必须同时有 level + wild 两层、王永不级牌、无级牌态不出徽章、输出不得含外链/字体/位图、渲染确定性；文本层三态矩阵（`both` 两层都在、`badge-only` 无 text、`text-only` 无素材层、非级牌三种模式都不出 L4、关闭后退化为素材层、`text-only` 缺 label 报错而 `badge-only` 缺 label 合法）、classic 花阵数量（7 → 7 花阵 + 4 角标、A → 1 + 4）、classic 缺花阵规格即报错；包往返逐字节一致、错钥拒绝、改一字节即失败、签名必须匹配公钥、无密钥可读 header、截断/非包拒绝、缺素材与路径穿越在加载期就报错。

### 实测（2026-11-26）

```powershell
gfdeck render-all --theme themes/defaults --level 8 -o target/deckout
  → 55 张（54 面 + 牌背）、manifest.json 6507 字节、单张 766~2789 字节（带文本层的级牌最大）
gfdeck pack themes/defaults -o defaults.gfdeck --sign-key <hex32>
  → 24 条目（theme.json 2492B + 23 素材）、17137 字节、加密 + 签名、keyId=builtin-v1
gfdeck info   → defaults@2，license=CC0-1.0，加密=true，签名=true
gfdeck verify --verify-key <pub>  → 校验通过：defaults@2、24 条目、54 面可渲染
gfdeck verify（不给公钥）        → [DK-PACK-SIGNATURE] 拒绝
gfdeck unpack                    → 还原 24 个文件，可直接 Theme::load_dir
gfdeck png H8.svg --scale 2      → 151767 字节（1000×1400），「级牌」二字正常光栅化
```

两套布局用同一份素材各出一版：`compact` 走左上角标 + 中央大花，`classic` 走四角标 + 花阵（改 `layout` 一个字段即可切换，素材与代码都不动）。

### 已知限制

1. `classic.courtStyle = "figure"`（J/Q/K 人物素材）只保留 schema 位，当前一律按 `monogram`（中央大符号）渲染。
2. `ranks.kind = atlas` 未实现，会报 `DK-THEME-INVALID`。
3. 素材仍是第一版自绘矢量（A 用两斜 + 横杠、J 带左下勾、王牌是王冠 + 星形纯图形）；要换整套风格只需替换 23 个文件，代码与 schema 不动。
4. 文本层开启时转 PNG 依赖系统字体：Windows/macOS 自带中文字体即可，最小化 Linux 镜像需补装 Noto Sans SC，否则「级牌」二字不显示（牌面其余层正常）。
5. 未做单张渲染性能基准（第 9 节目标值待补测）。
6. 牌背保持现状，无叠加层（按需求暂不扩展）。
