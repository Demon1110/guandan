//! 服务端入口：配置默认读工作目录的 `config.local.toml` / `config.toml`，
//! 路径与字段说明见 [`gf_server::config`]；环境变量可覆盖部署相关项。

use std::net::SocketAddr;

use gf_server::{App, ServerConfig, Store};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = ServerConfig::load()?;
    println!("配置：{}", cfg.summary());
    if cfg.uses_dev_secrets() {
        // 不阻塞启动（本机联调要能直接跑），但必须显眼：
        // 固定盐/密钥意味着谁拿到仓库就能伪造 token、反查手机号哈希。
        eprintln!(
            "警告：[security] auth_salt / jwt_secret 仍是开发默认值，只能用于本机联调；\
             上线前必须由 config.local.toml 或环境变量覆盖（§8.5）。"
        );
    }
    if !cfg.require_auth {
        eprintln!(
            "警告：[security] require_auth=false：玩家侧接口与 WS 允许匿名访问；\
             生产环境（尤其 sms.provider={}）必须开启 GF_REQUIRE_AUTH=true。",
            cfg.sms.provider
        );
    }
    if cfg.sms.provider == "log" && cfg.sms.test_whitelist.is_empty() && !cfg.require_auth {
        println!("提示：[sms] 还没接真实通道、也没有测试白名单，短信登录只有日志验证码可用。");
    }
    let addr: SocketAddr = cfg.bind.parse()?;
    let store = Store::connect_with(&cfg.database, &cfg.redis).await;
    let app = App::new(cfg);
    app.attach_store(store).await;
    println!("gf-server 启动，房间主循环 20Hz");
    gf_server::serve_on(app, addr).await?;
    Ok(())
}
