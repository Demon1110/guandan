# apps/desktop

掼蛋的桌面壳，Tauri 2，目标三平台：Windows / Linux / macOS。
**壳层不加业务逻辑**：玩法、渲染、存档语义都在 `apps/web`（纯静态资源）与 `crates/client-wasm`，这里只负责窗口、WebView 生命周期与安装包。

## 目录

```
apps/desktop/
├── build-installer.ps1       一键出包（cargo build --release + makensis）
├── installer/installer.nsi   自维护 NSIS 脚本（装 LocalAppData、卸载项、快捷方式）
├── dist/                     产物（已 gitignore）
└── src-tauri/
    ├── Cargo.toml            独立 workspace（不并入根 workspace，避免 Tauri 依赖树污染测试与覆盖率）
    ├── build.rs              tauri-build
    ├── tauri.conf.json       frontendDist 指向 ../../web（同级复用的前端）；bundle.targets = "all"（NSIS/AppImage/deb/dmg）
    ├── capabilities/         Tauri 2 权限集，只放 core:default
    ├── icons/                cargo tauri icon 生成的全套图标（app-icon.png 是源图）
    └── src/
        ├── lib.rs            壳入口（setup + run）
        └── main.rs           windows_subsystem = "windows"，release 不弹控制台
```

## 打包（Windows，当前可用路径）

```powershell
cd apps\desktop
pwsh -NoProfile -File .\build-installer.ps1            # cargo build --release + makensis
pwsh -NoProfile -File .\build-installer.ps1 -SkipBuild  # 只重打包
# 产物：apps\desktop\dist\Guandan-0.1.0-x64-setup.exe
```

**为什么不用 `cargo tauri build --bundles nsis`**：tauri-bundler 打包前会从 GitHub 拉 `nsis-3.13.zip`，本机实测 `timeout: global`；且它校验的文件清单要求定制包里的 `nsis_tauri_utils.dll`，官方 NSIS 包不满足，缓存放了也会被判定「missing files → Recreating it」并重新下载。所以这条路改成：官方 NSIS 3.13 + 自维护 `installer/installer.nsi`，链路完全离线。

NSIS 工具准备（一次性）：把 `nsis-3.13.zip` 解压到 `<repo>\target\nsis-tool`，内层目录名保持 `nsis-3.13`。
国内可用代理：`https://ghfast.top/https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.13/nsis-3.13.zip`

`installer/installer.nsi` 内容：`RequestExecutionLevel user`、装到 `$LOCALAPPDATA\Guandan`、写 HKCU 卸载项、开始菜单与桌面快捷方式、`Uninstall.exe`、WebView2 运行时缺失时提示（不阻断）。脚本是 UTF-8，`makensis` 必须带 `-INPUTCHARSET UTF8`（否则报 `Bad text encoding`）。

无前端构建链：`apps/web` 是纯静态目录（`index.html` / `app.js` / `styles.css` / `gf_client_wasm.wasm`，以及可选的 `online.html` / `online.css` / `online.js`），直接作为 `frontendDist` 打进 exe。本地 bot 走 `tauri.localhost` 同源加载；联机页是第二入口，额外连本机 8090 的 REST/WS。

## 验收方式（无人工点击）

WebView2 支持远程调试，用 CDP 驱动页面即可回归：

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9333"
Start-Process .\dist\Guandan-0.1.0-x64-setup.exe   # 或安装后的 guandan-desktop.exe
# 然后 node 脚本连 http://127.0.0.1:9333/json 取 page target，Runtime.evaluate 点 #lobby-start、读 #match-id / #hash
```

已实测（2026-11-26，Windows，WebView2 157.0.4341.42）：
- 静默安装 `/S /D=<dir>` 成功，注册表卸载项、`uninstall.exe` 齐全，静默卸载后无残留
- 启动 → 大厅 → 点「开始比赛」 → 牌桌：`matchId=web-51ed0000a106`、`stateHash=97775bdd…de67`、我方 27 张手牌、出牌/提示/过牌/组牌建议齐全
- 安装包 1.64 MB（LZMA），解包 exe 3.4 MB（含 1 MB wasm + 联机页静态资源）

## 三端打包的平台约束

- **不能在 Windows 上交叉产出 macOS / Linux 包**：macOS 必须在 macOS 机器或 macOS CI runner 上打（且需 Apple Developer 证书 + `notarytool` 公证）；Linux 在 Ubuntu runner 上打 AppImage / deb。
- Windows 包在本机用 `build-installer.ps1` 出；CI 里网络可达，走 `cargo tauri build --bundles nsis`，产物在 `src-tauri/target/release/bundle/nsis`。
- macOS 公证与 Windows 代码签名 secrets 要在 CI 里第一周就配好（对应风险册 R9：别留到最后一月）。

## CI（.github/workflows/desktop.yml）

四格 matrix：`windows-latest` / `ubuntu-22.04` / `macos-14（arm64）` / `macos-15-intel（x64）`。

- 触发：手动 `workflow_dispatch`、`v*` tag，或改动 `apps/desktop` / `apps/web` / workflow 本身。
- Linux 先装 `libwebkit2gtk-4.1-dev` 等系统依赖；三端统一 `cargo install tauri-cli` 后 `cargo tauri build --bundles …`。
- macOS 的 `APPLE_CERTIFICATE` / `APPLE_CERTIFICATE_PASSWORD` / `APPLE_SIGNING_IDENTITY` / `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` 从 secrets 注入；未配置时仍出未签名 dmg，用于内部冒烟，不用于外发。
- 产物按 `guandan-desktop-<platform>` 上传 artifact，不再依赖本机 `dist/`。

桌面壳的 CSP 已允许 `http://localhost:*` / `ws://localhost:*`：联机大厅在壳里可以连本机服务端；要连远端域名需在 `tauri.conf.json` 的 `connect-src` 增加对应 origin，不要为省事放成 `*`。

## 待办（骨架之后的适配项）

1. 存档载体：`apps/web/app.js` 现在用 IndexedDB；设计稿要求桌面壳用**本地文件**，需把存取抽成 storage 接口（浏览器 IndexedDB / 桌面文件两实现），否则杀进程续局在壳里要重做。
2. 关闭右键菜单、文本选择、浏览器快捷键，窗口最小尺寸与 DPR 适配。
3. CI matrix：windows-latest / ubuntu-latest / macos-latest 出三端产物 + 签名公证（workflow 已落，首次跑需补 secrets 与 Linux 冒烟）。
4. Linux WebKitGTK 差异是 CI 覆盖不了的风险，需在 Ubuntu 桌面手动冒烟一次（四端冒烟清单 §10.7）。
