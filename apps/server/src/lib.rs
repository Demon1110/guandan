//! 掼蛋联机服务端（M2 首个切片）。
//!
//! 本切片交付的东西：
//! - HTTP：`/healthz`、`/api/rooms`（建房 / 查房）；
//! - WS：`/ws`，protobuf over WebSocket（帧头见 `gf_protocol::wire`）；
//! - 房间：满四人自动发牌，动作判定走 `gf-table-runtime::Room`，结算广播，
//!   断线重连给全量快照。
//!
//! 明确不在本切片：账号与短信、匹配队列、PG/Redis 落库、回放导出、观战。
//! 这些按 `docs/开发计划.md` §3 M2 的顺序排在后面的切片。

pub mod config;
pub mod events;
pub mod host;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use gf_protocol::gf::protocol::v1 as pb;
use gf_protocol::wire;
use prost::Message as _;
use serde::Deserialize;
use tokio::sync::{mpsc, RwLock};

pub use config::ServerConfig;
pub use host::{JoinError, RoomHost, RoomOptions};

/// 进程级共享状态。
pub struct App {
    cfg: ServerConfig,
    rooms: RwLock<HashMap<String, Arc<RoomHost>>>,
    room_seq: AtomicU64,
    started_at: Instant,
}

impl App {
    /// 建进程状态。
    pub fn new(cfg: ServerConfig) -> Arc<Self> {
        Arc::new(Self {
            cfg,
            rooms: RwLock::new(HashMap::new()),
            room_seq: AtomicU64::new(1),
            started_at: Instant::now(),
        })
    }

    /// 取房间；不存在则按需建房（本机联调期的「进房即建房」）。
    pub async fn room_or_create(&self, match_id: &str) -> Arc<RoomHost> {
        if let Some(room) = self.rooms.read().await.get(match_id).cloned() {
            return room;
        }
        let mut rooms = self.rooms.write().await;
        if let Some(room) = rooms.get(match_id).cloned() {
            return room;
        }
        let seed = self.room_seq.fetch_add(1, Ordering::Relaxed);
        let opts = RoomOptions {
            tick_ms: self.cfg.tick_ms,
            next_deal_ms: self.cfg.next_deal_ms,
        };
        let room = RoomHost::spawn(match_id.to_owned(), seed, opts).expect("经典规则集建桌失败");
        rooms.insert(match_id.to_owned(), room.clone());
        room
    }

    /// 建房（返回可用房间号）。房间号由调用方给定，便于房间码分享。
    pub async fn create_room(&self, match_id: &str) -> Arc<RoomHost> {
        self.room_or_create(match_id).await
    }

    /// 路由表。
    pub fn router(self: &Arc<Self>) -> Router {
        Router::new()
            .route("/healthz", get(healthz))
            .route("/api/rooms", post(create_room))
            .route("/ws", get(ws_upgrade))
            .with_state(self.clone())
    }
}

/// 启动监听；`addr` 由调用方给出（`0` 端口可用于测试）。
pub async fn serve_on(app: Arc<App>, addr: SocketAddr) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    println!("gf-server 监听 {local}");
    axum::serve(listener, app.router()).await
}

#[derive(Deserialize)]
struct CreateRoomBody {
    match_id: String,
}

async fn healthz(State(app): State<Arc<App>>) -> impl IntoResponse {
    let rooms = app.rooms.read().await.len();
    Json(serde_json::json!({
        "status": "ok",
        "rooms": rooms,
        "uptime_ms": app.started_at.elapsed().as_millis() as u64,
        "ruleset_version": gf_table_runtime::DEFAULT_RULESET_VERSION,
    }))
}

async fn create_room(
    State(app): State<Arc<App>>,
    Json(body): Json<CreateRoomBody>,
) -> impl IntoResponse {
    if body.match_id.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"reason": "RS_SCHEMA_INVALID"})));
    }
    let room = app.create_room(body.match_id.trim()).await;
    (
        StatusCode::OK,
        Json(serde_json::json!({"match_id": room.match_id(), "seats": host::SEATS})),
    )
}

async fn ws_upgrade(ws: WebSocketUpgrade, State(app): State<Arc<App>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| session(app, socket))
}

/// 单条 WS 连接的生命周期：入座 → 收动作/重连 → 断开记账。
async fn session(app: Arc<App>, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let writer = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            if sink.send(Message::Binary(frame.into())).await.is_err() {
                break;
            }
        }
    });

    let mut bound: Option<(Arc<RoomHost>, u8)> = None;
    let idle = std::time::Duration::from_millis(app.cfg.heartbeat_timeout_ms);

    loop {
        let next = tokio::time::timeout(idle, stream.next()).await;
        let msg = match next {
            Ok(Some(Ok(msg))) => msg,
            // 心跳超时 / 对端关闭 / 传输错误：都按断线处理。
            Ok(Some(Err(_))) | Ok(None) | Err(_) => break,
        };
        let bytes = match msg {
            Message::Binary(bytes) => bytes,
            Message::Close(_) => break,
            // 文本帧不是本协议载体；Ping/Pong 由 axum 处理。
            _ => continue,
        };

        let (kind, body) = match wire::split(&bytes) {
            Ok(parts) => parts,
            Err(_) => break,
        };

        match kind {
            wire::kind::JOIN_ROOM => {
                let Ok(join) = pb::JoinRoom::decode(body) else { break };
                if bound.is_some() {
                    // 一条连接只允许入座一次；重复 JoinRoom 视为协议违规。
                    break;
                }
                if !app.cfg.accepts_version(&join.client_version) {
                    // TODO(M2-OPEN-02)：设计要求连接级错误走 WS Close code，本切片先借 JoinAck 载体。
                    let _ = tx.send(events::join_ack_frame(
                        &join.match_id,
                        0,
                        false,
                        &[],
                        Some("CLIENT_TOO_OLD"),
                    ));
                    break;
                }
                let room = app.room_or_create(&join.match_id).await;
                let hint = join.seat.and_then(|s| u8::try_from(s).ok());
                match room.join(hint, tx.clone()).await {
                    Ok(seat) => {
                        let _ = tx.send(events::join_ack_frame(
                            &join.match_id,
                            seat,
                            true,
                            &[],
                            None,
                        ));
                        // 入座回执先发，再按「满四人」开局；新座位随后拿全量快照。
                        room.start_if_full().await;
                        if let Ok(snapshot) = room.join_snapshot(seat).await {
                            let _ = tx.send(snapshot);
                        }
                        bound = Some((room, seat));
                    }
                    Err(JoinError::SeatTaken { available }) => {
                        let _ = tx.send(events::join_ack_frame(
                            &join.match_id,
                            0,
                            false,
                            &available,
                            Some("RS_SEAT_TAKEN"),
                        ));
                    }
                }
            }
            wire::kind::SUBMIT_ACTION => {
                let Some((room, seat)) = &bound else { break };
                let Ok(mut submit) = pb::SubmitAction::decode(body) else { break };
                if let Ok(msg_seat) = u8::try_from(submit.seat) {
                    if msg_seat != *seat {
                        // §5.9：请求 seat 与绑定座位不一致 → AUTH_INVALID，并记指标。
                        let _ = tx.send(events::action_ack_frame(
                            &submit.request_id,
                            room.seat_watermark(*seat).await,
                            room.room_seq().await,
                            false,
                            Some("AUTH_INVALID"),
                        ));
                        continue;
                    }
                }
                submit.match_id = room.match_id().to_owned();
                let ack = room.submit(*seat, submit, room.now_ms()).await;
                let _ = tx.send(ack);
            }
            wire::kind::RECONNECT => {
                let Ok(re) = pb::Reconnect::decode(body) else { break };
                let room = app.room_or_create(&re.match_id).await;
                let Ok(seat) = u8::try_from(re.seat) else { break };
                if !room.rebind(seat, tx.clone()).await {
                    break;
                }
                match room.reconnect(seat, re.last_seat_seq).await {
                    Ok(frames) => {
                        // 增量帧在前、快照在后，必须按顺序下发。
                        for frame in frames {
                            let _ = tx.send(frame);
                        }
                        bound = Some((room, seat));
                    }
                    Err(_) => break,
                }
            }
            wire::kind::HEARTBEAT => {
                // 收到即刷新本连接的空闲计时；无下行。
                if pb::Heartbeat::decode(body).is_err() {
                    break;
                }
            }
            // S→C 帧不合法出现在 C→S 方向。
            _ => break,
        }
    }

    if let Some((room, seat)) = bound {
        room.leave(seat, room.now_ms()).await;
    }
    // 让写侧把队列里的帧刷完再收尾（直接 abort 会截断最后几帧）。
    drop(tx);
    let _ = writer.await;
}
