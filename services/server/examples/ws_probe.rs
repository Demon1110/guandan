//! 手工探针：连本机服务端并发一条 JoinRoom，打印收到的帧。
//!
//! 用法：先 `cargo run --bin gf-server`，再 `cargo run --example ws_probe`。

use futures_util::{SinkExt, StreamExt};
use gf_protocol::gf::protocol::v1 as pb;
use gf_protocol::wire;
use prost::Message as _;
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "ws://127.0.0.1:8080/ws".to_owned());
    let (mut socket, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("连接失败");
    println!("已连接 {url}");

    let join = pb::JoinRoom {
        match_id: "probe".to_owned(),
        seat: None,
        client_version: "0.1.0".to_owned(),
    };
    socket
        .send(Message::Binary(
            wire::encode(wire::kind::JOIN_ROOM, &join.encode_to_vec()).into(),
        ))
        .await
        .expect("发送失败");

    for _ in 0..3 {
        match tokio::time::timeout(std::time::Duration::from_secs(3), socket.next()).await {
            Ok(Some(Ok(Message::Binary(bytes)))) => {
                let (kind, body) = wire::split(&bytes).expect("拆帧失败");
                println!("收到 kind={kind} body_len={}", body.len());
            }
            Ok(Some(Ok(other))) => println!("收到非二进制：{other:?}"),
            Ok(Some(Err(err))) => {
                println!("读错误：{err}");
                break;
            }
            Ok(None) => {
                println!("对端关闭");
                break;
            }
            Err(_) => {
                println!("等待超时");
                break;
            }
        }
    }
}
