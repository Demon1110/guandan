//! 服务端入口：`GF_BIND` 等环境变量见 `ServerConfig::from_env`。

use std::net::SocketAddr;

use gf_server::{App, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = ServerConfig::from_env();
    let addr: SocketAddr = cfg.bind.parse()?;
    let app = App::new(cfg);
    println!("gf-server 启动，房间主循环 20Hz");
    gf_server::serve_on(app, addr).await?;
    Ok(())
}
