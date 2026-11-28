//! 运营后台接口的最小 HTTP 行为测试：令牌鉴权、用户启停、玩法配置。
//!
//! 不依赖 PG：用户列表走 `Auth` 内存档案，配置走 `AdminState` 热缓存。

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gf_server::{App, ServerConfig, SmsConfig};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const PHONE_A: &str = "13800138000";
const PHONE_B: &str = "13900139000";
const TEST_CODE: &str = "123456";
const ADMIN_TOKEN: &str = "gf-dev-admin-token";

fn make_app(admin_token: &str) -> Arc<App> {
    App::new(ServerConfig {
        admin_token: admin_token.to_owned(),
        sms: SmsConfig {
            test_whitelist: vec![PHONE_A.to_owned(), PHONE_B.to_owned()],
            test_code: TEST_CODE.to_owned(),
            ..SmsConfig::default()
        },
        ..ServerConfig::default()
    })
}

async fn call(
    app: &Arc<App>,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let request = match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("请求构造失败"),
        None => builder.body(Body::empty()).expect("请求构造失败"),
    };
    let response = app
        .clone()
        .router()
        .oneshot(request)
        .await
        .expect("路由失败");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("读响应")
        .to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn admin(
    app: &Arc<App>,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    call(app, method, uri, Some(ADMIN_TOKEN), body).await
}

async fn request_sms(app: &Arc<App>, phone: &str) {
    let (status, body) = call(
        app,
        "POST",
        "/api/auth/sms",
        None,
        Some(json!({"phone": phone})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "发码失败：{body}");
}

async fn login(app: &Arc<App>, phone: &str) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({"phone": phone, "code": TEST_CODE, "clientVersion": "0.1.0"})),
    )
    .await
}

#[tokio::test]
async fn admin_is_off_when_token_empty() {
    let app = make_app("");
    let (status, body) = admin(&app, "GET", "/api/admin/session", None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["reason"], "ADMIN_DISABLED");
}

#[tokio::test]
async fn admin_rejects_missing_or_wrong_token() {
    let app = make_app(ADMIN_TOKEN);
    let (status, body) = call(&app, "GET", "/api/admin/session", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["reason"], "ADMIN_UNAUTHORIZED");

    let (status, _) = call(&app, "GET", "/api/admin/session", Some("wrong-token"), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = admin(&app, "GET", "/api/admin/session", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["gameId"], "guandan");
}

#[tokio::test]
async fn admin_users_can_be_disabled_and_reenabled() {
    let app = make_app(ADMIN_TOKEN);
    request_sms(&app, PHONE_A).await;
    let (status, a) = login(&app, PHONE_A).await;
    assert_eq!(status, StatusCode::OK);
    request_sms(&app, PHONE_B).await;
    let (status, b) = login(&app, PHONE_B).await;
    assert_eq!(status, StatusCode::OK);
    let player_a = a["playerId"].as_str().expect("playerId").to_owned();
    let token_a = a["token"].as_str().expect("token").to_owned();
    assert_ne!(player_a, b["playerId"].as_str().expect("playerId"));

    let (status, body) = admin(&app, "GET", "/api/admin/users", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 2);
    assert_eq!(body["items"].as_array().expect("items").len(), 2);

    let (status, body) = admin(
        &app,
        "GET",
        &format!("/api/admin/users?keyword={player_a}&status=active"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);

    let (status, body) = admin(
        &app,
        "PATCH",
        &format!("/api/admin/users/{player_a}"),
        Some(json!({"status": "disabled"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["status"], "disabled");

    // 禁用后旧 access 立刻失效。
    let (status, _) = call(&app, "GET", "/api/profile", Some(&token_a), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = admin(
        &app,
        "PATCH",
        &format!("/api/admin/users/{player_a}"),
        Some(json!({"status": "active"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["status"], "active");
    // 同一条 access 在解禁后恢复可用（未走登出撤销）。
    let (status, _) = call(&app, "GET", "/api/profile", Some(&token_a), None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = admin(&app, "GET", "/api/admin/audits", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().expect("items").len(), 2);
}

#[tokio::test]
async fn admin_lists_rooms_with_mode_and_seats() {
    let app = make_app(ADMIN_TOKEN);
    request_sms(&app, PHONE_A).await;
    let (_, login_body) = login(&app, PHONE_A).await;
    let player_token = login_body["token"].as_str().unwrap().to_owned();

    let (status, body) = call(
        &app,
        "POST",
        "/api/room/create",
        Some(&player_token),
        Some(json!({"mode": "casual"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "建房失败：{body}");
    let match_id = body["matchId"].as_str().expect("matchId").to_owned();

    let (status, body) = admin(&app, "GET", "/api/admin/rooms", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    let room = &body["items"][0];
    assert_eq!(room["matchId"], match_id);
    assert_eq!(room["mode"], "casual");
    assert_eq!(room["seatsCount"], 4);
    assert_eq!(room["seated"], 0);
    assert_eq!(room["online"], 0);
    assert_eq!(room["phaseLabel"], "waiting");
    assert_eq!(room["seats"].as_array().expect("seats").len(), 4);
    assert!(room["stateHash"].is_null());

    let (status, body) = admin(&app, "GET", "/api/admin/rooms?page=2&pageSize=1", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"].as_array().expect("items").len(), 0);
}

#[tokio::test]
async fn admin_audit_records_operator_header() {
    let app = make_app(ADMIN_TOKEN);
    request_sms(&app, PHONE_A).await;
    let (_, login_body) = login(&app, PHONE_A).await;
    let player_id = login_body["playerId"].as_str().unwrap().to_owned();

    let request = Request::builder()
        .method("PATCH")
        .uri(format!("/api/admin/users/{player_id}"))
        .header("authorization", format!("Bearer {ADMIN_TOKEN}"))
        .header("x-admin-actor", "张三")
        .header("content-type", "application/json")
        .body(Body::from(json!({"status": "disabled"}).to_string()))
        .expect("请求构造失败");
    let response = app
        .clone()
        .router()
        .oneshot(request)
        .await
        .expect("路由失败");
    assert_eq!(response.status(), StatusCode::OK);

    let (status, body) = admin(&app, "GET", "/api/admin/audits", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"][0]["actor"], "张三");
}

#[tokio::test]
async fn games_list_stays_visible_when_client_is_too_old() {
    let app = App::new(ServerConfig {
        min_client_version: "1.10.0".to_owned(),
        ..ServerConfig::default()
    });

    let (status, body) = call(&app, "GET", "/api/games?clientVersion=1.9.0", None, None).await;
    assert_eq!(status, StatusCode::OK, "旧客户端也要能看到玩法列表：{body}");
    assert_eq!(body["compatible"], false);
    assert_eq!(body["upgrade"]["required"], true);
    assert_eq!(body["upgrade"]["reason"], "CLIENT_TOO_OLD");
    assert_eq!(body["minClientVersion"], "1.10.0");
    let game = &body["items"][0];
    assert_eq!(game["gameId"], "guandan");
    assert_eq!(game["seatsCount"], 4);
    assert_eq!(game["modes"].as_array().expect("modes").len(), 3);

    let (status, body) = call(&app, "GET", "/api/games?clientVersion=1.10.0", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["compatible"], true);
    assert!(body["upgrade"].is_null());

    // 没上报版本按旧客户端处理，但仍然返回列表。
    let (status, body) = call(&app, "GET", "/api/games", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["compatible"], false);
    assert_eq!(body["items"].as_array().expect("items").len(), 1);
}

#[tokio::test]
async fn admin_game_switch_is_reflected_in_public_games_list() {
    let app = make_app(ADMIN_TOKEN);
    let (status, _) = admin(
        &app,
        "PUT",
        "/api/admin/games/guandan",
        Some(json!({
            "displayName": "掼蛋",
            "enabled": false,
            "modes": ["casual"],
            "nextDealMs": 5000,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call(&app, "GET", "/api/games", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"][0]["enabled"], false);
    let modes = body["items"][0]["modes"].as_array().expect("modes");
    assert!(modes.iter().all(|mode| mode["enabled"] == false));
}

#[tokio::test]
async fn admin_game_setting_validates_and_gates_room_create() {
    let app = make_app(ADMIN_TOKEN);
    request_sms(&app, PHONE_A).await;
    let (_, login_body) = login(&app, PHONE_A).await;
    let player_token = login_body["token"].as_str().unwrap().to_owned();

    let (status, body) = admin(&app, "GET", "/api/admin/games", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"][0]["enabled"], true);
    assert_eq!(body["items"][0]["rulesetVersion"], "guandan-classic-1.0.0");

    let (status, body) = admin(
        &app,
        "PUT",
        "/api/admin/games/guandan",
        Some(json!({
            "displayName": "掼蛋·测试",
            "enabled": true,
            "modes": ["ranked", "unknown"],
            "nextDealMs": 5_000,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["reason"], "GAME_MODE_INVALID");

    let (status, _) = admin(
        &app,
        "PUT",
        "/api/admin/games/guandan",
        Some(json!({
            "displayName": "掼蛋·测试",
            "enabled": true,
            "modes": ["ranked"],
            "nextDealMs": 5_000,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = call(
        &app,
        "POST",
        "/api/room/create",
        Some(&player_token),
        Some(json!({"mode": "casual"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["reason"], "GAME_MODE_DISABLED");

    let (status, _) = admin(
        &app,
        "PUT",
        "/api/admin/games/guandan",
        Some(json!({
            "displayName": "掼蛋·测试",
            "enabled": false,
            "modes": ["casual", "ranked"],
            "nextDealMs": 5_000,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = call(
        &app,
        "POST",
        "/api/room/create",
        Some(&player_token),
        Some(json!({"mode": "ranked"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["reason"], "GAME_MODE_DISABLED");
}
