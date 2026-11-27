//! WS 传输层冒烟：真实 WebSocket 握手 → 入座 → 收到入座回执与全量快照。

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use gf_protocol::gf::protocol::v1 as pb;
use gf_protocol::wire;
use prost::Message as _;
use tokio_tungstenite::tungstenite::Message;

use gf_server::{App, ServerConfig};

async fn connect(
    addr: std::net::SocketAddr,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let url = format!("ws://{addr}/ws");
    let (socket, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("WS 连接失败");
    socket
}

async fn next_frame(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Option<(u16, Vec<u8>)> {
    loop {
        eprintln!("[client] 等待帧…");
        let msg = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("等帧超时")?;
        match msg {
            Ok(msg) => match msg {
                Message::Binary(bytes) => {
                    let (kind, body) = wire::split(&bytes).expect("帧头应可拆");
                    return Some((kind, body.to_vec()));
                }
                Message::Close(_) => return None,
                other => {
                    eprintln!("[client] 非二进制帧：{other:?}");
                    continue;
                }
            },
            // 服务端主动断开（含鉴权失败）表现为读错误，统一当连接结束。
            Err(_) => return None,
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn four_clients_join_over_websocket() {
    let app = App::new(ServerConfig {
        tick_ms: 50,
        next_deal_ms: 5_000,
        ..ServerConfig::default()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定随机端口失败");
    let addr = listener.local_addr().expect("取本地地址失败");
    let router = app.router();
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("服务启动失败");
    });

    let mut clients = Vec::new();
    let mut seat_tokens = Vec::new();
    let mut seen_snapshots = Vec::new();

    // 先四座位全部入座（各自的入座回执必须第一帧到）。
    for expect_seat in 0..4u32 {
        let mut socket = connect(addr).await;
        let join = pb::JoinRoom {
            match_id: "match-ws".to_owned(),
            seat: None,
            seat_token: String::new(),
            client_version: "0.1.0".to_owned(),
        };
        socket
            .send(Message::Binary(
                wire::encode(wire::kind::JOIN_ROOM, &join.encode_to_vec()).into(),
            ))
            .await
            .expect("发入座帧失败");

        let (kind, body) = next_frame(&mut socket).await.expect("连接提前关闭");
        assert_eq!(kind, wire::kind::JOIN_ACK, "入座回执必须是第一帧");
        let ack = pb::JoinAck::decode(&body[..]).expect("JoinAck 解码失败");
        assert!(ack.accept, "入座应被接受");
        assert_eq!(ack.seat, expect_seat, "座位按最小空位分配");
        assert!(ack.reason.is_none());
        assert!(!ack.seat_token.is_empty(), "匿名入座必须下发重连令牌");
        seat_tokens.push(ack.seat_token);
        clients.push(socket);
    }

    // 满员发牌：每个座位各自应收到本座位的全量快照。
    for (expect_seat, socket) in clients.iter_mut().enumerate() {
        let mut snapshot = None;
        for _ in 0..4 {
            let Some((kind, body)) = next_frame(socket).await else {
                break;
            };
            if kind == wire::kind::ROOM_SNAPSHOT {
                snapshot = Some(pb::RoomSnapshot::decode(&body[..]).expect("快照解码失败"));
                break;
            }
        }
        let snapshot = snapshot.expect("满员开局必须下发快照");
        assert_eq!(snapshot.seat, expect_seat as u32);
        assert!(snapshot.snapshot_reset);
        assert_eq!(snapshot.ruleset_version, "guandan-classic-1.0.0");
        assert_eq!(snapshot.round_no, 1, "开局局号应为 1");
        assert!(!snapshot.view.is_empty(), "视图不得为空");
        seen_snapshots.push(snapshot);
    }

    // 匿名重连必须出示座位令牌：无令牌的 RECONNECT 不能顶掉座位 0。
    clients[0].close(None).await.expect("断开座位 0");
    tokio::time::sleep(Duration::from_millis(80)).await;

    let mut attacker = connect(addr).await;
    let hijack = pb::Reconnect {
        match_id: "match-ws".to_owned(),
        seat: 0,
        last_seat_seq: 0,
        seat_token: String::new(),
    };
    attacker
        .send(Message::Binary(
            wire::encode(wire::kind::RECONNECT, &hijack.encode_to_vec()).into(),
        ))
        .await
        .expect("发重连帧失败");
    assert!(
        next_frame(&mut attacker).await.is_none(),
        "匿名重连缺座位令牌必须被断开"
    );

    // 本人带令牌可以重连，并拿到增量/快照。
    let mut owner = connect(addr).await;
    let legit = pb::Reconnect {
        match_id: "match-ws".to_owned(),
        seat: 0,
        last_seat_seq: 0,
        seat_token: seat_tokens[0].clone(),
    };
    owner
        .send(Message::Binary(
            wire::encode(wire::kind::RECONNECT, &legit.encode_to_vec()).into(),
        ))
        .await
        .expect("发重连帧失败");
    let (kind, _) = next_frame(&mut owner).await.expect("本人重连应收到帧");
    assert!(
        matches!(kind, wire::kind::ROOM_DELTA | wire::kind::ROOM_SNAPSHOT),
        "重连应补发增量或快照，实际 {kind}"
    );
    let _ = owner.close(None).await;

    for socket in &mut clients {
        let _ = socket.close(None).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_too_old_is_refused() {
    let app = App::new(ServerConfig {
        min_client_version: "9.9.9".to_owned(),
        ..ServerConfig::default()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定随机端口失败");
    let addr = listener.local_addr().expect("取本地地址失败");
    let router = app.router();
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("服务启动失败");
    });

    let mut socket = connect(addr).await;
    let join = pb::JoinRoom {
        match_id: "match-old".to_owned(),
        seat: None,
        seat_token: String::new(),
        client_version: "0.1.0".to_owned(),
    };
    socket
        .send(Message::Binary(
            wire::encode(wire::kind::JOIN_ROOM, &join.encode_to_vec()).into(),
        ))
        .await
        .expect("发入座帧失败");

    let (kind, body) = next_frame(&mut socket).await.expect("应收到拒绝帧");
    assert_eq!(kind, wire::kind::JOIN_ACK);
    let ack = pb::JoinAck::decode(&body[..]).expect("解码失败");
    assert!(!ack.accept);
    assert_eq!(ack.reason.as_deref(), Some("CLIENT_TOO_OLD"));
}
