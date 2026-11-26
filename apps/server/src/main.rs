//! 服务端入口：`GF_BIND` 等环境变量见 `ServerConfig::from_env`。

use std::net::SocketAddr;

use gf_server::{App, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = ServerConfig::from_env();
    if cfg.uses_dev_secrets() {
        // 不阻塞启动（本机联调要能直接跑），但必须显眼：
        // 固定盐/密钥意味着谁拿到仓库就能伪造 token、反查手机号哈希。
        eprintln!(
            "警告：GF_AUTH_SALT / GF_JWT_SECRET 仍是开发默认值，只能用于本机联调；\
             上线前必须由环境变量或密钥管理服务覆盖（§8.5）。"
        );
    }
    if cfg.sms_test_whitelist.is_empty() && !cfg.require_auth {
        println!("提示：GF_SMS_TEST_WHITELIST 为空，短信通道未报备前只有匿名本机联调可用。");
    }
    let addr: SocketAddr = cfg.bind.parse()?;
    let app = App::new(cfg);
    println!("gf-server 启动，房间主循环 20Hz");
    gf_server::serve_on(app, addr).await?;
    Ok(())
}
