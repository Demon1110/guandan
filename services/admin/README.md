# services/admin — 掼蛋运营后台

给运营同学用的网页后台：查账号、停用/启用账号，开关玩法与模式、调续局间隔，看已结束对局和后台写操作审计。管理能力全部来自 `services/server` 的 `/api/admin/*`，本工程只做界面与交互，不直接连数据库。

## 技术栈

- Vue 3 + Vue Router（hash 路由）
- Vite + TypeScript，`vue-tsc --noEmit` 做类型检查
- 自建 CSS 设计系统（`src/styles.css`），没有引入 Element Plus 等组件库
- 运行时依赖只有 `vue` 与 `vue-router`

## 本地启动

先起接口服务，再起前端：

```powershell
# 1) 接口服务，默认 8080
cd D:\java\git\guandan
$env:GF_ADMIN_TOKEN = "gf-dev-admin-token"
cargo run -p gf-server

# 2) 后台前端，默认 5180
cd services\admin
npm install
npm run dev
```

打开 <http://localhost:5180/>，在登录页输入管理令牌（本地默认 `gf-dev-admin-token`）即可进入。

开发服务器把 `/api` 代理到 `http://127.0.0.1:8080`（见 `vite.config.ts`）；接口服务换端口时改代理目标，或给前端配 `VITE_API_BASE`。

## 环境变量

| 变量 | 作用 |
| --- | --- |
| `VITE_API_BASE` | 接口前缀，默认 `/api`。前后端分开部署、又没走同域反代时用它指向完整地址 |

管理令牌不写进前端代码：登录成功后只存在当前标签页的 `sessionStorage`（键 `gf.admin.token`），关标签页即失效。接口返回 401 时前端会清掉令牌并跳回登录页。

## 页面

| 路由 | 页面 | 能做什么 |
| --- | --- | --- |
| `/#/login` | 登录 | 校验管理令牌 |
| `/#/` | 总览 | 账号规模、近 24h 新增与对局、在线房间、存储与 Redis 状态、当前玩法摘要 |
| `/#/users` | 用户管理 | 按玩家 ID / 昵称 / 手机尾号检索，按启用状态筛选，分页，启用或禁用账号 |
| `/#/games` | 玩法配置 | 展示名、对外开关、开放模式（休闲 / 排位 / 混合）、结算续局间隔；保存后立即生效 |
| `/#/matches` | 对局记录 | 已结束场次只读列表：场次、模式、胜方、结束原因、开始时间、时长 |
| `/#/audits` | 审计日志 | 最近 100 条后台写操作，按用户 / 玩法筛选 |

几处业务边界，界面里也写明了：

- 停用账号后该玩家的旧 access token 立即失效，且不能再登录或刷新令牌；重新启用后恢复。
- 关闭玩法只拦**新建房间和新匹配入队**（`403 GAME_DISABLED`），已经在跑的房间仍可重连收尾。
- 关闭某个模式后，该模式的建房 / 入队返回 `403 GAME_MODE_DISABLED`；至少保留一个模式。
- 续局间隔只影响之后新建的房间，已开局房间保持原节奏；范围 1 秒–10 分钟。
- 规则集注册、dry-run、灰度和回滚属于 M3，这一版不做。

## 构建与部署

```powershell
cd services\admin
npm run build      # vue-tsc --noEmit && vite build，产物在 dist\
npm run preview    # 本地预览构建产物
```

`dist` 是纯静态文件，交给 Nginx 或任意静态服务器即可。生产上建议让站点和接口同域：静态文件走根路径，`/api` 反代到 `services/server`，这样不用配 `VITE_API_BASE`，也不用处理跨域。

`services/server` 目前没有内置静态托管，也没有开 CORS；如果前后端不同域，需要在网关层补 CORS，或把前端放到同域反代后面。

## 目录

```
services/admin/
├─ index.html
├─ vite.config.ts          # 5180 端口，/api 代理到 8080
├─ src/
│  ├─ main.ts / App.vue
│  ├─ router.ts            # hash 路由 + 登录守卫
│  ├─ api.ts               # 令牌存取、统一错误文案、401 处理
│  ├─ types.ts             # 与 /api/admin/* 对应的数据结构
│  ├─ styles.css           # 设计系统与全部页面样式
│  ├─ layouts/AdminLayout.vue
│  ├─ components/          # Toast、确认弹窗、状态胶囊、空态、分页、导航图标
│  ├─ lib/                 # 时间/模式等格式化、toast 状态
│  └─ views/               # 登录、总览、用户、玩法、对局、审计
└─ dist/                   # 构建产物，不入库
```

## 接口对照

管理令牌走 `Authorization: Bearer <GF_ADMIN_TOKEN>`，与玩家 JWT 完全分离。令牌留空时服务端关闭整个后台（`503 ADMIN_DISABLED`），前端登录会直接提示。

| 方法 | 路径 | 前端用途 |
| --- | --- | --- |
| `GET` | `/api/admin/session` | 登录校验、页脚规则集信息 |
| `GET` | `/api/admin/overview` | 总览指标 |
| `GET` | `/api/admin/users` | 用户列表（`keyword` / `status` / `page` / `pageSize`） |
| `PATCH` | `/api/admin/users/{playerId}` | 启用 / 禁用 |
| `GET` | `/api/admin/games` | 玩法配置 |
| `PUT` | `/api/admin/games/guandan` | 保存玩法配置 |
| `GET` | `/api/admin/matches` | 对局记录 |
| `GET` | `/api/admin/audits` | 审计日志 |

完整契约、错误码和没有 PG 时的降级行为见 `services/server/README.md` 的「运营后台接口」一节。
