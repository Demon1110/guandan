//! 掼蛋联机服务端（M2）。
//!
//! 已交付的切片：
//! - 联机骨架：HTTP 建房、WS protobuf、满四人发牌、动作判定、结算广播；
//! - 增量重连：`ReconnectBuffer` 内补发 `RoomDelta`，跨局/结算落快照重置；
//! - 大厅：房间码、凭码预占座位、匹配队列（内存态）；
//! - 账号：手机号 + 短信验证码登录、access/refresh token、测试白名单。
//!
//! 未做：PG/Redis 落库、回放导出、观战、桌面壳产物。

pub mod auth;
pub mod config;
pub mod events;
pub mod host;
pub mod lobby;
pub mod sms;
pub mod store;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use gf_protocol::gf::protocol::v1 as pb;
use gf_protocol::wire;
use prost::Message as _;
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex, RwLock};

pub use auth::{Auth, Profile, Session};
pub use config::{DatabaseConfig, RedisConfig, ServerConfig, SmsConfig};
pub use host::{JoinError, RoomHost, RoomOptions};
pub use lobby::{Lobby, Mode, TicketState};
pub use store::{RoundRow, StorageEvent, Store};

/// 本服务只承载掼蛋一个玩法（`GET /api/games` 的 `gameId` 取值）。
pub const GAME_ID: &str = "guandan";

/// 进程级共享状态。
pub struct App {
    cfg: ServerConfig,
    rooms: RwLock<HashMap<String, Arc<RoomHost>>>,
    /// 房间码与匹配队列（内存态；Redis 里另存一份镜像，存储见 `store`）。
    lobby: Mutex<Lobby>,
    /// 账号、验证码与令牌（内存态；PG 落库走 `store`）。
    auth: Auth,
    /// 持久化句柄：`App::new` 时不接，`attach_store` 时装一次（进程内只装一次）。
    store: std::sync::OnceLock<Store>,
    /// 房间 → 存档任务的通道；未挂存储时事件被丢弃。
    storage_tx: mpsc::UnboundedSender<StorageEvent>,
    storage_rx: Mutex<Option<mpsc::UnboundedReceiver<StorageEvent>>>,
    room_seq: AtomicU64,
    started_at: Instant,
}

impl App {
    /// 建进程状态。
    pub fn new(cfg: ServerConfig) -> Arc<Self> {
        let auth = Auth::new(
            cfg.auth_salt.clone(),
            cfg.jwt_secret.clone(),
            cfg.sms.test_whitelist.clone(),
            cfg.sms.test_code.clone(),
            sms::build_sender(&cfg.sms),
        );
        let (storage_tx, storage_rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            cfg,
            rooms: RwLock::new(HashMap::new()),
            lobby: Mutex::new(Lobby::new()),
            auth,
            store: std::sync::OnceLock::new(),
            storage_tx,
            storage_rx: Mutex::new(Some(storage_rx)),
            room_seq: AtomicU64::new(1),
            started_at: Instant::now(),
        })
    }

    /// 接上 PG/Redis（`main` 在建 `App` 之后调用；单测不调用即为纯内存态）。
    pub async fn attach_store(self: &Arc<Self>, store: Store) {
        if !store.enabled() {
            return;
        }
        if self.store.set(store).is_err() {
            eprintln!("[store] 已接过存储句柄，忽略重复接入");
            return;
        }
        // 已经建好的房间补挂存档通道（正常顺序是先 attach 再建房，这里只兜底）。
        for room in self.rooms.read().await.values() {
            room.attach_recorder(self.storage_tx.clone());
        }
        let rx = self
            .storage_rx
            .try_lock()
            .ok()
            .and_then(|mut guard| guard.take());
        let Some(mut rx) = rx else { return };

        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let Some(app) = weak.upgrade() else { break };
                app.persist(event).await;
            }
        });

        // 房间快照按配置刷新（默认 30s），TTL 取 `redis.snapshot_ttl_ms`；只对 Redis 有意义。
        let interval_ms = self.cfg.redis.snapshot_interval_ms.max(1_000);
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_millis(interval_ms));
            loop {
                ticker.tick().await;
                let Some(app) = weak.upgrade() else { break };
                app.flush_snapshots().await;
            }
        });
    }

    fn store(&self) -> Option<&Store> {
        self.store.get().filter(|store| store.enabled())
    }

    /// 处理一条存档事件：补座位归属 → 写 PG；动作流写 Redis。
    async fn persist(&self, event: StorageEvent) {
        let Some(store) = self.store() else { return };
        match event {
            StorageEvent::RoundSettled {
                match_id,
                mut row,
                actions,
            } => {
                if let Some(room) = self.rooms.read().await.get(&match_id).cloned() {
                    row.players = seat_players(&room, &row.ranks).await;
                }
                let lines: Vec<String> = actions
                    .iter()
                    .map(|record| store::action_line(&match_id, record))
                    .collect();
                store.insert_round(&row).await;
                store.append_actions(&match_id, &lines).await;
            }
            StorageEvent::MatchEnded {
                match_id,
                winner_team,
                end_reason,
                ended_at_ms,
            } => {
                store
                    .finish_match(&match_id, winner_team, end_reason, ended_at_ms)
                    .await;
            }
        }
    }

    /// 所有在线房间的快照元信息写 Redis（不含手牌，只有可公开的重建水位）。
    async fn flush_snapshots(&self) {
        let Some(store) = self.store() else { return };
        let rooms: Vec<Arc<RoomHost>> = self.rooms.read().await.values().cloned().collect();
        for room in rooms {
            let Some((room_seq, round_no, state_hash, phase)) = room.snapshot_meta().await else {
                continue;
            };
            let state_hash: String = state_hash
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            let json = serde_json::json!({
                "matchId": room.match_id(),
                "roomSeq": room_seq,
                "roundNo": round_no,
                "stateHash": state_hash,
                "phase": phase,
                "updatedAt": now_unix_ms(),
            });
            store.put_snapshot(room.match_id(), &json.to_string()).await;
        }
    }

    /// 单调毫秒（进程起点起算）。
    pub fn now_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
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
        if self.store().is_some() {
            room.attach_recorder(self.storage_tx.clone());
        }
        rooms.insert(match_id.to_owned(), room.clone());
        room
    }

    /// 建房（返回可用房间号）。房间号由调用方给定，便于房间码分享。
    pub async fn create_room(&self, match_id: &str) -> Arc<RoomHost> {
        self.room_or_create(match_id).await
    }

    /// `POST /api/room/create` 的实现：生成场次号 → 建房 → 出房间码。
    pub async fn create_room_with_code(&self, mode: Mode) -> (Arc<RoomHost>, String) {
        let match_id = loop {
            let id = self.lobby.lock().await.new_match_id();
            if !self.rooms.read().await.contains_key(&id) {
                break id;
            }
        };
        let room = self.room_or_create(&match_id).await;
        if let Some(store) = self.store() {
            store
                .insert_match(
                    &match_id,
                    GAME_ID,
                    mode.match_type(),
                    host::SEATS,
                    now_unix_ms(),
                )
                .await;
        }
        let code = {
            let mut lobby = self.lobby.lock().await;
            lobby.issue_code(&match_id, self.now_ms(), self.cfg.room_code_ttl_ms)
        };
        (room, code)
    }

    /// 凭房间码找房间；失效码返回 `None`。
    pub async fn room_by_code(&self, code: &str) -> Option<Arc<RoomHost>> {
        let match_id = self.lobby.lock().await.resolve_code(code, self.now_ms())?;
        Some(self.room_or_create(&match_id).await)
    }

    /// 入匹配队列，返回票号。
    pub async fn enqueue(&self, mode: Mode) -> String {
        let ticket = self
            .lobby
            .lock()
            .await
            .enqueue(GAME_ID, mode, self.now_ms());
        if let Some(store) = self.store() {
            store
                .queue_upsert(GAME_ID, mode.as_str(), &ticket, now_unix_ms())
                .await;
        }
        ticket
    }

    /// 轮询票状态。
    pub async fn ticket_state(&self, ticket_id: &str) -> Option<(TicketState, Option<String>)> {
        let now = self.now_ms();
        self.lobby
            .lock()
            .await
            .poll(ticket_id, now, self.cfg.match_timeout_ms)
    }

    /// 退队；返回是否真的退掉（已配对/不存在都返回 false）。
    pub async fn cancel_ticket(&self, ticket_id: &str) -> bool {
        let now = self.now_ms();
        let (removed, mode) = self.lobby.lock().await.cancel_with_mode(ticket_id, now);
        if removed {
            if let (Some(store), Some(mode)) = (self.store(), mode) {
                store.queue_remove(GAME_ID, mode.as_str(), ticket_id).await;
            }
        }
        removed
    }

    /// 尝试凑桌：够四张同模式等待票即建房并返回 `(matchId, 票号)`。
    ///
    /// 每次入队与轮询都调用一次，代价是 handful 次比较，不需要后台撮合线程。
    pub async fn pump_match(&self, mode: Mode) -> Option<(String, Vec<String>)> {
        // 已知场次号先快照出来，避免在撮合锁里 await 房间表（锁序：lobby → rooms）。
        let taken: Vec<String> = self.rooms.read().await.keys().cloned().collect();
        let now = self.now_ms();
        let mut lobby = self.lobby.lock().await;
        let (match_id, group) = lobby.form_group(
            GAME_ID,
            mode,
            host::SEATS as usize,
            now,
            self.cfg.match_timeout_ms,
            &taken,
        )?;
        drop(lobby);
        self.room_or_create(&match_id).await;
        if let Some(store) = self.store() {
            store
                .insert_match(
                    &match_id,
                    GAME_ID,
                    mode.match_type(),
                    host::SEATS,
                    now_unix_ms(),
                )
                .await;
            for ticket in &group {
                store.queue_remove(GAME_ID, mode.as_str(), ticket).await;
            }
        }
        Some((match_id, group))
    }

    /// 路由表。
    pub fn router(self: &Arc<Self>) -> Router {
        Router::new()
            .route("/healthz", get(healthz))
            // 本机联调用的直连建房（玩家侧正式入口是 /api/room/create）。
            .route("/api/rooms", post(create_room))
            .route("/api/room/create", post(room_create))
            .route("/api/room/join", post(room_join))
            .route("/api/match/queue", post(match_queue))
            .route(
                "/api/match/queue/{ticket_id}",
                get(match_status).delete(match_cancel),
            )
            .route("/api/auth/sms", post(auth_sms))
            .route("/api/auth/login", post(auth_login))
            .route("/api/auth/refresh", post(auth_refresh))
            .route("/api/profile", get(profile))
            .route("/ws", get(ws_upgrade))
            .with_state(self.clone())
    }

    /// 从 `Authorization: Bearer ...` 取玩家身份。
    ///
    /// - 没带 token 且未开 `GF_REQUIRE_AUTH`：返回 `Ok(None)`（本机联调放行）；
    /// - 带了 token 但无效：一律 401（显式给了坏凭据不能当匿名）；
    /// - 开了强制鉴权却没带：401。
    ///
    /// `Err` 装箱：`Response` 超过 128 字节，直接塞进 `Result` 会被 clippy 判大错误体。
    fn authenticate(
        &self,
        headers: &HeaderMap,
    ) -> Result<Option<String>, Box<axum::response::Response>> {
        let raw = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok());
        match raw {
            Some(value) => {
                let token = value
                    .strip_prefix("Bearer ")
                    .or_else(|| value.strip_prefix("bearer "))
                    .map(str::trim)
                    .filter(|t| !t.is_empty());
                match token.and_then(|t| self.auth.verify_access(t, now_unix_ms())) {
                    Some(player_id) => Ok(Some(player_id)),
                    None => Err(Box::new(unauthorized())),
                }
            }
            None if self.cfg.require_auth => Err(Box::new(unauthorized())),
            None => Ok(None),
        }
    }
}

/// 墙钟毫秒：账号令牌的有效期要跨进程重启，不能用进程内单调时钟。
pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 按名次表回填 `(seat, player_id, rank)`：`player_id` 取座位归属，未登录座位为 `None`。
async fn seat_players(room: &Arc<RoomHost>, ranks: &[u8]) -> Vec<(u8, Option<String>, u8)> {
    let mut players = Vec::with_capacity(ranks.len());
    for (index, seat) in ranks.iter().enumerate() {
        players.push((*seat, room.seat_owner(*seat).await, (index + 1) as u8));
    }
    players
}

fn unauthorized() -> axum::response::Response {
    use axum::response::IntoResponse as _;
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"reason": "AUTH_INVALID"})),
    )
        .into_response()
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
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"reason": "RS_SCHEMA_INVALID"})),
        );
    }
    let room = app.create_room(body.match_id.trim()).await;
    (
        StatusCode::OK,
        Json(serde_json::json!({"match_id": room.match_id(), "seats": host::SEATS})),
    )
}

/// `POST /api/room/create`（§5.15）：建房并出房间码。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoomCreateBody {
    #[serde(default)]
    game_id: Option<String>,
    mode: String,
    #[serde(default)]
    seats_count: Option<u32>,
}

async fn room_create(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<RoomCreateBody>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    if let Err(resp) = app.authenticate(&headers) {
        return *resp;
    }
    let game_id = body.game_id.unwrap_or_else(|| GAME_ID.to_owned());
    if game_id != GAME_ID {
        return invalid_request("GAME_UNKNOWN");
    }
    let Some(mode) = Mode::parse(&body.mode) else {
        return invalid_request("MODE_UNKNOWN");
    };
    // 座位数由 RuleSet 的 `seats.count` 决定，客户端只能传同值（§5.15）。
    if body
        .seats_count
        .is_some_and(|n| n != u32::from(host::SEATS))
    {
        return invalid_request("SEATS_MISMATCH");
    }
    let (room, code) = app.create_room_with_code(mode).await;
    Json(serde_json::json!({
        "matchId": room.match_id(),
        "inviteCode": code,
        "matchType": mode.match_type(),
        "seatsCount": host::SEATS,
    }))
    .into_response()
}

/// `POST /api/room/join`（§5.15）：凭房间码预占座位，结果与 `JoinAck` 同构。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoomJoinBody {
    invite_code: String,
}

async fn room_join(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<RoomJoinBody>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    let owner = match app.authenticate(&headers) {
        Ok(owner) => owner,
        Err(resp) => return *resp,
    };
    let Some(room) = app.room_by_code(&body.invite_code).await else {
        // 错误码表里没有「邀请码不存在」，按入参不合规走 `RS_SCHEMA_INVALID`，
        // 具体原因放 `detail`（`M2-GAP-05`，待项目方决定是否补正式码）。
        return invalid_request("INVITE_CODE_UNKNOWN");
    };
    match room.reserve_for(owner).await {
        Ok(seat) => Json(serde_json::json!({
            "matchId": room.match_id(),
            "seat": seat,
            "accept": true,
            "availableSeats": [],
        }))
        .into_response(),
        Err(JoinError::SeatTaken { available }) => Json(serde_json::json!({
            "matchId": room.match_id(),
            "seat": 0,
            "accept": false,
            "availableSeats": available,
            "reason": "RS_SEAT_TAKEN",
        }))
        .into_response(),
    }
}

/// `POST /api/match/queue`（§5.15）：入队，满四人即撮合成房。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MatchQueueBody {
    #[serde(default)]
    game_id: Option<String>,
    mode: String,
}

async fn match_queue(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<MatchQueueBody>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    if let Err(resp) = app.authenticate(&headers) {
        return *resp;
    }
    let game_id = body.game_id.unwrap_or_else(|| GAME_ID.to_owned());
    if game_id != GAME_ID {
        return invalid_request("GAME_UNKNOWN");
    }
    let Some(mode) = Mode::parse(&body.mode) else {
        return invalid_request("MODE_UNKNOWN");
    };
    let ticket = app.enqueue(mode).await;
    // 每次入队顺手撮一次：票数够就直接成房，客户端下一次轮询即见 matched。
    app.pump_match(mode).await;
    Json(serde_json::json!({"ticketId": ticket})).into_response()
}

async fn match_status(
    State(app): State<Arc<App>>,
    Path(ticket_id): Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    match app.ticket_state(&ticket_id).await {
        Some((state, match_id)) => {
            let mut body = serde_json::json!({"state": state.as_str()});
            if let Some(match_id) = match_id {
                body["matchId"] = serde_json::Value::String(match_id);
            }
            Json(body).into_response()
        }
        None => invalid_request("TICKET_UNKNOWN"),
    }
}

async fn match_cancel(
    State(app): State<Arc<App>>,
    Path(ticket_id): Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    Json(serde_json::json!({"ok": app.cancel_ticket(&ticket_id).await})).into_response()
}

/// `POST /api/auth/sms`（§5.15）：发验证码。
#[derive(Deserialize)]
struct SmsBody {
    phone: String,
}

async fn auth_sms(
    State(app): State<Arc<App>>,
    Json(body): Json<SmsBody>,
) -> axum::response::Response {
    use auth::SmsError;
    use axum::response::IntoResponse as _;
    match app.auth.request_sms(&body.phone, now_unix_ms()) {
        Ok(challenge) => Json(serde_json::json!({
            "requestId": challenge.request_id,
            "cooldownSec": challenge.cooldown_sec,
        }))
        .into_response(),
        Err(SmsError::InvalidPhone) => invalid_request("PHONE_INVALID"),
        Err(SmsError::RateLimited { retry_after_ms }) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "reason": "RATE_LIMITED",
                "retryAfterMs": retry_after_ms,
            })),
        )
            .into_response(),
    }
}

/// `POST /api/auth/login`（§5.15）：校码登录，发 access + refresh。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginBody {
    phone: String,
    code: String,
    #[serde(default)]
    client_version: String,
}

async fn auth_login(
    State(app): State<Arc<App>>,
    Json(body): Json<LoginBody>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    if !app.cfg.accepts_version(&body.client_version) {
        return (
            StatusCode::UPGRADE_REQUIRED,
            Json(serde_json::json!({"reason": "CLIENT_TOO_OLD"})),
        )
            .into_response();
    }
    match app.auth.login(&body.phone, &body.code, now_unix_ms()) {
        Ok(session) => {
            // 账号建档（`player_profile`）：存储未接时是 no-op。
            if let Some(profile) = app.auth.profile(&session.player_id) {
                if let Some(store) = app.store() {
                    store.upsert_profile(&profile).await;
                }
            }
            Json(serde_json::json!({
                "token": session.token,
                "refreshToken": session.refresh_token,
                "expiresIn": session.expires_in,
                "playerId": session.player_id,
            }))
            .into_response()
        }
        Err(_) => unauthorized(),
    }
}

/// `POST /api/auth/refresh`（§5.15）：换新 access，旧 refresh 立刻作废。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefreshBody {
    refresh_token: String,
}

async fn auth_refresh(
    State(app): State<Arc<App>>,
    Json(body): Json<RefreshBody>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    match app.auth.refresh(&body.refresh_token, now_unix_ms()) {
        Ok(session) => Json(serde_json::json!({
            "token": session.token,
            "refreshToken": session.refresh_token,
            "expiresIn": session.expires_in,
        }))
        .into_response(),
        Err(_) => unauthorized(),
    }
}

/// `GET /api/profile`（§5.15）：个人资料。
///
/// `stats` 在 PG 切片接入 `player_stat` 前恒为空表——字段位先按契约留好。
async fn profile(State(app): State<Arc<App>>, headers: HeaderMap) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    let player_id = match app.authenticate(&headers) {
        Ok(Some(player_id)) => player_id,
        Ok(None) => return unauthorized(),
        Err(resp) => return *resp,
    };
    let Some(profile) = app.auth.profile(&player_id) else {
        return unauthorized();
    };
    Json(serde_json::json!({
        "playerId": profile.player_id,
        "nickname": profile.nickname,
        "phoneTail4": profile.phone_tail4,
        "stats": [],
    }))
    .into_response()
}

/// 入参不合规（§5.7 没有专门的「参数错误」码，统一复用 `RS_SCHEMA_INVALID`，
/// 细分原因写 `detail`）。
fn invalid_request(detail: &str) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"reason": "RS_SCHEMA_INVALID", "detail": detail})),
    )
        .into_response()
}

#[derive(Deserialize)]
struct WsQuery {
    token: Option<String>,
}

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(app): State<Arc<App>>,
    Query(query): Query<WsQuery>,
) -> axum::response::Response {
    let owner = match query
        .token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        Some(token) => match app.auth.verify_access(token, now_unix_ms()) {
            Some(player_id) => Some(player_id),
            None => return unauthorized(),
        },
        None if app.cfg.require_auth => return unauthorized(),
        None => None,
    };
    ws.on_upgrade(move |socket| session(app, socket, owner))
}

/// 单条 WS 连接的生命周期：入座 → 收动作/重连 → 断开记账。
async fn session(app: Arc<App>, socket: WebSocket, owner: Option<String>) {
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
                let Ok(join) = pb::JoinRoom::decode(body) else {
                    break;
                };
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
                match room.join_as(hint, tx.clone(), owner.clone()).await {
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
                let Ok(mut submit) = pb::SubmitAction::decode(body) else {
                    break;
                };
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
                let Ok(re) = pb::Reconnect::decode(body) else {
                    break;
                };
                let room = app.room_or_create(&re.match_id).await;
                let Ok(seat) = u8::try_from(re.seat) else {
                    break;
                };
                // 带 token 的重连只允许回到本人占过的座位（§5.9 AUTH_INVALID 的座位口径）。
                if let Some(owner) = &owner {
                    if room.seat_owner(seat).await.as_deref() != Some(owner.as_str()) {
                        break;
                    }
                }
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
