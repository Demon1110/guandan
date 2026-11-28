//! 运营后台（M2 最小管理台）。
//!
//! 鉴权用独立的 `GF_ADMIN_TOKEN`（Bearer），与玩家 access token 完全分离：
//! 后台令牌泄露也只影响管理接口，不会伪造玩家身份。令牌为空 = 后台关闭。
//!
//! 本轮交付：
//! - 概览（账号/对局/房间/存储状态）；
//! - 用户列表、按玩家 ID / 昵称 / 尾号检索、启用/禁用；
//! - 玩法配置（开关、模式、结算续局间隔），即时作用到新房间与匹配；
//! - 对局记录只读列表；
//! - 写操作审计（状态变更、配置变更）。
//!
//! 用户/对局数据优先读 PG；没接 PG 时用户列表退回 `Auth` 内存档案，
//! 对局列表为空。审计写 PG 失败只打日志，不阻断已经生效的管理动作。

use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{Mutex, RwLock};

use crate::auth::Profile;
use crate::lobby::Mode;
use crate::{now_unix_ms, App};

/// 玩法配置默认值（掼蛋 v1 只有这一款，game_id 固定）。
pub const DEFAULT_GAME_ID: &str = "guandan";

/// 运营可改的玩法配置。规则集版本、座位数来自编译产物，后台只读展示。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSetting {
    pub game_id: String,
    pub display_name: String,
    pub enabled: bool,
    /// `casual` / `ranked` / `mixed` 子集；空表在 enabled=true 时非法。
    pub modes: Vec<String>,
    /// 结算后自动开下一局的间隔（毫秒）。
    pub next_deal_ms: u64,
    pub updated_at_ms: u64,
}

impl Default for GameSetting {
    fn default() -> Self {
        Self {
            game_id: DEFAULT_GAME_ID.to_owned(),
            display_name: "掼蛋".to_owned(),
            enabled: true,
            modes: vec!["casual".to_owned(), "ranked".to_owned(), "mixed".to_owned()],
            next_deal_ms: 5_000,
            updated_at_ms: 0,
        }
    }
}

impl GameSetting {
    pub fn mode_enabled(&self, mode: Mode) -> bool {
        self.enabled && self.modes.iter().any(|m| m == mode.as_str())
    }
}

/// 后台写操作审计。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminAudit {
    pub audit_id: String,
    pub action: String,
    pub target: String,
    pub actor: String,
    pub detail: Value,
    pub created_at_ms: u64,
}

/// 概览统计；PG 不可用时只填 `Auth` 能提供的那几项。
#[derive(Clone, Debug, Default)]
pub struct AdminStats {
    pub users_total: i64,
    pub users_active: i64,
    pub users_disabled: i64,
    pub users_recent: i64,
    pub matches_total: i64,
    pub matches_recent: i64,
    pub rounds_total: i64,
}

/// `match_record` 的后台只读行。
#[derive(Clone, Debug)]
pub struct AdminMatchRow {
    pub match_id: String,
    pub game_id: String,
    pub match_type: String,
    pub winner_team: Option<i16>,
    pub end_reason: Option<String>,
    pub seats_count: i16,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
}

/// 进程级后台状态：配置热缓存 + 审计内存副本。
#[derive(Debug)]
pub struct AdminState {
    setting: RwLock<GameSetting>,
    audits: Mutex<Vec<AdminAudit>>,
    audit_seq: AtomicU64,
}

impl Default for AdminState {
    fn default() -> Self {
        Self {
            setting: RwLock::new(GameSetting::default()),
            audits: Mutex::new(Vec::new()),
            audit_seq: AtomicU64::new(1),
        }
    }
}

impl AdminState {
    pub async fn setting(&self) -> GameSetting {
        self.setting.read().await.clone()
    }

    /// 玩法配置快照（大厅玩法列表读它，避免把内部锁露出去）。
    pub async fn game_setting(&self) -> GameSetting {
        self.setting.read().await.clone()
    }

    pub async fn game_enabled(&self) -> bool {
        self.setting.read().await.enabled
    }

    pub async fn mode_enabled(&self, mode: Mode) -> bool {
        self.setting.read().await.mode_enabled(mode)
    }

    /// 建房读取；配置写入口已经做过区间校验，这里再夹一次防脏库。
    pub async fn next_deal_ms(&self) -> u64 {
        self.setting.read().await.next_deal_ms.clamp(1_000, 600_000)
    }

    pub async fn replace_setting(&self, setting: GameSetting) {
        *self.setting.write().await = setting;
    }

    /// 启动时从 PG 恢复最近审计（DB 读失败时保持空表）。
    pub async fn restore_audits(&self, rows: Vec<AdminAudit>) {
        let mut audits = self.audits.lock().await;
        audits.clear();
        audits.extend(rows.into_iter().take(200));
    }

    pub async fn audits(&self, limit: usize) -> Vec<AdminAudit> {
        self.audits
            .lock()
            .await
            .iter()
            .take(limit.min(200))
            .cloned()
            .collect()
    }

    /// 记一条审计并放进内存；返回副本供 PG 持久化。
    pub async fn record_audit(
        &self,
        action: &str,
        target: &str,
        detail: Value,
        actor: &str,
    ) -> AdminAudit {
        let now = now_unix_ms();
        let seq = self.audit_seq.fetch_add(1, Ordering::Relaxed);
        let audit = AdminAudit {
            audit_id: format!("a-{now:x}-{seq:08x}"),
            action: action.to_owned(),
            target: target.to_owned(),
            actor: audit_actor(actor),
            detail,
            created_at_ms: now,
        };
        let mut audits = self.audits.lock().await;
        audits.insert(0, audit.clone());
        audits.truncate(200);
        audit
    }
}

/// 后台路由；由 `App::router` merge。
pub fn router() -> Router<std::sync::Arc<App>> {
    Router::new()
        .route("/api/admin/session", get(session))
        .route("/api/admin/overview", get(overview))
        .route("/api/admin/users", get(users))
        .route("/api/admin/users/{player_id}", patch(update_user))
        .route("/api/admin/rooms", get(rooms))
        .route("/api/admin/games", get(games))
        .route("/api/admin/games/{game_id}", put(update_game))
        .route("/api/admin/matches", get(matches))
        .route("/api/admin/audits", get(audits))
}

/// 管理令牌校验：空令牌 = 后台关闭；缺失/不符统一 401。
fn check_admin(app: &App, headers: &HeaderMap) -> Result<(), Box<Response>> {
    if app.cfg.admin_token.trim().is_empty() {
        return Err(Box::new(
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"reason": "ADMIN_DISABLED"})),
            )
                .into_response(),
        ));
    }
    let provided = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
                .map(str::trim)
        })
        .filter(|v| !v.is_empty());
    let Some(provided) = provided else {
        return Err(Box::new(unauthorized("ADMIN_UNAUTHORIZED")));
    };
    if !constant_eq(provided.as_bytes(), app.cfg.admin_token.trim().as_bytes()) {
        return Err(Box::new(unauthorized("ADMIN_UNAUTHORIZED")));
    }
    Ok(())
}

fn unauthorized(reason: &str) -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({"reason": reason}))).into_response()
}

fn bad_request(reason: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"reason": reason}))).into_response()
}

fn not_found(reason: &str) -> Response {
    (StatusCode::NOT_FOUND, Json(json!({"reason": reason}))).into_response()
}

fn storage_unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"reason": "STORAGE_UNAVAILABLE"})),
    )
        .into_response()
}

async fn session(State(app): State<std::sync::Arc<App>>, headers: HeaderMap) -> Response {
    if let Err(resp) = check_admin(&app, &headers) {
        return *resp;
    }
    Json(json!({
        "ok": true,
        "gameId": DEFAULT_GAME_ID,
        "rulesetVersion": gf_table_runtime::DEFAULT_RULESET_VERSION,
        "serverTimeMs": now_unix_ms(),
    }))
    .into_response()
}

async fn overview(State(app): State<std::sync::Arc<App>>, headers: HeaderMap) -> Response {
    if let Err(resp) = check_admin(&app, &headers) {
        return *resp;
    }
    let now = now_unix_ms();
    let since = now.saturating_sub(24 * 60 * 60 * 1_000);
    let stats = match app.store().filter(|store| store.pg_enabled()) {
        Some(store) => match store.admin_stats(since).await {
            Ok(stats) => stats,
            Err(err) => {
                eprintln!("[admin] 概览统计失败：{err}");
                memory_stats(&app, since).await
            }
        },
        None => memory_stats(&app, since).await,
    };
    let setting = app.admin.setting().await;
    let rooms = app.rooms.read().await.len();
    Json(json!({
        "generatedAtMs": now,
        "users": {
            "total": stats.users_total,
            "active": stats.users_active,
            "disabled": stats.users_disabled,
            "recent24h": stats.users_recent,
        },
        "matches": {
            "total": stats.matches_total,
            "recent24h": stats.matches_recent,
            "rounds": stats.rounds_total,
            "activeRooms": rooms,
        },
        "game": {
            "gameId": setting.game_id,
            "displayName": setting.display_name,
            "enabled": setting.enabled,
            "modes": setting.modes,
            "nextDealMs": setting.next_deal_ms,
        },
        "storage": {
            "postgres": app.store().is_some_and(|store| store.pg_enabled()),
            "redis": app.store().is_some_and(|store| store.redis_enabled()),
        },
        "uptimeMs": app.started_at.elapsed().as_millis() as u64,
    }))
    .into_response()
}

/// 房间巡检：只读当前在册房间，按建房时间倒序；不含手牌与任何令牌。
async fn rooms(
    State(app): State<std::sync::Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
) -> Response {
    if let Err(resp) = check_admin(&app, &headers) {
        return *resp;
    }
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let mut handles = app.room_handles().await;
    handles.sort_by_key(|room| std::cmp::Reverse(room.created_at_ms()));
    let total = handles.len();
    let offset = ((page - 1) as usize).saturating_mul(page_size as usize);
    let selected = handles
        .into_iter()
        .skip(offset)
        .take(page_size as usize)
        .collect::<Vec<_>>();
    let mut items = Vec::with_capacity(selected.len());
    for handle in selected {
        let seats = handle.seat_directory().await;
        let online = handle.connected_seats().await;
        let meta = handle.snapshot_meta().await;
        let started = handle.started();
        let seated = seats.iter().filter(|(_, owner)| owner.is_some()).count();
        let item = json!({
            "matchId": handle.match_id(),
            "gameId": DEFAULT_GAME_ID,
            "mode": handle.mode().await,
            "phase": meta.as_ref().map(|m| m.3),
            "phaseLabel": if !started {
                "waiting"
            } else {
                meta.as_ref().map(|m| phase_label(m.3)).unwrap_or("unknown")
            },
            "roundNo": meta.as_ref().map(|m| m.1),
            "roomSeq": meta.as_ref().map(|m| m.0),
            "stateHash": meta.as_ref().map(|m| hex32(&m.2)),
            "seatsCount": crate::host::SEATS,
            "seated": seated,
            "online": online.len(),
            "closed": handle.closed(),
            "createdAtMs": handle.created_at_ms(),
            "uptimeMs": handle.now_ms(),
            "seats": seats.iter().map(|(seat, owner)| json!({
                "seat": seat,
                "playerId": owner,
                "online": online.contains(seat),
            })).collect::<Vec<_>>(),
        });
        items.push(item);
    }
    Json(json!({
        "items": items,
        "total": total,
        "page": page,
        "pageSize": page_size,
    }))
    .into_response()
}

fn phase_label(phase: u16) -> &'static str {
    match phase {
        0 => "deal",
        1 => "tribute",
        2 => "play",
        3 => "settle",
        _ => "unknown",
    }
}

fn hex32(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// 审计操作者：单令牌后台没有账号体系，允许运维在请求头 `X-Admin-Actor`
/// 里自报一个 1–32 字符的标识（中文名也可以）；缺省记 `admin`。
fn audit_actor(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 32 || trimmed.chars().any(|c| c.is_control())
    {
        return "admin".to_owned();
    }
    trimmed.to_owned()
}

fn actor_from_headers(headers: &HeaderMap) -> String {
    headers
        .get("x-admin-actor")
        .and_then(|value| std::str::from_utf8(value.as_bytes()).ok())
        .map(audit_actor)
        .unwrap_or_else(|| "admin".to_owned())
}

async fn memory_stats(app: &App, since: u64) -> AdminStats {
    let profiles = app.auth.profiles_snapshot();
    let users_total = profiles.len() as i64;
    let users_active = profiles.iter().filter(|p| p.status == "active").count() as i64;
    let users_recent = profiles.iter().filter(|p| p.created_at_ms >= since).count() as i64;
    AdminStats {
        users_total,
        users_active,
        users_disabled: users_total - users_active,
        users_recent,
        ..AdminStats::default()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsersQuery {
    #[serde(default)]
    keyword: String,
    #[serde(default)]
    status: String,
    page: Option<u32>,
    page_size: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UserView {
    player_id: String,
    nickname: String,
    phone_tail4: String,
    status: String,
    created_at_ms: u64,
}

impl From<&Profile> for UserView {
    fn from(profile: &Profile) -> Self {
        Self {
            player_id: profile.player_id.clone(),
            nickname: profile.nickname.clone(),
            phone_tail4: profile.phone_tail4.clone(),
            status: profile.status.clone(),
            created_at_ms: profile.created_at_ms,
        }
    }
}

async fn users(
    State(app): State<std::sync::Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<UsersQuery>,
) -> Response {
    if let Err(resp) = check_admin(&app, &headers) {
        return *resp;
    }
    let keyword = query.keyword.trim().to_lowercase();
    let status = query.status.trim().to_owned();
    if !matches!(status.as_str(), "" | "active" | "disabled") {
        return bad_request("USER_STATUS_INVALID");
    }
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 50);
    let offset = u64::from((page - 1) * page_size);

    let (profiles, total) = match app.store().filter(|store| store.pg_enabled()) {
        Some(store) => match store
            .admin_list_users(&keyword, &status, i64::from(page_size), offset as i64)
            .await
        {
            Ok(pair) => pair,
            Err(err) => {
                eprintln!("[admin] 用户列表查询失败：{err}");
                return storage_unavailable();
            }
        },
        None => {
            let mut profiles = app.auth.profiles_snapshot();
            profiles.retain(|profile| {
                (status.is_empty() || profile.status == status)
                    && (keyword.is_empty()
                        || profile.player_id.to_lowercase().contains(&keyword)
                        || profile.nickname.to_lowercase().contains(&keyword)
                        || profile.phone_tail4.contains(&keyword))
            });
            profiles.sort_by_key(|profile| std::cmp::Reverse(profile.created_at_ms));
            let total = profiles.len();
            let page_rows = profiles
                .into_iter()
                .skip(offset as usize)
                .take(page_size as usize)
                .collect::<Vec<_>>();
            (page_rows, total as i64)
        }
    };

    let items: Vec<UserView> = profiles.iter().map(UserView::from).collect();
    Json(json!({
        "items": items,
        "total": total,
        "page": page,
        "pageSize": page_size,
    }))
    .into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserPatch {
    status: String,
}

async fn update_user(
    State(app): State<std::sync::Arc<App>>,
    headers: HeaderMap,
    Path(player_id): Path<String>,
    Json(body): Json<UserPatch>,
) -> Response {
    if let Err(resp) = check_admin(&app, &headers) {
        return *resp;
    }
    if !matches!(body.status.as_str(), "active" | "disabled") {
        return bad_request("USER_STATUS_INVALID");
    }
    let before = match app.store().filter(|store| store.pg_enabled()) {
        Some(store) => match store.find_profile_by_player_id(&player_id).await {
            Ok(Some(profile)) => profile,
            Ok(None) => return not_found("USER_NOT_FOUND"),
            Err(err) => {
                eprintln!("[admin] 读取用户失败：{err}");
                return storage_unavailable();
            }
        },
        None => match app.auth.profile(&player_id) {
            Some(profile) => profile,
            None => return not_found("USER_NOT_FOUND"),
        },
    };

    if let Some(store) = app.store().filter(|store| store.pg_enabled()) {
        match store.update_profile_status(&player_id, &body.status).await {
            Ok(true) => {}
            Ok(false) => return not_found("USER_NOT_FOUND"),
            Err(err) => {
                eprintln!("[admin] 更新用户状态失败：{err}");
                return storage_unavailable();
            }
        }
    }
    if !app.auth.set_profile_status(&player_id, &body.status) {
        // PG 里有、内存缓存没有：补一条档案，避免管理端看到的状态与实际不一致。
        let mut profile = before.clone();
        profile.status = body.status.clone();
        app.auth.remember_profile(profile);
    }

    let mut after = before;
    let from_status = after.status.clone();
    after.status = body.status.clone();
    let audit = app
        .admin
        .record_audit(
            "user.status",
            &player_id,
            json!({"from": from_status, "to": body.status}),
            &actor_from_headers(&headers),
        )
        .await;
    persist_audit(&app, &audit).await;
    Json(json!({"ok": true, "user": UserView::from(&after)})).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GamesQuery {
    #[serde(default)]
    keyword: String,
}

async fn games(
    State(app): State<std::sync::Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<GamesQuery>,
) -> Response {
    if let Err(resp) = check_admin(&app, &headers) {
        return *resp;
    }
    let setting = app.admin.setting().await;
    let keyword = query.keyword.trim();
    if !keyword.is_empty()
        && !setting.game_id.contains(keyword)
        && !setting.display_name.contains(keyword)
    {
        return Json(json!({"items": []})).into_response();
    }
    Json(json!({"items": [game_view(&setting)]})).into_response()
}

fn game_view(setting: &GameSetting) -> Value {
    json!({
        "gameId": setting.game_id,
        "displayName": setting.display_name,
        "enabled": setting.enabled,
        "modes": setting.modes,
        "nextDealMs": setting.next_deal_ms,
        "rulesetVersion": gf_table_runtime::DEFAULT_RULESET_VERSION,
        "seats": crate::host::SEATS,
        "updatedAtMs": setting.updated_at_ms,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GamePatch {
    display_name: String,
    enabled: bool,
    modes: Vec<String>,
    next_deal_ms: u64,
}

async fn update_game(
    State(app): State<std::sync::Arc<App>>,
    headers: HeaderMap,
    Path(game_id): Path<String>,
    Json(body): Json<GamePatch>,
) -> Response {
    if let Err(resp) = check_admin(&app, &headers) {
        return *resp;
    }
    if game_id != DEFAULT_GAME_ID {
        return not_found("GAME_NOT_FOUND");
    }
    let display_name = body.display_name.trim();
    if display_name.is_empty() || display_name.chars().count() > 24 {
        return bad_request("GAME_NAME_INVALID");
    }
    let mut modes = Vec::new();
    for mode in &body.modes {
        if !matches!(mode.as_str(), "casual" | "ranked" | "mixed") {
            return bad_request("GAME_MODE_INVALID");
        }
        if !modes.contains(mode) {
            modes.push(mode.clone());
        }
    }
    if body.enabled && modes.is_empty() {
        return bad_request("GAME_MODES_REQUIRED");
    }
    if !(1_000..=600_000).contains(&body.next_deal_ms) {
        return bad_request("NEXT_DEAL_INVALID");
    }

    let before = app.admin.setting().await;
    let setting = GameSetting {
        game_id: DEFAULT_GAME_ID.to_owned(),
        display_name: display_name.to_owned(),
        enabled: body.enabled,
        modes,
        next_deal_ms: body.next_deal_ms,
        updated_at_ms: now_unix_ms(),
    };
    if let Some(store) = app.store().filter(|store| store.pg_enabled()) {
        if let Err(err) = store.upsert_game_setting(&setting).await {
            eprintln!("[admin] 保存玩法配置失败：{err}");
            return storage_unavailable();
        }
    }
    app.admin.replace_setting(setting.clone()).await;
    let audit = app
        .admin
        .record_audit(
            "game.setting",
            DEFAULT_GAME_ID,
            json!({
                "from": {
                    "enabled": before.enabled,
                    "modes": before.modes,
                    "nextDealMs": before.next_deal_ms,
                },
                "to": {
                    "enabled": setting.enabled,
                    "modes": setting.modes,
                    "nextDealMs": setting.next_deal_ms,
                },
            }),
            &actor_from_headers(&headers),
        )
        .await;
    persist_audit(&app, &audit).await;
    Json(json!({"ok": true, "game": game_view(&setting)})).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageQuery {
    page: Option<u32>,
    page_size: Option<u32>,
}

async fn matches(
    State(app): State<std::sync::Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
) -> Response {
    if let Err(resp) = check_admin(&app, &headers) {
        return *resp;
    }
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 50);
    let offset = u64::from((page - 1) * page_size);
    let (rows, total) = match app.store().filter(|store| store.pg_enabled()) {
        Some(store) => match store
            .admin_list_matches(i64::from(page_size), offset as i64)
            .await
        {
            Ok(pair) => pair,
            Err(err) => {
                eprintln!("[admin] 对局列表查询失败：{err}");
                return storage_unavailable();
            }
        },
        None => (Vec::new(), 0),
    };
    let items: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "matchId": row.match_id,
                "gameId": row.game_id,
                "matchType": row.match_type,
                "winnerTeam": row.winner_team,
                "endReason": row.end_reason,
                "seatsCount": row.seats_count,
                "startedAtMs": row.started_at_ms,
                "endedAtMs": row.ended_at_ms,
            })
        })
        .collect();
    Json(json!({"items": items, "total": total, "page": page, "pageSize": page_size}))
        .into_response()
}

async fn audits(State(app): State<std::sync::Arc<App>>, headers: HeaderMap) -> Response {
    if let Err(resp) = check_admin(&app, &headers) {
        return *resp;
    }
    let items = app.admin.audits(100).await;
    Json(json!({"items": items})).into_response()
}

async fn persist_audit(app: &App, audit: &AdminAudit) {
    let Some(store) = app.store().filter(|store| store.pg_enabled()) else {
        return;
    };
    if let Err(err) = store.insert_admin_audit(audit).await {
        eprintln!("[admin] 审计落库失败（内存仍保留）：{err}");
    }
}

/// 定长比较，避免按字节提前返回泄露后台令牌前缀。
fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}
