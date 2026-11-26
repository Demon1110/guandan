# 掼蛋 M1 · Web 端（本地房间）

浏览器里直接跑 `gf-table-runtime` 的房间主干：大厅开局 → 牌桌行动（含计时与托管）→
本局结算 → 手动「再来一局」。人类座位可在大厅选（默认座位 0），其余座位挂 bot，
档位可选 AI-L0 / L1 / L2（默认 L2，只读 `view(seat)` + 本座位合法集 + 搭档状态）。
bot 出牌按 `recommend` 的最优整手拆分走：领出先出牌力最小的那一组（散张参与比较），
跟牌避开「只取了某组一部分牌」的候选——不把对子拆成单张零着打。

## 为什么是 wasm 直跑

`crates/client-wasm` 把房间编成 `cdylib`，产物零 import，浏览器 `WebAssembly.instantiate`
裸加载即可，不需要 wasm-bindgen / npm / 构建工具链（和 `crates/engine/tests/wasm` 的 M0
验证同一套路）。规则集在编译期由 `include_str!` 内嵌，前端不需要额外加载夹具。

## 跑起来

```powershell
pwsh -File apps/web/build.ps1      # cargo build wasm + 复制到 apps/web
pwsh -File apps/web/serve.ps1      # 起静态服务 http://127.0.0.1:8123/
```

`file://` 打不开（wasm 必须走 fetch）。带查询参数会直接进牌桌（跳过大厅）：

```
http://127.0.0.1:8123/index.html?seed=0x51EDA101&seat=2&level=1
```

不带参数则停在**大厅**：填种子（1–16 位十六进制）、选座位与 bot 档位，点「开始比赛」。
该种子 + 座位已有存档时，大厅会提示条数并要求续局（也可点「继续上次对局」）。

## 表现层

纯 DOM + CSS，不引 PixiJS、不引外部素材。

- **大厅**：种子 / 座位 / 档位 / 开始（含存档检测与提示）。
- **牌桌**：四方位座位（自己永远在下方，其余按顺时针排）；左上角记分板显示双方本局级牌
  （「我方 / 对方」文字在上、级牌在下，打级方黄框高亮；不再用箭头标当前行动方）；座位卡给出我方/对方、手牌张数、
  最近一手，出完的座位打「头游 / 二游 / 三游 / 末游」标签。
  手牌**同点数一列纵向叠放**（只露左上角标），列序按牌力降序：大王、小王、级牌、A、K…2。
  点一张选一张，也可以**按住左键横向/纵向滑过**连续选中（起点是已选牌则整段改为取消）。
  出牌区按四个方位铺在各自座位前：自己在**手牌区上方**，右家在自己座位的**左侧**，对家在座位**下方**，
  左家在座位**右侧**；四个人本墩最近一手的牌会同时摆出来，谁过牌就在原位显示「过」，新墩开始时统一清空。
  最后一手把牌出完后，手牌同步为空、出牌区在结算时一起清空；牌已经打出去了，不会留在桌面继续显示。
  每块出牌区标注「你 / 右家 / 对家 / 左家 + 牌型中文名」。
  按钮：出牌 / 提示 / 过牌 / 取消选牌压在「选牌与牌型」行**上方**，只在轮到自己时出现，其余时间整行收起。
  出牌按 `gf_room_can_play` 判合法（选牌不合法时置灰）、**提示**在 `gf_room_legal` 里循环选一手
  （要不起时置灰）、领出时过牌置灰；非法出牌在中央区红字提示并保留选牌。
  手牌下方是**理牌条**，纯布局层：不动手牌、不动选中、不影响判定。
  「组牌建议」直接铺 `recommend` 的最优整手拆分：炸弹 / 同花顺进最左列、其余进最右列，
  没进任何组的散张留在中间普通列按牌力排；点一列就是一手整组出，不用自己拼牌。
  中间四个花色 ♠♥♦♣ 在能凑出该花色同花顺时高亮，反复点击在多个同花顺窗口间循环切换（中央提示「方案 x/y」）；
  「理成一列」把选中的牌整理掉——炸弹 / 同花顺去最左、其余去最右（只认牌型，不看压不压得住），
  重复理同一组会替换而不是叠加；「回复理牌」在选中牌正好等于某个理牌组时只恢复该组，否则恢复全部。
  理牌列可整组点选：点一列＝选中整组，再点＝取消。
- **结算**：名次表（**头游 / 二游 / 三游 / 末游** · 座位 · 我方-对方）、名次串、头游队；
  结算发生时桌面会摊开**所有座位剩余的牌**（缩成小牌贴在座位卡下方，已出完的座位标「已出完」），
  出牌区同时清空：最后一手已经打出去，不再留在桌面冒充「还没出完」。
  **不再自动续局**，点「再来一局」才发下一局；场次结束（过 A / 安全上限）时按钮置灰并提示回大厅。
- **设置**：出牌速度（120/240/600ms，影响 bot 连续行动的观感）、显示牌 id（调试）、
  **角标字号**（100–300%，只对主题牌面生效，见下节）、动作日志侧栏开关、清空当前存档、刷新页面。
  设置存 `localStorage` 的 `guandan-m1-settings`，角标字号单独存 `guandan-corner-pct`。

窄屏（<1200px）自动收起日志侧栏，日志改为右侧浮层。

## 主题牌面（`.gfdeck` 运行时合成）

主题包 `crates/deckart/pkgs/defaults.gfdeck`（签名版，静态服务把 `/pkgs/` 映射到该目录）由 wasm 现场解密 + 验签 + 逐张合成 SVG，**包里不存任何预览图**。
`defaults.gfdeck` 怎么生成、怎么重打、怎么校验，命令都在 `crates/deckart/README.md` 第 8 节（`render-all` 出 54 面 → `pack` 签名 → `verify`）。换包不用重编 wasm。

- URL 开关：`?theme=1` 用主题牌面（不填 `hand` 时默认走横排 `fan`）、`?pkg=pkgs/squabby.gfdeck` 指定主题包（默认 `pkgs/defaults.gfdeck`）、`?hand=fan|cols` 手动指定手牌排布、
  `?corner=240` 覆盖角标字号（截图/联调给死值，优先于 `localStorage`）、`?debug=1` 挂 `window.__gd`。
- 加载失败（包损坏 / 签名不符 / 缺素材）一律退回 DOM 字卡，牌桌不阻塞，日志会写原因。
- **角标字号**：`faceOptions.corner_scale` 是 deckart 的渲染参数（详见 `crates/deckart/README.md` 14.6）。
  拖设置里的滑块会 `theme.cache.clear()` 并重渲染手牌；wasm 单张 0.02ms，几十张也不掉帧。
- **默认值来自主题包**：`theme.json` 里可选 `cornerScale`（1.0–3.0，即 100%–300%），`defaults` 声明了 2.2。
  优先级是 `?corner=` / `localStorage`（手动调过）> 主题 `cornerScale` > 客户端兜底 220；
  采用了主题默认时日志会写一条「角标字号采用主题默认：220%（theme.json cornerScale=2.2）」（详见 deckart README 14.7）。
- **角标位置也由主题决定**：`theme.json` 的 `corners` 可选 `topLeft`/`topRight`/`bottomLeft`/`bottomRight`
  （详见 deckart README 14.8）。横排只露左上一条边，主题若去掉 `topLeft`，横排就读不出点数。
- **不做整图放大裁切**：早先版本把 100×140 的牌按 350% 放大、只露左上角，主题的底板圆角、描边、
  中央花阵全被裁掉，看着就是一张白底字卡。现在整张牌 **1:1 显示**，只把角标那一区在牌面内部放大。
- **横排紧叠**：桌面 100×140、`margin-left:-44px`（每张露 56px），每行最多 14 张、27 张折 2 行；
  手机 70×96、`-48px`、2 行。牌面层级按 index 递增，`z-index` 高的压上面。
- **王**：素材是整幅图案，小丑/国王本身就能区分大小王；牌面上不再额外叠“大王/小王”文字，避免遮住插画。
- 出牌区的迷你牌（34×48）和手牌共用同一份 SVG 缓存，键是 `牌面@级牌标签@角标字号`。

## 存档与续局

IndexedDB 库 `guandan-m1`：

- `matches`（主键 `matchId`）：`seedHi/seedLo/humanMask/humanSeat/level/lastHash/lastRoomSeq/lastRoundNo`
- `actions`（主键 `[matchId, seq]`）：`seq(roomSeq)/roundNo/seat/pass/cards`

每落一手动作就与「当前权威 `stateHash`」写进同一个事务，两者必然同步。所以「杀进程」
（关掉浏览器进程）之后只剩这份日志也能把局面复原：

1. `gf_room_new_level(seed, mask, level)` + `gf_room_start`；
2. 遇到 `roundNo` 递增先 `gf_room_next_round`，再逐条 `gf_room_replay_action`；
3. `gf_room_hash` 与存档 `lastHash` 比对，相等才继续 `gf_room_resume_window` + 推进。

页面上的「刷新页面（验证续局）」就是这条路径的等价演练：刷新会重建页面进程与 wasm 实例，
内存状态全部丢弃，只有 IndexedDB 留下。日志会打印恢复条数与「哈希一致 ✓/✗」。

## wasm 导出

| 导出 | 说明 |
|---|---|
| `gf_room_new(seed_hi, seed_lo, human_mask)` | 建桌，bot 固定 L2（兼容旧页面/脚本） |
| `gf_room_new_level(seed_hi, seed_lo, human_mask, bot_level)` | 建桌，`bot_level` 0/1/2 → L0/L1/L2 |
| `gf_room_start / step / submit / replay_action / next_round / resume_window` | 房间推进 |
| `gf_room_view / hash / meta / seats / last_record` | 视图与自证 |
| `gf_room_legal` | 当前行动座位的合法动作集（`[{"pass":false,"patternId":"bomb-4","cards":[…]}]`，Pass 项只含 `pass:true`），`patternId` 走 `preview_play` 得出；「提示」与「组牌建议」按它选牌 |
| `gf_room_can_play` | 预检一手牌能否出（与 `submit` 同口径）：结构不成立、不在手牌、压不过上家都返回 `ok:false`，同时回传识别出的 `patternId`；「出牌」按钮读它、理牌按 `patternId` 认牌型 |
| `gf_room_plan` | 当前行动座位的一手牌推荐拆分（`recommend`，纯只读、不进 `stateHash`）：`{"ok":true,"seat":…,"score":…,"singleCount":…,"groups":[{"patternId":…,"cards":[…]}]}`；bot 决策与「组牌建议」共用这一份，散张不在 `groups` 里、由房间按「手牌 − 各组」补齐 |
| `gf_room_info` | 桌面公开信息（不进座位视图、不影响 `stateHash`）：相位、行动座位、连续过牌数、双方级牌、打级队、各家手牌张数与已锁定名次 |
| `gf_theme_load(pack_ptr, pack_len, pub_ptr, pub_len)` | 载入并校验主题包，一次返回元信息 + 预览四张 |
| `gf_theme_card(code_ptr, code_len, level_rank, corner_pct)` | 单张牌面，`level_rank` 是 `Rank` 编号（1=A…13=K，0 = 无级牌态），`corner_pct` 是角标字号百分比（`0` = 用主题 `cornerScale`，`100` = 素材原样） |
| `gf_theme_preview / gf_theme_back / gf_theme_unload` | 预览四张 / 牌背 / 卸载（换主题前先卸） |

## 无头回归

```powershell
node apps/web/test_restore.mjs
node apps/web/verify_wasm_hash.mjs
```

`test_restore.mjs` 在一个 Node 进程内跑完「两局 → 存档 → 丢弃 Instance → 新建 Instance 回放」，
断言恢复哈希与存档哈希相等且恢复后还能继续推进。这是 `apps/web` 的可自动化 gate。

## 主题工坊 `theme-studio.html`

单文件主题编辑器：`theme.json` 的每个字段都摊成表单，右侧 15 张牌（2–A + 大小王）实时预览。

- **打开方式**：直接双击 `apps/web/theme-studio.html`（素材已内联，无任何外链），或走 `serve.ps1` 访问
  `http://127.0.0.1:8123/theme-studio.html`。
- **预览不是贴图**：页面里那份渲染内核逐条照 `render.rs` 实现（角标倍率重排、`cornerSuitPlacement` 的
  下方/右侧分支、classic 花阵的 `pipInset` 安全区与 `PIP_GAP` 自适应收窄）。用同一份配置与
  `gfdeck render` 对拍过三组（compact 220% below、compact 240% right、classic 240% 四角），
  所有 `translate/scale` 数值逐条相同。
- **级联**：`layout=classic` 才露花阵参数、勾「启用文字」才露字体与位置、角标花色选「右侧」时把失效的
  `cornerSuit.x/y` 变灰、取消全部角标会有告警。
- **素材补录**：23 个 SVG 位都能替换，上传后按目标框（花色 100×100、点数 100×140、其余 500×700）
  等比缩进并居中；关掉自适应就按素材原始尺寸摆，位置会走样。
- **进出配置**：默认值即 `crates/deckart/themes/defaults/theme.json`，可导入已有 `theme.json` 继续改，导出/复制新配置。
  导出只给 JSON，素材文件仍需按 JSON 里的路径放进主题目录，再 `gfdeck pack` 打成 `.gfdeck`。

## 已知边界

- 花色显示顺序按引擎 id 编码 `(suit×13+rank)×2+copy` 映射为 ♠♥♦♣；纯表现层，不参与判定。
- 对局中其他座位的牌面不下发（座位视图只含本人手牌）；座位卡上的「手牌 N 张」来自房间公开信息
  `gf_room_info`（真实牌桌同样可见张数），名次同理。结算阶段为了摊牌，客户端会读取
  `room.view(seat)` 里各座位剩余的 `hand.cards`，只用于结算展示，不参与后续判定。
- 「连续两家过牌后收起上家牌面」是纯表现层规则，按 `passesSince >= 2` 折叠，不影响可出牌判定。
- 人类超时托管走房间的 `Timeout` 分支：页面按 400ms 轮询 `step`，到点由房间代打。
- bot 在发牌时绑定，档位开局后锁定；换档位要回大厅重开。
- 推荐拆分（`gf_room_plan`）只按本座位手牌算，纯只读、不写状态、不进 `stateHash`；牌力比较一律用房间算好的
  「牌力值」（级牌按 `levelInsert` 提升、王最大），bot 不按原始点数猜，否则会把级牌当小牌、出的手被房间判非法。
