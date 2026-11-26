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
| SVG 输出与 PNG 转换 | 牌桌布局、动画、点击判定（属 `apps/web`） |
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

两个布局的**角标位置都可改**：`theme.json` 写 `"corners": ["topLeft", "bottomRight"]` 就只画这两个角（详见 14.8）；不写时 `compact` 默认左上+右下、`classic` 默认四角。

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
`classic` 的四角用同一语义，额外自动旋转：右上 `translate(W-x, y+h·s)`、左下 `translate(x+w·s, H-y)`、右下 `translate(W-x, H-y)`，三者统一 `rotate(180)` 让字头朝牌心。要少画几个角就用 `corners` 过滤（14.8），位置公式不变。

角标花色摆在哪由 `cornerSuitPlacement` 决定（`below` 缺省 = 点数下方，`right` = 点数右侧，见 14.10）：配成 `right` 时 `slots.cornerSuit` / `classic.cornerSuit` 的 `x/y` 会被忽略，渲染层按点数放大后的实际右缘重新算。

> 改素材尺寸必须同步本表与 `theme.json`；只改配色/形状则不用动配置。

### 花阵规范（`layout = classic`）

`theme.json` 的 `classic` 块就是完整规范，代码里没有硬编码点位：

```json
"classic": {
  "cornerRank": { "x": 34, "y": 30, "scale": 0.36 },
  "cornerSuit": { "x": 38, "y": 98, "scale": 0.22 },
  "pipScale": 0.88,
  "pipInset": 0.10,
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
- 点位表是「花阵长什么样」，**不是**「能画到哪」：渲染时会把每个点位按 `pipInset` 压进内边距，再按角标放大后的实际宽度继续往里让（左右对称），所以点位表写了 0.28 也未必落在 0.28——角标越大，花阵越往中间收（见 14.9）；
- `pipScale` 是单个花符号的缩放（素材 100×100 → 画布单位），0.88 在 500 宽的牌面上约 88px；角标特别大时渲染层会自动等比缩小一点，避免两列点位粘在一起；
- `pipInset` 是花阵安全区的归一化内边距（缺省 0.10，合法 0..=0.35），保证花阵四周留白、不贴牌边；
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

主题源、打包产物、脚本全部收在本 crate 下，仓库根不再有 `themes/`：

```
crates/deckart/
├── themes/                    # 主题源：一个主题一个目录（入 git）
│   └── defaults/
│       ├── theme.json
│       ├── base.svg
│       ├── suits/{spade,heart,diamond,club}.svg
│       ├── ranks/{1..13}.svg  # 或 ranks.png + ranks.json
│       ├── jokers/{small,big}.svg
│       ├── back.svg
│       └── badges/{level,wild}.svg
├── pkgs/                      # 打包产物：themes/<id>/ → <id>.gfdeck
│   └── defaults.gfdeck
└── scripts/
    └── pack-all.ps1           # 一键打包 themes/ 下所有主题
```

新主题在 `themes/` 下建目录（目录名即包名），`theme.json` 的 `id` 保持 `[a-z0-9-]`；打完的 `<id>.gfdeck` 落在 `pkgs/`。

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
  "cornerScale": 2.2,
  "corners": ["topLeft", "bottomRight"],
  "cornerSuitPlacement": "below",
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
  "preview": { "faces": ["C2", "S2", "H2", "D2"], "levelRank": 2 },
  "classic": { "cornerRank": {…}, "cornerSuit": {…}, "pipScale": 0.88, "pipInset": 0.10, "aceScale": 1.70, "courtStyle": "monogram", "pipPositions": {…} }
}
```
`textLayer`、`preview`、`cornerScale`、`corners`、`cornerSuitPlacement` 与 `classic` 都是可选块（缺省用默认值/关闭），完整字段含义见第 3 节的尺寸规范、花阵规范与文本层说明，`preview` 见第 14 节，`cornerScale` 见 14.6/14.7，`corners` 见 14.8，`cornerSuitPlacement` 见 14.10。
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

## 8. CLI：从素材目录到客户端主题包（可直接复制）

命令都在**仓库根目录**执行，`gfdeck` 是 `crates/deckart` 的二进制，统一用 `cargo run -p gf-deckart --bin gfdeck --` 调起。

路径约定已固定下来：主题源在 `crates/deckart/themes/<id>/`，打包产物落 `crates/deckart/pkgs/<id>.gfdeck`，构建脚本在 `crates/deckart/scripts/`。仓库根不再有 `themes/`。

两个 feature 先说清楚（见 `crates/deckart/Cargo.toml`）：

| feature | 作用 | 什么时候要开 |
| --- | --- | --- |
| `pack-write` | 打包 / 签名 / 解包，`gfdeck` 这个 bin 本身也靠它才编得出来 | **默认开**，不用显式给 |
| `png` | SVG → PNG（resvg + tiny-skia + 系统字体） | 只有要转 PNG 时才加 `--features png` |

> 只在 `crates/deckart` 里跑不需要别的环境变量。如果命令是从整个 workspace 编过去的、报找不到 `protoc`，那是别的 crate（protocol）在编，补一个 `$env:PROTOC` 指到本机 protoc 即可。

### 8.1 出 54 张牌面（+ 牌背）

```powershell
# 54 面 + 牌背，一次全出；--level 决定哪一档打级牌高亮（2..A，或 none 表示不出级牌层）
cargo run -p gf-deckart --bin gfdeck -- render-all --theme crates/deckart/themes/defaults --level 2 -o target/faces
```

角标倍率不写 `--corner` 时**用 `theme.json` 里声明的 `cornerScale`**（defaults 声明了 2.2，也就是 220%），想按素材原样出图就显式 `--corner 100`。

产物：`target/faces/` 下 **55 个文件**（54 面 `S2…CA` + `JOKER-S`/`JOKER-B`，加 1 张 `BACK.svg`），外加 `manifest.json`：

```json
{
  "themeId": "defaults", "themeVersion": 2, "level": "2", "cornerScale": 2.20,
  "layout": "Compact",
  "corners": ["topLeft", "bottomRight"],
  "faces": [{ "code": "S2", "file": "S2.svg", "sha256": "…" }, …],
  "back": { "file": "BACK.svg", "sha256": "…" }
}
```

`manifest.json` 里的 sha256 是给「素材换版/灰产换皮」做逐张比对的，客户端不用它；`cornerScale` 记的是这一批出图实际用的倍率（`2.20` = 220%），`corners` 记的是实际画了哪几个角标（`theme.json` 的 `corners` 配过就按配的，见 14.8）。

要换字号就加 `--corner`，整包 54 张一起改（压过主题默认）：

```powershell
cargo run -p gf-deckart --bin gfdeck -- render-all --theme crates/deckart/themes/defaults --corner 240 -o target/faces-big
cargo run -p gf-deckart --bin gfdeck -- render-all --theme crates/deckart/themes/defaults --corner 100 -o target/faces-plain
```

### 8.2 单张调试 / 转 PNG

```powershell
# 单张：--face 用牌面编码（S2 / H10 / CA / JOKER-B），--level 可省，--corner 用百分比
cargo run -p gf-deckart --bin gfdeck -- render --theme crates/deckart/themes/defaults --face H8 --level 8 -o target/H8.svg
cargo run -p gf-deckart --bin gfdeck -- render --theme crates/deckart/themes/defaults --face H8 --level 8 --corner 240 -o target/H8-big.svg

# SVG → PNG（要 --features png；--scale 2 = 1000×1400，文本层「级牌」靠系统字体渲染）
cargo run -p gf-deckart --bin gfdeck --features png -- png target/H8.svg --scale 2 -o target/H8@2x.png
```

### 8.3 打包成 `.gfdeck`（客户端只认这个）

```powershell
cargo run -p gf-deckart --bin gfdeck -- pack crates/deckart/themes/defaults `
  -o crates/deckart/pkgs/defaults.gfdeck `
  --key-id builtin-v1 `
  --sign-key 1111111111111111111111111111111111111111111111111111111111111111 `
  --pub-key-out target/pub.hex
```

参数含义：

| 参数 | 说明 |
| --- | --- |
| `<theme-dir>` | 主题目录（`theme.json` + `assets/` 里那 23 个素材），位置参数放第一个 |
| `-o` | 输出包路径；父目录不存在会自动建 |
| `--key-file` | 对称加密密钥：32 字节二进制，或 64 位 hex 文本。**不给就用内置密钥**（keyId 自动为 `builtin-v1`） |
| `--key-id` | 密钥标识，写进包头，客户端按它选密钥；换密钥必须换 id |
| `--sign-key` | Ed25519 私钥种子（64 位 hex），给了就签名；不给则包可读但不防篡改 |
| `--pub-key-out` | 把对应公钥（hex）落盘，`verify` / 客户端别处要用；不给就打印到 stdout |

**注意**：`pack` 总是加密（没有 `--encrypt` 开关，加密是这个格式本身的一部分）；包里存的是 `theme.json` 的副本 + 23 个素材，**不存预览图**（预览由客户端现场合成，见 14.1）。

发布到客户端**不用重编 wasm**，也不用再往 `apps/web/` 拷一份：

1. 包直接打在 `crates/deckart/pkgs/<id>.gfdeck`；
2. `apps/web/serve.ps1` 把 `/pkgs/` 映射到这个目录，`app.js` 的 `THEME_PACK_URL` 是 `pkgs/defaults.gfdeck`；
3. 换签钥时才要改 `apps/web/app.js` 里的 `THEME_PUBKEY`（沿用内置密钥不用动）。

独立部署（不用 `serve.ps1`）时把 `pkgs/` 一起带上，放到静态根的同级 `/pkgs/` 即可。

刷新页面即生效（浏览器缓存里旧的包要先强刷一次），牌桌读的是包里那份 `theme.json`，**改了源目录的 `theme.json` 必须重新 `pack`**，否则客户端看到的还是旧副本。

### 8.4 校验 / 查看 / 解包

```powershell
# 只看包头：themeId@version、license、条目数、字节数、是否加密/签名、keyId
cargo run -p gf-deckart --bin gfdeck -- info crates/deckart/pkgs/defaults.gfdeck

# 完整校验：逐条解密 + 比对 sha256 + 验签 + 54 面能渲染
$pub = (Get-Content target/pub.hex).Trim()
cargo run -p gf-deckart --bin gfdeck -- verify crates/deckart/pkgs/defaults.gfdeck --verify-key $pub

# 解包回目录（签名包必须给 --verify-key，否则直接 DK-PACK-SIGNATURE；输出目录不能已存在）
cargo run -p gf-deckart --bin gfdeck -- unpack crates/deckart/pkgs/defaults.gfdeck -o target/unpacked --verify-key $pub
```

错误码都印在 stderr，脚本里可直接判：`DK-PACK-SIGNATURE`（签名不符 / 换钥 / 被改一字节）、`DK-PACK-DECRYPT`（密钥不对）、`DK-PACK-EXISTS`（`unpack` 的输出目录已存在，**不覆盖**，先删再解）、`DK-ASSET-MISSING`（少素材）、`DK-THEME-INVALID`（`theme.json` 不合 schema）、`DK-FACE-INVALID`（牌面编码或 `--corner` 越界）。

`pack` 的输出文件可以直接覆盖（每次加密用的 nonce 随机，同样输入打出来的字节不完全一致，但解密后的内容一致）；`unpack` 反过来，输出目录存在就拒绝。

### 8.5 一键打包所有主题（`scripts/pack-all.ps1`）

```powershell
# 遍历 crates/deckart/themes/*/theme.json，逐个打到 crates/deckart/pkgs/<id>.gfdeck
pwsh -File crates/deckart/scripts/pack-all.ps1

# 只打一个 / 换输出目录 / 复用已编好的二进制 / 换签钥
pwsh -File crates/deckart/scripts/pack-all.ps1 -Theme defaults
pwsh -File crates/deckart/scripts/pack-all.ps1 -OutDir D:\tmp\pkgs -SkipBuild
pwsh -File crates/deckart/scripts/pack-all.ps1 -KeyId studio-v1 -SignKey <64位hex>
```

脚本内部顺序：`cargo build -q -p gf-deckart --bin gfdeck --release`（`-SkipBuild` 可跳过）→ 收集主题目录 → 每个主题 `pack` 一次（公钥落 `target/theme-build/<id>.pub.hex`）→ 立刻 `info` + `verify` → 打印一张表。任一步非 0 退出即中断，不会再打后面的包。

| 参数 | 缺省 | 说明 |
| --- | --- | --- |
| `-Theme` | 全部 | 只打指定目录名，可传多个；名字不存在直接报错 |
| `-OutDir` | `crates/deckart/pkgs` | 产物目录，不存在自动建 |
| `-KeyId` | `builtin-v1` | 与客户端 `themeKey` 表对应，换密钥必须换 id |
| `-SignKey` | `0x11 × 32` | Ed25519 私钥种子（64 位 hex），与 `apps/web/app.js` 的 `THEME_PUBKEY` 成对 |
| `-SkipBuild` | 关 | 直接用手上已有的 `target/release/gfdeck` |

手动等价（只打一份、顺带出图）：

```powershell
# 产物统一落在 target/theme-build/：faces（SVG）、faces-big（大字角标）、defaults.gfdeck、pub.hex
cargo run -p gf-deckart --bin gfdeck --features png -- render-all --theme crates/deckart/themes/defaults --level 2 -o target/theme-build/faces
cargo run -p gf-deckart --bin gfdeck --features png -- render-all --theme crates/deckart/themes/defaults --level 2 --corner 240 -o target/theme-build/faces-big
cargo run -p gf-deckart --bin gfdeck --features png -- pack crates/deckart/themes/defaults -o target/theme-build/defaults.gfdeck --key-id builtin-v1 --sign-key 1111111111111111111111111111111111111111111111111111111111111111 --pub-key-out target/theme-build/pub.hex
cargo run -p gf-deckart --bin gfdeck --features png -- verify target/theme-build/defaults.gfdeck --verify-key (Get-Content target/theme-build/pub.hex).Trim()
```

### 8.6 实测（2026-11-26，Windows + PowerShell 7）

```text
render-all --level 2             → 55 张（54 面 + 牌背）+ manifest.json，角标 220%（主题默认）
render-all --corner 100          → 同样 55 张，角标 100%（素材原样）
render H8 --level 8              → 2799 字节（主题默认 220%）
render H8 --level 8 --corner 100 → 2789 字节（含文本层「级牌」）
render H8 --level 8 --corner 240 → 2799 字节
png H8.svg --scale 2             → 151,767 字节（1000×1400）
png H8@240.svg --scale 2         → 164,395 字节
pack --key-id builtin-v1 --sign-key <0x11×32> → 24 条目 / 17,312 字节，加密=true、签名=true
公钥                               d04ab232…c9778737
info                             → defaults@2，license=CC0-1.0，24 条目，加密=true，签名=true，keyId=builtin-v1
verify --verify-key <pub>        → 校验通过：defaults@2，24 条目，54 面可渲染，角标 220%（主题默认）
unpack --verify-key <pub>        → 还原 24 个文件（theme.json + 23 素材），可直接 Theme::load_dir
```

（`crates/deckart/pkgs/defaults.gfdeck` 已按上面命令重打过：17,312 字节，包里有 `preview` 块、`cornerScale: 2.2` 与 `classic.pipInset`。）

---

## 9. 测试与验收口径

| 项 | 判据 |
| --- | --- |
| 面覆盖 | 54 面 + 牌背全部可渲染，无缺素材、无未闭合标签 |
| 分层断言 | `data-level` / `data-wild` 与输入一致；王永远不是级牌；红桃级牌同时有 level + wild 两层 |
| 快照 | `defaults` 主题 55 张 SVG 文本哈希固定（改素材必须显式更新快照） |
| 包往返 | `pack → unpack → render` 产出与直接读目录逐字节一致 |
| 预览一致 | `preview` 配置生效（默认四张 2 全带级牌、`levelRank=null` 不出 `L4`、非法 faces/越界点数加载期报错）；wasm 端输出与 `gfdeck render` 逐字节一致 |
| 角标倍率 | 1.0 与旧入口逐字节一致；0.6~3.0 内点数与角标花色同步放大、花色重排到点数下方、底面层不动；越界报 `DK-FACE-INVALID` |
| 默认角标 | `theme.json` 的 `cornerScale` 可选（缺省 1.0）、1.0~3.0 外报 `DK-THEME-INVALID`；打包往返不丢；CLI 不给 `--corner` 时用它，`--corner` 压过它 |
| 角标位置 | `corners` 决定画哪几个角（缺省 compact 左上+右下 / classic 四角）；重复、超 4 项、未知名字加载期拒绝；`manifest.json` 同步记录 |
| 花阵安全区 | classic 的点位一律落在 `pipInset` 内边距之内；角标 100%/240%/300% 下每个点位都不与角标点数（放大后的实际宽度）相交；两列间距 ≥ 26 单位（角标过大时花阵自动等比缩小）；`pipInset` 越界加载期拒绝 |
| 角标花色位置 | `cornerSuitPlacement` 缺省 `below`（compact 1.0 时逐字节同素材原样）；配 `right` 后花色落在点数右缘之外并纵向居中；classic 下花阵左缘始终不压到右侧花色；`left` / `above` 等未知名加载期拒绝 |
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
├── themes/            主题源（每个主题一个目录）
├── pkgs/              打包产物 *.gfdeck
├── scripts/           pack-all.ps1 等构建脚本
└── tests/
    ├── render.rs     54 面 + 徽章 + 自包含性 + 预览配置
    └── pack.rs       往返 / 错钥 / 篡改 / 签名 / 坏包 / 字节入口
```

主题源目录在本 crate 内：`crates/deckart/themes/defaults/`（theme.json + 23 个素材）；打包产物在 `pkgs/`。

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
| 8 | 主题预览 | **客户端运行时生成，包里不存预览图**：`theme.json` 只声明渲染哪几张面（`preview`），SVG 由 `render_face` 现场产出，与牌桌同源；扇形叠放属 UI 层 |

方案 A 的诚实边界：客户端必然持有解密能力，A 只能挡住「改后缀直接看素材」，挡不住逆向提取；Ed25519 签名解决的是「包被篡改」，与保密正交。接方案 B 时只换 `PackOptions.key_id` 与会话密钥，包格式不变。

## 13. 实现状态与用法

### 已实现

- `card`：牌面语义类型，编码 `H8` / `S10` / `JOKER-B` / `JOKER-S`
- `theme`：目录加载 + schema 校验（id 规则、13 字形、路径穿越、外链/字体/位图禁用）
- `render`：四层合成、级牌与逢人配判定、固定图层 id 与 `data-*` 属性；`compact` 与 `classic` 两种布局；可选文本层（`L4t-level-label`）；`FaceOptions.corner_scale` 角标字号倍率（0.6..=3.0，见 14.6）；`theme.json` 的 `cornerScale` 给主题默认（1.0..=3.0，缺省 1.0，见 14.7）；`corners` 决定画哪几个角标（缺省按布局，见 14.8）；classic 花阵的安全区与自适应缩放（`pipInset` / `pipScale`，见 14.9）；角标花色摆点数下方还是右侧（`cornerSuitPlacement`，见 14.10）
- `pack`：`.gfdeck` 打包 / 解密 / 签名 / 摘要校验 / 边界校验，`open()` 可直接在内存里用主题；`open_bytes()` / `info_from_bytes()` 走纯字节入口（wasm 没有文件系统）
- `preview`：`theme.json` 的 `preview` 配置 + `render_preview()`，主题选择界面与牌桌共用同一条渲染路径
- `png`：SVG → PNG（`--features png`，不进 wasm）
- `gfdeck` CLI：`render` / `render-all` / `pack` / `unpack` / `info` / `verify` / `png`（`render`、`render-all` 支持 `--corner <pct>`）

### 测试（44 个单测 + 1 个 doctest，全绿）

```powershell
cd crates\deckart
cargo test                 # 5 lib + 13 pack + 26 render + 1 doctest
cargo test --features png  # 额外可用 PNG 子命令
```

覆盖：54 面编码唯一且闭合、级牌只在匹配点数命中、红桃级牌必须同时有 level + wild 两层、王永不级牌、无级牌态不出徽章、输出不得含外链/字体/位图、渲染确定性；文本层三态矩阵（`both` 两层都在、`badge-only` 无 text、`text-only` 无素材层、非级牌三种模式都不出 L4、关闭后退化为素材层、`text-only` 缺 label 报错而 `badge-only` 缺 label 合法）、classic 花阵数量（7 → 7 花阵 + 4 角标、A → 1 + 4）、classic 缺花阵规格即报错；角标倍率（1.0 与旧入口逐字节一致、2.4 倍时点数与花色同步放大且花色重排、classic 四角同样生效、越界拒绝）；主题默认角标（声明 2.2 读得出、字段可省、1.0~3.0 外加载期拒绝、打包往返不丢）；角标位置（compact 默认两角 / classic 默认四角 / 收成两角后的旋转计数、空数组合法、重复与未知名字拒绝）；classic 花阵安全区（100%/240%/300% 三档点位都不与角标点数相交、四缘在 `pipInset` 内、两列间距下限、`pipInset` 越界拒绝）；角标花色位置（缺省不重排、`right` 落到点数右缘外并纵向居中、classic 花阵让位、未知名拒绝）；包往返逐字节一致、错钥拒绝、改一字节即失败、签名必须匹配公钥、无密钥可读 header、截断/非包拒绝、缺素材与路径穿越在加载期就报错。

### 实测（2026-11-26）

下面按命令简写记录结果（实际调用形如 `cargo run -p gf-deckart --bin gfdeck -- render-all …`，完整命令见第 8 节）：

```powershell
gfdeck render-all --theme crates/deckart/themes/defaults --level 8 -o target/deckout
  → 55 张（54 面 + 牌背）、manifest.json 6507 字节、单张 766~2789 字节（带文本层的级牌最大）
     （角标走主题默认 220%；要看素材原样加 --corner 100）
gfdeck pack crates/deckart/themes/defaults -o defaults.gfdeck --sign-key <hex32>
  → 24 条目（theme.json 5160B 的 pretty 序列化 + 23 素材）、17312 字节、加密 + 签名、keyId=builtin-v1
     （上一版是 17,137 字节、包里没有 `preview` 块与 `cornerScale`；加 `preview` 后 17,266，加 `cornerScale: 2.2` 是 17,288，再加 `pipInset` 与 classic 新参数是 17,312）
gfdeck info   → defaults@2，license=CC0-1.0，加密=true，签名=true
gfdeck verify --verify-key <pub>  → 校验通过：defaults@2、24 条目、54 面可渲染、角标 220%（主题默认）
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

---

## 14. 主题预览与客户端 wasm 绑定（2026-11-26）

### 14.1 结论：预览不预存，客户端现场生成

`.gfdeck` 里**只装 24 个条目**（`theme.json` + 23 个分层素材，实测量 17137 字节）。主题选择界面的「四张 2」由客户端在 wasm 里调 `render_face` 当场算出来，包里既不放整图也不放缩略图。理由：

- 与牌桌**同一条渲染路径**，不会出现「预览里好看、上桌变形」这种两套实现漂移；
- 主题作者只维护 23 个矢量素材，不用再交预览图，也不用为不同 DPI 出多套；
- 包体不变大：四张预览本身就是 4 段 SVG 字符串，实测 2153~2721 字节/张。

商店/列表页那种**封面图**（PNG、进后台接口）是另一条链路，用 CLI 出图即可，不进客户端包：

```powershell
gfdeck render-all --theme crates/deckart/themes/defaults --level 2 -o target/preview
gfdeck png target/preview/C2.svg -o cover-c2.png --scale 2
```

### 14.2 `theme.json` 的 `preview` 块

```json
"preview": { "faces": ["C2", "S2", "H2", "D2"], "levelRank": 2 }
```

| 字段 | 类型 | 缺省 | 说明 |
| --- | --- | --- | --- |
| `faces` | `string[1..=8]` | `["C2","S2","H2","D2"]` | 用牌面编码（`C2`/`S10`/`JOKER-B`）指定渲染哪几张，顺序即 UI 顺序 |
| `levelRank` | `u8 \| null` | `2` | 预览时按这个点数当级牌；**显式写 `null`** 表示纯牌面 |

约束与取向：

- 缺省 `levelRank = 2`，四张 2 全是级牌，红桃 2 顺带是逢人配——一屏能同时看到「级牌胶囊 + 描边 + 逢人配徽章」三种标记，主题差异最明显；
- 想展示「不带级牌态」的干净牌面就写 `"levelRank": null`，客户端会自动不带 `L4` 层；
- 校验（加载期就报 `DK-THEME-INVALID`，不是等客户端渲染才炸）：`faces` 数量 1..=8、每项能 `Face::parse`、`levelRank` 落在 1..=13。

### 14.3 扇形排布（UI 侧，不进主题包）

主题卡里的那圈扇形是纯 CSS，和主题无关；四张牌各自独立、正立、按自己的坐标旋转，**不做合成大图**（合成会带来 DPR 与文字模糊，且缓存粒度变粗）。

| 参数 | 值 |
| --- | --- |
| 单张尺寸 | 100 × 140 CSS px（5:7，与 canvas 同比例，直接 `<svg style="width:100px;height:140px">`） |
| 水平步进 | `translateX` 0 / 58 / 116 / 174 px（露 58% 牌宽） |
| 旋转 | `-14° / -5° / +5° / +14°` |
| 旋转原点 | `transform-origin: 50% 120%`（绕牌底下方转，看起来像握在手里） |
| 层级 | `z-index` 递增，后一张压前一张 |
| 容器 | 约 430 × 150 px（含投影余量），`filter: drop-shadow(0 3px 6px …)` 代替合成阴影 |

### 14.4 客户端可调的 wasm 方法（`crates/client-wasm`）

`gf-client-wasm` 以 `default-features = false` 依赖本 crate：**`pack-write` 与 `rand` 不进 wasm**，只带解密 + 验签 + 渲染。新增五个导出，全部沿用既有约定（裸 `WebAssembly.instantiate`、零 import、出参 NUL 结尾 JSON + `gf_str_free`）。

| 方法 | 入参 | 出参 |
| --- | --- | --- |
| `gf_theme_load(pack_ptr, pack_len, pub_ptr, pub_len)` | 包字节 + 公钥 32B（`pub_len = 0` 跳过验签） | `{ok, themeId, version, name, license, layout, cornerScale, bytes, levelRank, faces, preview:[{code,svg}]}` |
| `gf_theme_preview()` | — | `{ok, cards:[{code,svg}]}` |
| `gf_theme_card(code_ptr, code_len, level_rank, corner_pct)` | 牌面编码 + 级牌点（`0` = 无级牌态）+ 角标字号百分比（`0` = 用主题 `cornerScale`，`100` = 素材原样） | `{ok, code, svg}` |
| `gf_theme_back()` | — | `{ok, svg}` |
| `gf_theme_unload()` | — | `1` |

`gf_theme_load` 一次就把预览四张一起返回，客户端不必再调 `preview`（省一次跨边界）；`gf_theme_card` 留给牌桌逐张取，级牌点由服务端下发，客户端不推断。失败统一 `{ok:false, error:"…"}`。

```js
// 1) 载入主题包（字节由 fetch/IndexedDB 得来）
const ptr = ex.gf_alloc(packBytes.length);
new Uint8Array(ex.memory.buffer, ptr, packBytes.length).set(packBytes);
const meta = JSON.parse(readStr(ex.gf_theme_load(ptr, packBytes.length, pubPtr, 32)));
// meta.preview → [{code:'C2', svg:'<svg …>'}, …]，直接插进主题卡的四个 <div>

// 2) 上桌逐张取（缓存键 (themeId, themeVersion, code, levelRank, cornerPct)）
const card = JSON.parse(readStr(ex.gf_theme_card(codePtr, 2, levelRank, 240)));
// 主题说了算就传 0（用 theme.json 的 cornerScale）；传 100 才是素材原样
const themed = JSON.parse(readStr(ex.gf_theme_card(codePtr, 2, levelRank, 0)));
```

要点：

- 入参用 `gf_alloc` 写字节、`gf_mem_free` 归还；出参读 NUL 截止后 `gf_str_free`，与房间那套完全一致；
- 公钥是 32 字节原始公钥（不是 PEM/hex 串），`0x11×32` 私钥对应的公钥为 `d04ab232…c9778737`；
- **两套点数编号别混**：`gf_theme_card` 的 `level_rank` 是 `Rank` 编号（**1=A、2=2、…、13=K**），而引擎 `round.level` 是 `levelTable.ranks` 的**档位下标**（0=2、…、12=A）。推荐做法：客户端内部一律用**点数标签**（`'2'`…`'A'`，一份 `RANKS` 表），只在调 wasm 前用 `rankCode(label)` 转成 `Rank` 编号；直接拿下标加减（如 `下标 + 1`）会整体错一档：打 4 画到 3 上、打 2 画到 A 上、打 A 画到 K 上（2026-11-26 已实机修掉，并补了 13 档逐档断言）；
- **降级**：`gf_theme_load` 返回 `ok:false`（包损坏、签名不符、缺素材）时，客户端退回现有的 DOM/CSS + Unicode 花色卡片，牌桌不阻塞；
- 换主题先 `gf_theme_unload()` 再 load，避免同时挂两份素材。

### 14.5 实测（2026-11-26，Node 22 直跑 wasm，无 wasm-bindgen）

```text
wasm 体积   1,062,590 → 1,432,403 字节（+369,813 / +34.8%，含 aes-gcm + ed25519 + serde_json + deckart）
load        7 ms（解密 + 验签 + 装配 + 预览四张）
preview     0 ms（再算一次四张）
单张 SVG    C2 2609 / S2 2606 / H2 2721 / D2 2153 字节，牌背 766 字节
错误公钥      拒绝（PackSignature）
逐字节一致    wasm 的 C2 与 `gfdeck render --face C2 --level 2` 输出 2609 字节完全相同
级牌落点      13 档逐档断言（2→K→A）：`data-level` 与 `data-leveled` 只命中该档，不串档
角标倍率      浏览器 wasm 单张（106 张均值）：100% 0.018 ms / 240% 0.023 ms / 300% 0.015 ms，放大不吃性能
```

体积涨的主要是验签（`ed25519-dalek` + `curve25519`）与 `serde_json`；若要压回去，下一步接 `wasm-opt -Oz`（当前机器未装）。

### 14.6 角标字号可调（`FaceOptions.corner_scale`，2026-11-26）

**为什么不做「整张 SVG 放大再裁切」**：手机/PC 上横排紧叠时，早先的做法是把整张 100×140 的牌按 350% 放大、只露左上角。结果是主题的底板圆角、描边、中央花阵层面**全被裁掉**，屏幕上只剩白底 + 一个点数，看着跟主题没关系——用户判断「这也不是默认主题」。正确做法是**在牌面内部只放大角标那一区**，底板/描边/花阵原地不动。

API 形状：

```rust
pub struct FaceOptions {
    pub level: Option<LevelState>,   // 本局级牌态，None = 不出 L4
    pub corner_scale: f64,           // 角标倍率，1.0 = 主题原样；合法 0.6..=3.0
}
pub fn render_face_opts(theme, face, opts) -> Result<String, DeckError>;  // 新增
pub fn render_face(theme, face, level)     -> Result<String, DeckError>;  // 保留，等价 corner_scale = 1.0
```

`corner_scale != 1.0` 时做什么（`render_normal_compact` / `render_normal_classic` 同一套口径）：

1. `slots.corner_rank.scale` 与 `slots.corner_suit.scale` **同乘**倍率——点数与花色一起长大，只放大点数会显得头重脚轻；
2. 放大后角标花色**重新定位**：`suit.y = rank.y + rank 素材高 × 新 scale + 间隙 × 倍率`（间隙基准 10 画布单位）。主题默认那点间距是按小字号排的，字号一大花色就被点数压住了；
3. 中央花阵、底面、徽章、镜像角标的位置全部不动；右下镜像用同一个 scale，自动跟着放大。

尺寸换算（canvas 500×700，牌面 100×140 CSS px，1 单位 = 0.2 px）：

| 倍率 | 点数高（桌面 100×140） | 手机 70×96 |
| --- | --- | --- |
| 100%（主题原样） | ≈ 9 px | ≈ 6 px |
| **220%**（客户端默认） | ≈ 26 px | ≈ 18 px |
| 240% | ≈ 28 px | ≈ 20 px |
| 300%（上限） | ≈ 34 px，开始压到中央花阵 | ≈ 24 px |

**王没有角标层**：素材是整幅图案（冠 + 星），放大角标的机制对它无效；横排只露一条边时大小王分不出来。客户端在主题牌面上给王补一个文字角标（`小王` / `大王`，字号跟随同一个百分比），图案保留在底下，选中/出牌区看整张时仍然完整。

边界与报错：

- 倍率越界（`< 0.6`、`> 3.0`、`NaN`）→ `DeckError::FaceInvalid("corner_scale 越界（合法 0.6..=3.0）")`，不吃掉一张牌；
- `corner_scale = 1.0` 与旧入口 `render_face` **逐字节一致**（测试 `corner_scale_one_is_byte_identical_to_plain_render` 卡住）；
- 倍率本身不改主题素材：`slots.corner_*` 仍是主题作者定的基准值，倍数叠加在它之上。主题想给一个**默认值**，写在 `theme.json` 的 `cornerScale` 里（见 14.7）。

测试（`crates/deckart/tests/render.rs`，新增 4 条）：

| 用例 | 判据 |
| --- | --- |
| `corner_scale_one_is_byte_identical_to_plain_render` | 倍率 1.0 = 旧入口逐字节一致 |
| `corner_scale_enlarges_corner_and_reflows_suit` | L3/L2 首处 scale 都乘到 2.4；角标花色 y 下移 > 60 单位；底面层前 200 字节不动 |
| `corner_scale_survives_classic_layout` | `layout = classic` 四角点数同样乘倍率 |
| `corner_scale_rejects_out_of_range` | 0.1 / 0.0 / -1.0 / 4.0 / NaN 全部拒绝；0.6 与 3.0 边界可用 |

### 14.7 主题包里的默认角标（`theme.json` 的 `cornerScale`，2026-11-26）

字号可调之后，缺省用多少成了主题自己的事：横排叠放的主题需要 220%，按整张牌欣赏的主题希望 100%。所以这个默认值**放在主题包里**，而不是写死在客户端。

```json
{ "id": "defaults", "version": 2, "layout": "compact", "cornerScale": 2.2, … }
```

| 项 | 规则 |
| --- | --- |
| 类型 | 数值倍率（`2.2` = 220%），**不是** `"220"` 或 `"220%"` |
| 可选 | 缺省即 1.0（素材原样）；`pack` 重新序列化 `theme.json` 时会原样带走 |
| 合法区间 | `1.0..=3.0`（与客户端滑块 100%–300% 同一口径），越界在**加载期**报 `DK-THEME-INVALID`（`cornerScale 越界（合法 1..=3，即 100%–300%）`），不会拖到渲染时才炸 |
| 优先级 | 客户端 `?corner=` / localStorage（用户手动调过）> 主题 `cornerScale` > 客户端兜底 220 |
| CLI | `gfdeck render/render-all` 不给 `--corner` 时用它（`manifest.json` 的 `cornerScale` 记实际值，打印带「（主题默认）」后缀）；`--corner` 一律压过它 |
| wasm | `gf_theme_load` 的返回值多一个 `cornerScale`（`null` = 主题没声明）；`gf_theme_card` 的 `corner_pct` 传 `0` = 用主题默认 |
| 预览 | 主题墙预览（`render_preview`）固定按 1.0 出图，不跟 `cornerScale`——预览是「素材长什么样」，牌桌才是「怎么叠」 |

`defaults` 现在声明 `2.2`，所以客户端不传 `?corner` 也是 220%，与加这个字段之前完全一致；`render-all` 不给 `--corner` 也出 220% 的图，要看原样请显式 `--corner 100`。

相关测试（`tests/render.rs` 2 条 + `tests/pack.rs` 1 条）：

| 用例 | 判据 |
| --- | --- |
| `theme_declares_default_corner_scale` | defaults 声明了 2.2，`Theme::corner_scale()` 读得出来 |
| `theme_corner_scale_is_optional_and_bounded` | 删掉字段 → `None`；3.5 / 0.5 / -2.0 在加载期报 `DK-THEME-INVALID`；1.0 与 3.0 可用 |
| `pack_preserves_declared_corner_scale` | 打包会重新序列化 `theme.json`，`cornerScale` 不能在路上丢 |

### 14.8 角标位置可配（`theme.json` 的 `corners`，2026-11-26）

`layout = classic` 的默认就是**四角标 + 花阵**（真实扑克排布），所以用 classic 出图时四角都有点数与花色不是 bug。要让 classic 也只画左上 + 右下（牌桌横排只露一条边，四个角有俩永远看不见，还多花一次渲染），加一行配置：

```json
{
  "layout": "classic",
  "cornerScale": 2.4,
  "corners": ["topLeft", "bottomRight"],
  "classic": { … }
}
```

| 项 | 规则 |
| --- | --- |
| 取值 | `topLeft` / `topRight` / `bottomLeft` / `bottomRight`，数组，最多 4 项 |
| 缺省 | 不写 = 跟布局走：`compact` → 左上 + 右下；`classic` → 四角全画 |
| 朝向 | 左上正写；其余三个一律 `rotate(180)`（字头/花尖朝牌心），与 classic 原口径一致，位置公式不变 |
| 空数组 | 合法，等于「一个角标都不画」，只剩中央大花色 / 花阵 + 徽章 |
| 校验 | 未知名字（serde 层）、重复项、超过 4 项 → 加载期 `DK-THEME-INVALID` |
| 与 `cornerScale` | 正交：`corners` 决定**画哪几个角**，`cornerScale` 决定**画多大** |
| 花阵 | 不受影响：classic 的点位表照旧，去掉两个角标后原本被大字号角标压住的点位会露出来 |
| `manifest.json` | `render-all` 会写 `"corners": [...]`，记录这批图实际画了哪几个角 |
| 客户端 | 横排紧叠只露左上一条边，**主题若去掉 `topLeft`，横排就看不到点数**——要上牌桌的主题请留着左上 |

`defaults` 没写 `corners`（compact 本来就是两角）。想把四角收成两角时，`layout` 与 `corners` 一起改，然后按第 8 节重打一次包即可。

实测（`D10`、classic、`--corner 240`）：四角版 3414 字节，`corners: ["topLeft","bottomRight"]` 后 2558 字节（少 6 个 `rotate(180)` 角标组里的 4 个），花阵点位一个不少。

相关测试（`tests/render.rs` 2 条）：

| 用例 | 判据 |
| --- | --- |
| `corners_config_limits_marks_in_both_layouts` | compact 默认 2 处旋转、只留左上 0 处、classic 默认 6 处、classic 收两角 2 处；花阵组仍在 |
| `corners_config_is_validated` | 重复 / 5 项 / 未知名字在加载期拒绝；空数组合法且 `corner_positions()` 为空 |

### 14.9 classic 花阵：安全区 + 自适应缩放（`pipInset` / 自动收窄，2026-11-26）

角标字号一放大，两个老问题一起爆出来：点数把角标花色压住（14.6 已修，classic 这次同一套口径补齐），以及**放大的角标压到花阵点位**——`D10` 在 classic + 240% 下，10 个 ♦ 只能看见 6 个，靠近四角的 4 个被点数挡住。点位表本身没错，错在「点位表写的是全幅坐标，没给角标让出地方」。

现在渲染层按三步摆花阵：

1. **算角标实际占多宽**：`cornerRank.x + 素材宽 × 角标 scale`，与角标花色取大的那个（都按放大后的 scale 算）；
2. **定安全区**：`insetX = max(pipInset × 牌宽, 角标右缘 + CORNER_GAP × 倍率)`，上限 34% 牌宽；`insetY = pipInset × 牌高`；
3. **对称收拢 + 自适应缩放**：每个点位按 `min(p_x, 1-p_x)` 先算离牌边多远，压不进安全区就左右**对称**往里推（不会整片花阵歪到一边）；如果角标大到两列会被挤在一起（列间距 < `PIP_GAP` = 26），整个花阵等比缩小到刚好留出这道缝。

`pipInset` 缺省 0.10（合法 0..=0.35，越界 `DK-THEME-INVALID`）。想更宽松就 0.14，想更满就 0.06。

`defaults` 这次一起调了 classic 的观感（对齐 CC0 参考牌：大字点数 + 紧随其下的小花色 + 大花阵 + 充足留白）：

| 字段 | 改前 | 改后 | 为什么 |
| --- | --- | --- | --- |
| `classic.cornerRank.scale` | 0.40 | 0.36 | 数字窄一点，给花阵腾横向空间（放大后仍是大字） |
| `classic.cornerSuit.scale` | 0.30 | 0.22 | 参考牌的花色明显小于点数，且不再被点数压住 |
| `classic.pipScale` | 0.60 | 0.88 | 花阵单个 ♦ 从 60 涨到 88（牌宽 17.6%），整体放大 |
| `classic.pipInset` | 无 | 0.10 | 花阵四周留白，不与角标/牌边相压 |

实测（`D10`、classic + `corners:["topLeft","bottomRight"]`、×2 PNG）：100% → 花阵 88、列距 220；240% → 花阵 88、列距 110（角标右缘 130 之外的对称位）；300% → 角标宽 108，花阵自动缩到 67、列距 93，10 个点位全部露头、互不相压。compact 布局不受影响（`pipScale` / `pipInset` 只属于 classic 块，compact 的中央大花走 `slots.centerSuit`）。

相关测试（`tests/render.rs` 2 条）：

| 用例 | 判据 |
| --- | --- |
| `classic_pips_clear_corner_marks_when_corners_are_huge` | 100% / 240% / 300% 三档：每个点位左缘 ≥ 角标点数右缘（按放大后实际宽度算）、四缘都在 `pipInset` 之内、两列间距 ≥ 20、角标花色 y 排在点数下方 |
| `classic_pip_inset_is_validated` | `pipInset` 0.5 / -0.1 在加载期报 `DK-THEME-INVALID` |

### 14.10 角标花色摆哪：点数下方 or 点数右侧（`cornerSuitPlacement`，2026-11-26）

`cornerScale` 只管放大，放大之后花色摆哪儿原来是写死的「点数下方」。现在进 `theme.json`：

```json
{
  "cornerScale": 2.2,
  "cornerSuitPlacement": "right",
  "classic": { … }
}
```

| 取值 | 摆放 | 说明 |
| --- | --- | --- |
| `below`（缺省） | 花色在点数正下方 | 传统扑克口径；`cornerScale = 1.0` 时完全沿用 `slots.cornerSuit` 的坐标，输出逐字节不变 |
| `right` | 花色贴点数右缘（`x = 点数x + 素材宽 × scale + 10`），纵向与点数居中 | 角标读起来是「10♦」这种横排；`slots.cornerSuit` / `classic.cornerSuit` 的 `x/y` 会被忽略，只保留 `scale` |

两条布局都支持，但代价不一样：

- **compact** 中央只有一个大花色，右侧摆完全不吃位置，240% 下画面仍然干净（`10 ♦` 并排 + 中央大 ♦）。
- **classic** 有花阵，右侧摆会把角标区撑宽（点数本身已经占掉 1/4 牌宽），花阵只能整体往中间收、并且按 `PIP_GAP` 下限缩一点。实测 `D10`（classic + 两角）：`right` + 180% 花阵仍 63~79，观感与「下方」近似；`right` + 240% 花阵只剩 44、`right` + 300% 只剩 27，牌面会显得空。所以 classic 想用右侧摆放，建议角标 ≤ 200%，或者把 `classic.cornerRank.scale` 从 0.36 收到 0.30 左右；要大字 + 大花阵就还是 `below`。

`right` 时点数与花色之间的间隙固定为 `CORNER_GAP`（10），不跟着倍率涨——否则角标区太宽，花阵没地方站。

相关测试（`tests/render.rs` 2 条）：

| 用例 | 判据 |
| --- | --- |
| `corner_suit_can_be_placed_right_of_the_rank` | 缺省时花色仍用 slot 原坐标（x 36）；`right` + 220% 时花色 x ≥ 点数右缘 + 9、纵向中心与点数中心差 < 1；classic `right` 在 100%/180%/240% 下花阵左缘都 ≥ 花色右缘 |
| `corner_suit_placement_rejects_unknown_value` | `left` / `above` 在解析期就报 `DK-THEME-INVALID` |


