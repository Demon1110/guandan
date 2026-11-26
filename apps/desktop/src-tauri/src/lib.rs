// 桌面壳入口：只负责窗口与 WebView 生命周期。
// 玩法、渲染、存档语义全在 apps/client（纯静态资源）+ crates/client-wasm，壳层不加业务逻辑。

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| Ok(()))
        .run(tauri::generate_context!())
        .expect("error while running guandan desktop shell");
}
