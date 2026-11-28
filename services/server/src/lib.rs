//! 掼蛋联机服务端（M2）。
//!
//! 已交付的切片：
//! - 联机骨架：HTTP 建房、WS protobuf、满四人发牌、动作判定、结算广播；
//! - 增量重连：`ReconnectBuffer` 内补发 `RoomDelta`，跨局/结算落快照重置；
//! - 大厅：房间码、凭码预占座位、匹配队列（内存态）；
//! - 账号：手机号 + 短信验证码注册/登录、JWT 认证、refresh 轮换、登出撤销；
//! - 账号持久化：`player_profile` / `player_refresh_token` / `access_revocation`，重启恢复；
//! - PG/Redis 落库：战绩、账号、快照、动作流、队列镜像；
//! - 回放包导出：`GET /api/replays/{matchId}`（§7.7），离线复现用 `tools/replay-viewer`。
//!
//! 未做：结算重试与死信补偿、回放式观战 UI、桌面壳跨平台产物。

pub mod admin;
pub mod auth;
pub mod config;
pub mod events;
pub mod host;
pub mod lobby;
pub mod replay;
pub mod sms;
pub mod store;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use gf_protocol::gf::protocol::v1 as pb;
use gf_protocol::wire;
use prost::Message as _;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex, RwLock};

pub use auth::{Auth, Profile, Session};
pub use config::{DatabaseConfig, RedisConfig, ServerConfig, SmsConfig};
pub use host::{JoinError, ReplayExportError, RoomHost, RoomOptions};
pub use lobby::{CodeLookup, Lobby, Mode, TicketState};
pub use store::{RoundRow, StorageEvent, Store};

/// 本服务只承载掼蛋一个玩法（`GET /api/games` 的 `gameId` 取值）。
pub const GAME_ID: &str = "guandan";

/// WS 单连接下行队列容量：慢客户端积压到上限即断开，防止 OOM。
const WS_SEND_QUEUE: usize = 256;
/// WS 单帧上限（一帧 protobuf 远小于此）。
const WS_MAX_MESSAGE: usize = 64 * 1024;
/// WS 单连接每秒最多处理的消息数（含心跳）。
const WS_MSG_PER_SEC: u32 = 30;
/// HTTP JSON 请求体上限：这些接口只收手机号、验证码、房间码等短字段，
/// 256 KiB 足够覆盖未来加字段；不沿用 axum 默认的 2 MiB，避免匿名请求
/// 用大 JSON 抬高内存/解析成本。（WS 帧另有 [`WS_MAX_MESSAGE`]。）
const MAX_HTTP_BODY: usize = 256 * 1024;

/// 进程级共享状态。
pub struct App {
    cfg: ServerConfig,
    rooms: RwLock<HashMap<String, Arc<RoomHost>>>,
    /// 房间码与匹配队列（内存态；Redis 里另存一份镜像，存储见 `store`）。
    lobby: Mutex<Lobby>,
    /// 账号、验证码与令牌（内存态；PG 落库走 `store`）。
    auth: Auth,
    /// 运营后台配置与审计（内存热缓存；PG 落库走 `store`）。
    admin: admin::AdminState,
    /// 持久化句柄：`App::new` 时不接，`attach_store` 时装一次（进程内只装一次）。
    store: std::sync::OnceLock<Store>,
    /// 房间 → 存档任务的通道；未挂存储时事件被丢弃。
    storage_tx: mpsc::UnboundedSender<StorageEvent>,
    storage_rx: Mutex<Option<mpsc::UnboundedReceiver<StorageEvent>>>,
    room_seq: AtomicU64,
    /// 匹配入队频控：key = 玩家 ID 或来源 IP，value = 上次入队 ms。
    queue_guard: Mutex<HashMap<String, u64>>,
    started_at: Instant,
}

/// 建房拒绝原因（对外只暴露短原因，细节进日志）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomCreateError {
    /// 场次号不合规（长度/字符集），也挡住把 `:` 之类拼进 Redis 键的玩法。
    InvalidMatchId,
    /// 进程内房间数已达 `max_rooms`。
    TooManyRooms,
    /// 运营后台已关闭该玩法（新局/新房间不放行，已有房间仍可重连收尾）。
    GameDisabled,
    /// 运营后台关闭了该模式（休闲/排位/混合）。
    ModeDisabled,
    /// 规则集建桌失败（正常不会发生）。
    Engine(String),
}

/// 场次号白名单：1..=48 个 `[A-Za-z0-9_-]`，且必须以字母/数字开头。
pub fn valid_match_id(match_id: &str) -> bool {
    let bytes = match_id.as_bytes();
    if bytes.is_empty() || bytes.len() > 48 {
        return false;
    }
    if !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    bytes
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-' || *byte == b'_')
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
            admin: admin::AdminState::default(),
            store: std::sync::OnceLock::new(),
            storage_tx,
            storage_rx: Mutex::new(Some(storage_rx)),
            room_seq: AtomicU64::new(1),
            queue_guard: Mutex::new(HashMap::new()),
            started_at: Instant::now(),
        })
    }

    /// 接上 PG/Redis（`main` 在建 `App` 之后调用；单测不调用即为纯内存态）。
    pub async fn attach_store(self: &Arc<Self>, store: Store) {
        if !store.enabled() {
            return;
        }
        let has_pg = store.pg_enabled();
        if self.store.set(store).is_err() {
            eprintln!("[store] 已接过存储句柄，忽略重复接入");
            return;
        }
        // 账号状态先恢复：进程重启后同一手机号必须拿到原 `playerId`，
        // 已登出的 access 与已轮换的 refresh 也必须继续失效。
        if has_pg {
            self.restore_auth().await;
            self.restore_admin().await;
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

    /// 启动时把 PG 里的账号态灌回 `Auth` 热缓存。
    ///
    /// 失败只记日志不阻断启动：账号接口在数据库故障时按「不可用」返回，
    /// 而不是让整个服务起不来。
    async fn restore_auth(&self) {
        let Some(store) = self.store() else { return };
        let now = now_unix_ms();
        match store.load_profiles().await {
            Ok(profiles) => {
                let count = profiles.len();
                self.auth.restore_profiles(profiles);
                eprintln!("[auth] 已恢复 {count} 个账号档案");
            }
            Err(err) => eprintln!("[auth] 恢复账号档案失败，登录/注册将返回 503：{err}"),
        }
        match store.load_refresh_tokens(now).await {
            Ok(rows) => {
                let count = rows.len();
                self.auth.restore_refresh_tokens(rows, now);
                eprintln!("[auth] 已恢复 {count} 枚未过期 refresh token");
            }
            Err(err) => eprintln!("[auth] 恢复 refresh token 失败：{err}"),
        }
        match store.load_access_revocations(now).await {
            Ok(rows) => {
                let count = rows.len();
                self.auth.restore_access_revocations(rows, now);
                eprintln!("[auth] 已恢复 {count} 条 access 撤销记录");
            }
            Err(err) => eprintln!("[auth] 恢复 access 撤销记录失败：{err}"),
        }
    }

    /// 启动时恢复运营配置与审计。失败只记日志，回落默认配置。
    async fn restore_admin(&self) {
        let Some(store) = self.store() else { return };
        match store.load_game_setting().await {
            Ok(Some(setting)) => {
                eprintln!(
                    "[admin] 已恢复玩法配置：enabled={} modes={:?} nextDealMs={}",
                    setting.enabled, setting.modes, setting.next_deal_ms
                );
                self.admin.replace_setting(setting).await;
            }
            Ok(None) => eprintln!("[admin] 库里没有玩法配置，使用默认值"),
            Err(err) => eprintln!("[admin] 恢复玩法配置失败，使用默认值：{err}"),
        }
        match store.load_admin_audits(200).await {
            Ok(rows) => {
                let count = rows.len();
                self.admin.restore_audits(rows).await;
                eprintln!("[admin] 已恢复 {count} 条审计记录");
            }
            Err(err) => eprintln!("[admin] 恢复审计失败：{err}"),
        }
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
    pub async fn room_or_create(&self, match_id: &str) -> Result<Arc<RoomHost>, RoomCreateError> {
        if !valid_match_id(match_id) {
            return Err(RoomCreateError::InvalidMatchId);
        }
        if let Some(room) = self.rooms.read().await.get(match_id).cloned() {
            return Ok(room);
        }
        if !self.admin.game_enabled().await {
            return Err(RoomCreateError::GameDisabled);
        }
        let next_deal_ms = self.admin.next_deal_ms().await;
        let mut rooms = self.rooms.write().await;
        if let Some(room) = rooms.get(match_id).cloned() {
            return Ok(room);
        }
        if rooms.len() >= self.cfg.max_rooms {
            return Err(RoomCreateError::TooManyRooms);
        }
        let seed = self.room_seq.fetch_add(1, Ordering::Relaxed);
        let opts = RoomOptions {
            tick_ms: self.cfg.tick_ms,
            next_deal_ms,
        };
        let room = RoomHost::spawn(match_id.to_owned(), seed, opts)
            .map_err(|err| RoomCreateError::Engine(err.to_string()))?;
        if self.store().is_some() {
            room.attach_recorder(self.storage_tx.clone());
        }
        rooms.insert(match_id.to_owned(), room.clone());
        Ok(room)
    }

    /// 只取已有房间（回放查询等不该顺手建房的地方用）。
    pub async fn room(&self, match_id: &str) -> Option<Arc<RoomHost>> {
        self.rooms.read().await.get(match_id).cloned()
    }

    /// 所有在册房间句柄（运营后台巡检只读；顺序由调用方决定）。
    pub async fn room_handles(&self) -> Vec<Arc<RoomHost>> {
        self.rooms.read().await.values().cloned().collect()
    }

    /// 回收已结束/空置房间，返回清掉的个数。每 60 秒由后台任务调一次。
    pub async fn reap_rooms(&self) -> usize {
        let retention = std::time::Duration::from_millis(self.cfg.room_retention_ms);
        let idle = std::time::Duration::from_millis(self.cfg.room_idle_ms);
        let candidates: Vec<(String, Arc<RoomHost>)> = self
            .rooms
            .read()
            .await
            .iter()
            .map(|(id, room)| (id.clone(), room.clone()))
            .collect();
        let mut dead = Vec::new();
        for (id, room) in candidates {
            if room.reapable(retention, idle).await {
                dead.push(id);
            }
        }
        if dead.is_empty() {
            return 0;
        }
        let mut rooms = self.rooms.write().await;
        for id in &dead {
            rooms.remove(id);
        }
        println!(
            "[rooms] 回收 {} 张房间（上限 {}）",
            dead.len(),
            self.cfg.max_rooms
        );
        dead.len()
    }

    /// 建房（返回可用房间号）。房间号由调用方给定，便于房间码分享。
    pub async fn create_room(&self, match_id: &str) -> Result<Arc<RoomHost>, RoomCreateError> {
        self.room_or_create(match_id).await
    }

    /// `POST /api/room/create` 的实现：生成场次号 → 建房 → 出房间码。
    pub async fn create_room_with_code(
        &self,
        mode: Mode,
    ) -> Result<(Arc<RoomHost>, String), RoomCreateError> {
        if !self.admin.mode_enabled(mode).await {
            return Err(RoomCreateError::ModeDisabled);
        }
        let match_id = loop {
            let id = self.lobby.lock().await.new_match_id();
            if !self.rooms.read().await.contains_key(&id) {
                break id;
            }
        };
        let room = self.room_or_create(&match_id).await?;
        room.set_mode(mode.as_str()).await;
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
        Ok((room, code))
    }

    /// 凭房间码找房间；失效码返回 `None`。
    pub async fn room_by_code(&self, code: &str) -> Option<Arc<RoomHost>> {
        self.room_by_code_result(code).await.ok()
    }

    /// 凭房间码找房间，保留「未知 / 过期」区分（HTTP 需要分别回错误码）。
    pub async fn room_by_code_result(&self, code: &str) -> Result<Arc<RoomHost>, CodeLookup> {
        match self.lobby.lock().await.lookup_code(code, self.now_ms()) {
            CodeLookup::Active(match_id) => self
                .room_or_create(&match_id)
                .await
                .map_err(|_| CodeLookup::Unknown),
            CodeLookup::Expired => Err(CodeLookup::Expired),
            CodeLookup::Unknown => Err(CodeLookup::Unknown),
        }
    }

    /// 入匹配队列，返回票号。
    /// 匹配入队频控：同一玩家/IP 2 秒内只允许一次；票总量超过 4096 直接拒绝。
    pub async fn admit_match_queue(&self, key: &str) -> bool {
        let now = now_unix_ms();
        let mut guard = self.queue_guard.lock().await;
        guard.retain(|_, last| now.saturating_sub(*last) < 60_000);
        if guard
            .get(key)
            .is_some_and(|last| now.saturating_sub(*last) < 2_000)
        {
            return false;
        }
        if self.lobby.lock().await.total_waiting() >= 4_096 {
            return false;
        }
        guard.insert(key.to_owned(), now);
        true
    }

    pub async fn enqueue(&self, mode: Mode, owner: Option<&str>) -> String {
        let ticket = self.lobby.lock().await.enqueue_for(
            GAME_ID,
            mode,
            self.now_ms(),
            owner.map(|s| s.to_owned()),
        );
        if let Some(store) = self.store() {
            store
                .queue_upsert(GAME_ID, mode.as_str(), &ticket, now_unix_ms())
                .await;
        }
        ticket
    }

    /// 轮询票状态（票号归属不符按不存在处理）。
    pub async fn ticket_state(
        &self,
        ticket_id: &str,
        owner: Option<&str>,
    ) -> Option<(TicketState, Option<String>)> {
        let now = self.now_ms();
        self.lobby
            .lock()
            .await
            .poll_for(ticket_id, owner, now, self.cfg.match_timeout_ms)
    }

    /// 退队；返回是否真的退掉（已配对/不存在/归属不符都返回 false）。
    pub async fn cancel_ticket(&self, ticket_id: &str, owner: Option<&str>) -> bool {
        let now = self.now_ms();
        let (removed, mode) = self
            .lobby
            .lock()
            .await
            .cancel_with_mode_for(ticket_id, owner, now);
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
        let room = match self.room_or_create(&match_id).await {
            Ok(room) => room,
            Err(_) => {
                // 房间数触顶：本批票先不放行，避免客户端拿着票找不到房。
                return None;
            }
        };
        room.set_mode(mode.as_str()).await;
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
            .route("/api/auth/register", post(auth_register))
            .route("/api/auth/login", post(auth_login))
            .route("/api/auth/logout", post(auth_logout))
            .route("/api/auth/refresh", post(auth_refresh))
            .route("/api/profile", get(profile))
            .route("/api/games", get(games))
            .route("/api/replays/{match_id}", get(replay_package))
            .route("/ws", get(ws_upgrade))
            .merge(admin::router())
            // 联机大厅是独立静态页（apps/web/online.html），本机/内网分开部署时需要跨源。
            // 鉴权走 Bearer 头而非 Cookie，permissive 不会带上凭据；生产可再收窄 origin。
            .layer(tower_http::cors::CorsLayer::permissive())
            .layer(DefaultBodyLimit::max(MAX_HTTP_BODY))
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

    /// 登出：撤销当前 access token（签名/过期校验通过才收）。
    pub fn revoke_access_token(&self, token: &str) -> bool {
        self.auth.revoke_access(token, now_unix_ms())
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
    // 房间回收：已结束的留 10 分钟给回放导出，空房留 10 分钟；上限另有 max_rooms 兜底。
    let reaper = app.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            reaper.reap_rooms().await;
        }
    });
    let server = axum::serve(
        listener,
        app.router()
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal());
    // 只接受停止信号，不再接新连接；给在途帧/落库 15s 收尾，超时再强制退出。
    match tokio::time::timeout(std::time::Duration::from_secs(15), server).await {
        Ok(result) => result?,
        Err(_) => eprintln!("[shutdown] 15s 内仍有长连接未收尾，强制退出"),
    }
    app.flush_snapshots().await;
    Ok(())
}

/// Ctrl-C / SIGTERM 优雅退出信号。
async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
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
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    let match_id = body.match_id.trim();
    match app.create_room(match_id).await {
        Ok(room) => (
            StatusCode::OK,
            Json(serde_json::json!({"match_id": room.match_id(), "seats": host::SEATS})),
        )
            .into_response(),
        Err(RoomCreateError::InvalidMatchId) => invalid_request("MATCH_ID_INVALID"),
        Err(RoomCreateError::GameDisabled) => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"reason": "GAME_DISABLED"})),
        )
            .into_response(),
        Err(RoomCreateError::ModeDisabled) => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"reason": "GAME_MODE_DISABLED"})),
        )
            .into_response(),
        Err(RoomCreateError::TooManyRooms) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({"reason": "RATE_LIMITED"})),
        )
            .into_response(),
        Err(err) => {
            eprintln!("[lobby] 建房失败：{err:?}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"reason": "INTERNAL"})),
            )
                .into_response()
        }
    }
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
    if !app.admin.mode_enabled(mode).await {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"reason": "GAME_MODE_DISABLED"})),
        )
            .into_response();
    }
    // 座位数由 RuleSet 的 `seats.count` 决定，客户端只能传同值（§5.15）。
    if body
        .seats_count
        .is_some_and(|n| n != u32::from(host::SEATS))
    {
        return invalid_request("SEATS_MISMATCH");
    }
    let (room, code) = match app.create_room_with_code(mode).await {
        Ok(pair) => pair,
        Err(RoomCreateError::GameDisabled) | Err(RoomCreateError::ModeDisabled) => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"reason": "GAME_MODE_DISABLED"})),
            )
                .into_response()
        }
        Err(RoomCreateError::TooManyRooms) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({"reason": "RATE_LIMITED"})),
            )
                .into_response()
        }
        Err(err) => {
            eprintln!("[lobby] 建房失败：{err:?}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"reason": "INTERNAL"})),
            )
                .into_response();
        }
    };
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
    let room = match app.room_by_code_result(&body.invite_code).await {
        Ok(room) => room,
        // §5.7：房间码未知 / 已过期分开回，不再包 `RS_SCHEMA_INVALID` 的 detail。
        Err(CodeLookup::Expired) => return not_found("INVITE_CODE_EXPIRED"),
        Err(CodeLookup::Unknown) => return not_found("INVITE_CODE_UNKNOWN"),
        Err(CodeLookup::Active(_)) => unreachable!("Active 分支已返回房间"),
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
        // reserve_for 本身不会产生 AuthInvalid，这里只为匹配闭集。
        Err(JoinError::AuthInvalid) => unauthorized(),
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
    peer: PeerAddr,
    headers: HeaderMap,
    Json(body): Json<MatchQueueBody>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    let owner = match app.authenticate(&headers) {
        Ok(owner) => owner,
        Err(resp) => return *resp,
    };
    let game_id = body.game_id.unwrap_or_else(|| GAME_ID.to_owned());
    if game_id != GAME_ID {
        return invalid_request("GAME_UNKNOWN");
    }
    let Some(mode) = Mode::parse(&body.mode) else {
        return invalid_request("MODE_UNKNOWN");
    };
    if !app.admin.mode_enabled(mode).await {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"reason": "GAME_MODE_DISABLED"})),
        )
            .into_response();
    }
    // 入队频控：优先按玩家 ID，其次按来源 IP；anonymous 且无 peer（单测）不额外限。
    if let Some(key) = owner
        .as_ref()
        .map(|id| format!("player:{id}"))
        .or_else(|| peer.0.map(|addr| format!("ip:{addr}")))
    {
        if !app.admit_match_queue(&key).await {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({"reason": "RATE_LIMITED"})),
            )
                .into_response();
        }
    }
    let ticket = app.enqueue(mode, owner.as_deref()).await;
    // 每次入队顺手撮一次：票数够就直接成房，客户端下一次轮询即见 matched。
    app.pump_match(mode).await;
    Json(serde_json::json!({"ticketId": ticket})).into_response()
}

async fn match_status(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(ticket_id): Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    let owner = match app.authenticate(&headers) {
        Ok(owner) => owner,
        Err(resp) => return *resp,
    };
    match app.ticket_state(&ticket_id, owner.as_deref()).await {
        Some((state, match_id)) => {
            let mut body = serde_json::json!({"state": state.as_str()});
            if let Some(match_id) = match_id {
                body["matchId"] = serde_json::Value::String(match_id);
            }
            Json(body).into_response()
        }
        None => not_found("TICKET_UNKNOWN"),
    }
}

async fn match_cancel(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(ticket_id): Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    let owner = match app.authenticate(&headers) {
        Ok(owner) => owner,
        Err(resp) => return *resp,
    };
    Json(serde_json::json!({"ok": app.cancel_ticket(&ticket_id, owner.as_deref()).await}))
        .into_response()
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

/// 可选的 TCP 对端地址：`oneshot` 测试没挂 ConnectInfo 时返回 None，生产由
/// `into_make_service_with_connect_info` 注入。
struct PeerAddr(Option<SocketAddr>);

impl<S> axum::extract::FromRequestParts<S> for PeerAddr
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(PeerAddr(
            parts
                .extensions
                .get::<axum::extract::ConnectInfo<SocketAddr>>()
                .map(|info| info.0),
        ))
    }
}

/// `POST /api/auth/register`：显式注册（手机号 + 短信验证码），直接返回会话。
///
/// 登录仍保留「首次校码自动建档」；这个接口是给需要明确「注册成功/已存在」
/// 语义的客户端用的加法接口，底层账号主数据同一张 `player_profile`。
async fn auth_register(
    State(app): State<Arc<App>>,
    peer: PeerAddr,
    Json(body): Json<LoginBody>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    if !app.cfg.accepts_version(&body.client_version) {
        return client_too_old();
    }
    let now = now_unix_ms();
    let verified = match app.auth.verify_login_code(
        &body.phone,
        &body.code,
        peer.0.as_ref().map(|a| a.to_string()).as_deref(),
        now,
    ) {
        Ok(verified) => verified,
        Err(err) => return login_error_response(err),
    };
    let profile = match app.store().filter(|store| store.pg_enabled()) {
        Some(store) => {
            let new_player_id = app.auth.next_player_id();
            match store
                .create_profile(
                    &verified.phone_hash,
                    &verified.phone_tail4,
                    now,
                    &new_player_id,
                )
                .await
            {
                Ok(Some(profile)) => profile,
                Ok(None) => {
                    return (
                        StatusCode::CONFLICT,
                        Json(serde_json::json!({"reason": "ACCOUNT_EXISTS"})),
                    )
                        .into_response();
                }
                Err(err) => {
                    eprintln!("[auth] 注册建档失败：{err}");
                    return storage_unavailable();
                }
            }
        }
        // 纯内存路径：没有 PG 的本地联调/单测也要能显式注册。
        None => {
            if app.auth.phone_hash_known(&body.phone) {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({"reason": "ACCOUNT_EXISTS"})),
                )
                    .into_response();
            }
            app.auth
                .upsert_profile(&verified.phone_hash, &verified.phone_tail4, now)
        }
    };
    if profile.status != "active" {
        return account_disabled();
    }
    app.auth.remember_profile(profile.clone());
    issue_persisted_session(&app, &profile, now).await
}

async fn auth_login(
    State(app): State<Arc<App>>,
    peer: PeerAddr,
    Json(body): Json<LoginBody>,
) -> axum::response::Response {
    if !app.cfg.accepts_version(&body.client_version) {
        return client_too_old();
    }
    let now = now_unix_ms();
    let peer = peer.0.map(|addr| addr.to_string());
    let verified = match app
        .auth
        .verify_login_code(&body.phone, &body.code, peer.as_deref(), now)
    {
        Ok(verified) => verified,
        Err(err) => return login_error_response(err),
    };

    let profile = match app.store().filter(|store| store.pg_enabled()) {
        Some(store) => {
            let new_player_id = app.auth.next_player_id();
            match store
                .find_or_create_profile(
                    &verified.phone_hash,
                    &verified.phone_tail4,
                    now,
                    &new_player_id,
                )
                .await
            {
                Ok(profile) => {
                    app.auth.remember_profile(profile.clone());
                    profile
                }
                Err(err) => {
                    eprintln!("[auth] 登录读取/建档失败：{err}");
                    return storage_unavailable();
                }
            }
        }
        // 纯内存路径（单测/未接 PG）：与旧行为完全一致。
        None => app
            .auth
            .upsert_profile(&verified.phone_hash, &verified.phone_tail4, now),
    };
    if profile.status != "active" {
        return account_disabled();
    }
    issue_persisted_session(&app, &profile, now).await
}

/// 发会话并落 refresh；PG 写失败时把刚发的 token 全部收回，避免内存里有
/// 一枚服务端不认的 refresh。
async fn issue_persisted_session(
    app: &Arc<App>,
    profile: &Profile,
    now_ms: u64,
) -> axum::response::Response {
    let session = app.auth.issue_session(&profile.player_id, now_ms);
    if let Some(store) = app.store().filter(|store| store.pg_enabled()) {
        let key = Auth::refresh_key(&session.refresh_token);
        let expires_at = Auth::refresh_expires_at(now_ms);
        if let Err(err) = store
            .insert_refresh_token(&key, &profile.player_id, expires_at, now_ms)
            .await
        {
            eprintln!("[auth] 写 refresh token 失败：{err}");
            app.auth.revoke_refresh(&session.refresh_token);
            app.auth.revoke_access(&session.token, now_ms);
            return storage_unavailable();
        }
    }
    session_response(&session)
}

fn session_response(session: &Session) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    Json(serde_json::json!({
        "token": session.token,
        "refreshToken": session.refresh_token,
        "expiresIn": session.expires_in,
        "playerId": session.player_id,
    }))
    .into_response()
}

/// `POST /api/auth/logout`：撤销 access；body 里带 `refreshToken` 时一并撤销。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LogoutBody {
    refresh_token: String,
}

async fn auth_logout(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    let now = now_unix_ms();
    let token = bearer_token(&headers);
    let mut ok = false;
    let mut storage_failed = false;

    if let Some(token) = token {
        if app.auth.revoke_access(token, now) {
            ok = true;
            if let Some(store) = app.store().filter(|store| store.pg_enabled()) {
                // 只撤内存会在重启后「复活」，撤销表必须落库。
                if let Err(err) = store
                    .insert_access_revocation(
                        &Auth::access_key(token),
                        Auth::access_expires_at(now),
                    )
                    .await
                {
                    storage_failed = true;
                    eprintln!("[auth] 写 access 撤销记录失败：{err}");
                }
            }
        }
    }

    let refresh_token = serde_json::from_slice::<LogoutBody>(&body)
        .ok()
        .map(|body| body.refresh_token)
        .filter(|token| !token.trim().is_empty());
    if let Some(token) = refresh_token {
        if let Some(key) = app.auth.revoke_refresh(&token) {
            ok = true;
            if let Some(store) = app.store().filter(|store| store.pg_enabled()) {
                if let Err(err) = store.delete_refresh_token(&key).await {
                    storage_failed = true;
                    eprintln!("[auth] 删除 refresh token 失败：{err}");
                }
            }
        }
    }

    if storage_failed {
        return storage_unavailable();
    }
    Json(serde_json::json!({"ok": ok})).into_response()
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
    let now = now_unix_ms();
    let (session, rollback) = match app.auth.rotate_refresh(&body.refresh_token, now) {
        Ok(pair) => pair,
        Err(err) => return login_error_response(err),
    };
    if let Some(store) = app.store().filter(|store| store.pg_enabled()) {
        let new_key = Auth::refresh_key(&session.refresh_token);
        let expires_at = Auth::refresh_expires_at(now);
        if let Err(err) = store
            .rotate_refresh_token(&rollback.key, &new_key, &session.player_id, expires_at, now)
            .await
        {
            app.auth.revoke_refresh(&session.refresh_token);
            app.auth.revoke_access(&session.token, now);
            if matches!(err, sqlx::Error::RowNotFound) {
                // 权威表里已经没有了：过期、被另一端轮换或已登出。
                // 旧 refresh 也不能放回内存，否则会一直「内存有效、库里没有」。
                return unauthorized();
            }
            // 存储抖动才回滚：旧 refresh 放回，客户端可重试同一枚。
            app.auth.restore_refresh(rollback);
            eprintln!("[auth] 轮换 refresh token 失败：{err}");
            return storage_unavailable();
        }
    }
    Json(serde_json::json!({
        "token": session.token,
        "refreshToken": session.refresh_token,
        "expiresIn": session.expires_in,
    }))
    .into_response()
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
                .map(str::trim)
        })
        .filter(|t| !t.is_empty())
}

fn login_error_response(err: auth::LoginError) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    match err {
        auth::LoginError::Locked { retry_after_ms } => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "reason": "RATE_LIMITED",
                "retryAfterMs": retry_after_ms,
            })),
        )
            .into_response(),
        auth::LoginError::AccountDisabled => account_disabled(),
        _ => unauthorized(),
    }
}

fn client_too_old() -> axum::response::Response {
    use axum::response::IntoResponse as _;
    (
        StatusCode::UPGRADE_REQUIRED,
        Json(serde_json::json!({"reason": "CLIENT_TOO_OLD"})),
    )
        .into_response()
}

fn account_disabled() -> axum::response::Response {
    use axum::response::IntoResponse as _;
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({"reason": "ACCOUNT_DISABLED"})),
    )
        .into_response()
}

fn storage_unavailable() -> axum::response::Response {
    use axum::response::IntoResponse as _;
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({"reason": "STORAGE_UNAVAILABLE"})),
    )
        .into_response()
}

/// `GET /api/profile`（§5.15）：个人资料。
///
/// `stats` 在 PG 切片接入 `player_stat` 前恒为空表——字段位先按契约留好。
/// 大厅玩法列表：版本不达标也返回 200 与完整列表，附带升级提示；
/// 真正拦对局的是 WS `JoinRoom.clientVersion` 的 `CLIENT_TOO_OLD`。
#[derive(Debug, Deserialize)]
struct GamesQuery {
    #[serde(alias = "clientVersion")]
    client_version: Option<String>,
}

async fn games(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<GamesQuery>,
) -> axum::response::Response {
    let client_version = query
        .client_version
        .or_else(|| {
            headers
                .get("x-client-version")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        })
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let compatible = match client_version.as_deref() {
        Some(version) => app.cfg.accepts_version(version),
        None => app.cfg.min_client_version.trim().is_empty(),
    };
    let setting = app.admin.game_setting().await;
    let modes = [
        ("casual", "休闲场"),
        ("ranked", "排位场"),
        ("mixed", "混合场"),
    ]
    .into_iter()
    .map(|(mode, label)| {
        json!({
            "mode": mode,
            "label": label,
            "enabled": setting.enabled && setting.modes.iter().any(|m| m == mode),
        })
    })
    .collect::<Vec<_>>();
    let upgrade = if compatible {
        Value::Null
    } else {
        json!({
            "required": true,
            "reason": "CLIENT_TOO_OLD",
            "minimumVersion": app.cfg.min_client_version,
            "message": "当前客户端版本低于服务端最低要求，请升级后再进入对局。",
        })
    };
    Json(json!({
        "ok": true,
        "serverTimeMs": now_unix_ms(),
        "clientVersion": client_version,
        "minClientVersion": app.cfg.min_client_version,
        "compatible": compatible,
        "upgrade": upgrade,
        "items": [{
            "gameId": admin::DEFAULT_GAME_ID,
            "displayName": setting.display_name,
            "enabled": setting.enabled,
            "modes": modes,
            "rulesetVersion": gf_table_runtime::DEFAULT_RULESET_VERSION,
            "seatsCount": host::SEATS,
            "nextDealMs": setting.next_deal_ms,
            "updatedAtMs": setting.updated_at_ms,
        }],
    }))
    .into_response()
}

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

/// `GET /api/replays/{matchId}`（§5.15）：只对已结束房间、且调用者是该桌玩家的请求返回回放包。
///
/// 包内含他人手牌，属敏感数据，所以不认匿名请求；不是本桌玩家一律 403。
async fn replay_package(
    State(app): State<Arc<App>>,
    Path(match_id): Path<String>,
    headers: HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    let player_id = match app.authenticate(&headers) {
        Ok(Some(player_id)) => player_id,
        Ok(None) => return unauthorized(),
        Err(resp) => return *resp,
    };
    let Some(room) = app.room(&match_id).await else {
        return not_found("PLAY_REPLAY_NOT_FOUND");
    };
    if !room.seated(&player_id).await {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"reason": "AUTH_INVALID"})),
        )
            .into_response();
    }
    match room.export_replay().await {
        Ok(pkg) => {
            if !pkg.verified {
                eprintln!("[replay] {match_id} 重放末态与权威不一致，包仍导出待排查");
            }
            let mut response = axum::body::Bytes::from(pkg.bytes).into_response();
            let headers = response.headers_mut();
            headers.insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("application/zip"),
            );
            if let Ok(value) = axum::http::HeaderValue::from_str(&format!(
                "attachment; filename=\"{}.zip\"",
                match_id.replace('"', "")
            )) {
                headers.insert(axum::http::header::CONTENT_DISPOSITION, value);
            }
            response
        }
        Err(ReplayExportError::NotFinished) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"reason": "TOO_LATE", "detail": "REPLAY_NOT_FINISHED"})),
        )
            .into_response(),
        Err(ReplayExportError::Build(err)) => {
            eprintln!("[replay] {match_id} 导出失败：{err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"reason": "REPLAY_FAILED"})),
            )
                .into_response()
        }
    }
}

fn not_found(reason: &str) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"reason": reason})),
    )
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
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
) -> axum::response::Response {
    // 优先 Authorization 头；浏览器不能设自定义头，故同时支持子协议 `gf-token.<jwt>`。
    // `?token=` 只保留给旧客户端过渡，生产应逐步禁用（会进访问日志/Referer）。
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty());
    let subprotocol = headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(',')
                .map(str::trim)
                .find_map(|p| p.strip_prefix("gf-token."))
        })
        .map(str::trim)
        .filter(|t| !t.is_empty());
    let (token, from_query) = match bearer.or(subprotocol) {
        Some(token) => (Some(token.to_owned()), false),
        None => (
            query
                .token
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_owned),
            true,
        ),
    };
    if from_query && token.is_some() && app.cfg.env == "production" {
        // 生产禁止 token 进 URL：会落访问日志/Referer。浏览器改用子协议 `gf-token.<jwt>`。
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"reason": "TOKEN_IN_QUERY_DISABLED"})).into_response(),
        )
            .into_response();
    }
    let owner = match token.as_deref() {
        Some(token) => match app.auth.verify_access(token, now_unix_ms()) {
            Some(player_id) => Some(player_id),
            None => return unauthorized(),
        },
        None if app.cfg.require_auth => return unauthorized(),
        None => None,
    };
    ws.max_message_size(WS_MAX_MESSAGE)
        .max_frame_size(WS_MAX_MESSAGE)
        .protocols(["gf.v1"])
        .on_upgrade(move |socket| session(app, socket, owner))
}

/// 单条 WS 连接的生命周期：入座 → 收动作/重连 → 断开记账。
async fn session(app: Arc<App>, socket: WebSocket, owner: Option<String>) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(WS_SEND_QUEUE);

    let writer = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            if sink.send(Message::Binary(frame.into())).await.is_err() {
                break;
            }
        }
    });

    let mut bound: Option<(Arc<RoomHost>, u8)> = None;
    let idle = std::time::Duration::from_millis(app.cfg.heartbeat_timeout_ms);
    let mut window = Instant::now();
    let mut msg_count = 0u32;

    loop {
        if window.elapsed() >= std::time::Duration::from_secs(1) {
            window = Instant::now();
            msg_count = 0;
        }
        msg_count = msg_count.saturating_add(1);
        if msg_count > WS_MSG_PER_SEC {
            // 单连接消息洪水：直接断开，保护房间主循环。
            break;
        }
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
                    let _ = tx
                        .send(events::join_ack_frame(
                            &join.match_id,
                            0,
                            false,
                            &[],
                            Some("CLIENT_TOO_OLD"),
                            None,
                        ))
                        .await;
                    break;
                }
                let Ok(room) = app.room_or_create(&join.match_id).await else {
                    let _ = tx
                        .send(events::join_ack_frame(
                            &join.match_id,
                            0,
                            false,
                            &[],
                            Some("RATE_LIMITED"),
                            None,
                        ))
                        .await;
                    break;
                };
                let hint = join.seat.and_then(|s| u8::try_from(s).ok());
                match room
                    .join_with_token(
                        hint,
                        tx.clone(),
                        owner.clone(),
                        (!join.seat_token.is_empty()).then_some(join.seat_token.as_str()),
                    )
                    .await
                {
                    Ok(ticket) => {
                        let _ = tx
                            .send(events::join_ack_frame(
                                &join.match_id,
                                ticket.seat,
                                true,
                                &[],
                                None,
                                Some(&ticket.token),
                            ))
                            .await;
                        // 入座回执先发，再按「满四人」开局；新座位随后拿全量快照。
                        room.start_if_full().await;
                        if let Ok(snapshot) = room.join_snapshot(ticket.seat).await {
                            let _ = tx.send(snapshot).await;
                        }
                        if let Some(frame) = room.rematch_state_frame().await {
                            let _ = tx.send(frame).await;
                        }
                        bound = Some((room, ticket.seat));
                    }
                    Err(JoinError::SeatTaken { available }) => {
                        let _ = tx
                            .send(events::join_ack_frame(
                                &join.match_id,
                                0,
                                false,
                                &available,
                                Some("RS_SEAT_TAKEN"),
                                None,
                            ))
                            .await;
                    }
                    Err(JoinError::AuthInvalid) => {
                        let _ = tx
                            .send(events::join_ack_frame(
                                &join.match_id,
                                0,
                                false,
                                &[],
                                Some("AUTH_INVALID"),
                                None,
                            ))
                            .await;
                        break;
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
                        let _ = tx
                            .send(events::action_ack_frame(
                                &submit.request_id,
                                room.seat_watermark(*seat).await,
                                room.room_seq().await,
                                false,
                                Some("AUTH_INVALID"),
                            ))
                            .await;
                        continue;
                    }
                }
                submit.match_id = room.match_id().to_owned();
                let ack = room.submit(*seat, submit, room.now_ms()).await;
                let _ = tx.send(ack).await;
            }
            wire::kind::REMATCH_READY => {
                let Some((room, seat)) = &bound else { break };
                let Ok(ready) = pb::RematchReady::decode(body) else {
                    break;
                };
                if !ready.match_id.is_empty() && ready.match_id != room.match_id() {
                    // 房间号不符不切房间，按非法请求忽略；确认态仍以绑定座位为准。
                    continue;
                }
                let _ = room.mark_rematch_ready(*seat).await;
            }
            wire::kind::RECONNECT => {
                let Ok(re) = pb::Reconnect::decode(body) else {
                    break;
                };
                let Ok(seat) = u8::try_from(re.seat) else {
                    break;
                };
                let Some(room) = app.room(&re.match_id).await else {
                    break;
                };
                // 带 token 的重连只允许回到本人占过的座位（§5.9 AUTH_INVALID 的座位口径）；
                // 匿名座位必须出示 JoinAck 下发的一次性令牌，否则凭座位号就能顶号。
                if let Some(owner) = &owner {
                    if room.seat_owner(seat).await.as_deref() != Some(owner.as_str()) {
                        break;
                    }
                } else if !room.seat_token_matches(seat, &re.seat_token).await {
                    break;
                }
                if !room.rebind(seat, tx.clone()).await {
                    break;
                }
                match room.reconnect(seat, re.last_seat_seq).await {
                    Ok(frames) => {
                        // 增量帧在前、快照在后，必须按顺序下发。
                        for frame in frames {
                            let _ = tx.send(frame).await;
                        }
                        if let Some(frame) = room.rematch_state_frame().await {
                            let _ = tx.send(frame).await;
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

#[cfg(test)]
mod audit_tests {
    use super::*;

    #[test]
    fn match_id_whitelist_blocks_paths_and_junk() {
        assert!(valid_match_id("match-001_A"));
        assert!(valid_match_id("a"));
        assert!(!valid_match_id(""));
        assert!(!valid_match_id("../etc/passwd"));
        assert!(!valid_match_id("-leading"));
        assert!(!valid_match_id("with space"));
        assert!(!valid_match_id(&"x".repeat(49)));
    }

    #[tokio::test]
    async fn room_cap_and_id_validation_are_enforced() {
        let app = App::new(ServerConfig {
            max_rooms: 1,
            tick_ms: 50,
            ..ServerConfig::default()
        });
        assert!(matches!(
            app.room_or_create("../x").await,
            Err(RoomCreateError::InvalidMatchId)
        ));
        assert!(app.room_or_create("m-1").await.is_ok());
        assert!(matches!(
            app.room_or_create("m-2").await,
            Err(RoomCreateError::TooManyRooms)
        ));
        assert!(
            app.room_or_create("m-1").await.is_ok(),
            "已有房间不受上限影响"
        );
    }
}
