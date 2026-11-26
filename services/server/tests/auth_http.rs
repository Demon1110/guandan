//! 账号接口（短信 / 登录 / 刷新 / 资料）与 token 绑定座位的 HTTP 行为测试。

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gf_server::{App, ServerConfig};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const PHONE_A: &str = "13800138000";
const PHONE_B: &str = "13900139000";
const TEST_CODE: &str = "123456";

fn make_app() -> Arc<App> {
    App::new(ServerConfig {
        sms_test_whitelist: vec![PHONE_A.to_owned(), PHONE_B.to_owned()],
        sms_test_code: TEST_CODE.to_owned(),
        ..ServerConfig::default()
    })
}

async fn call(
    app: &Arc<App>,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
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

async fn login(app: &Arc<App>, phone: &str, code: &str) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({"phone": phone, "code": code, "clientVersion": "0.1.0"})),
    )
    .await
}

async fn token_of(app: &Arc<App>, phone: &str) -> String {
    let (status, body) = login(app, phone, TEST_CODE).await;
    assert_eq!(status, StatusCode::OK, "白名单登录失败：{body}");
    body["token"].as_str().expect("缺 token").to_owned()
}

#[tokio::test]
async fn sms_is_rate_limited_per_phone() {
    let app = make_app();
    let (status, body) = call(
        &app,
        "POST",
        "/api/auth/sms",
        None,
        Some(json!({"phone": PHONE_A})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["cooldownSec"], 60);
    assert!(body["requestId"]
        .as_str()
        .is_some_and(|id| id.starts_with("s-")));

    let (status, body) = call(
        &app,
        "POST",
        "/api/auth/sms",
        None,
        Some(json!({"phone": PHONE_A})),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["reason"], "RATE_LIMITED");
    assert!(body["retryAfterMs"].as_u64().is_some_and(|ms| ms > 0));
}

#[tokio::test]
async fn sms_rejects_malformed_phone() {
    let app = make_app();
    let (status, body) = call(
        &app,
        "POST",
        "/api/auth/sms",
        None,
        Some(json!({"phone": "1380013800"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["detail"], "PHONE_INVALID");
}

#[tokio::test]
async fn whitelist_login_issues_token_and_profile() {
    let app = make_app();
    let token = token_of(&app, PHONE_A).await;

    let (status, body) = call(&app, "GET", "/api/profile", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["phoneTail4"], "8000");
    assert_eq!(body["nickname"], "掼友8000");
    assert_eq!(body["stats"], json!([]));

    let (status, body) = call(&app, "GET", "/api/profile", Some("not-a-token"), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["reason"], "AUTH_INVALID");

    let (status, _) = call(&app, "GET", "/api/profile", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "资料接口必须带 token");
}

#[tokio::test]
async fn wrong_code_and_version_gate_are_rejected() {
    let app = make_app();
    let (status, body) = login(&app, PHONE_A, "000000").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["reason"], "AUTH_INVALID");

    let app = App::new(ServerConfig {
        min_client_version: "1.0.0".to_owned(),
        sms_test_whitelist: vec![PHONE_A.to_owned()],
        ..ServerConfig::default()
    });
    let (status, body) = call(
        &app,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({"phone": PHONE_A, "code": TEST_CODE, "clientVersion": "0.9.0"})),
    )
    .await;
    assert_eq!(status, StatusCode::UPGRADE_REQUIRED);
    assert_eq!(body["reason"], "CLIENT_TOO_OLD");
}

#[tokio::test]
async fn refresh_rotates_and_invalidates_old_token() {
    let app = make_app();
    let (_, body) = login(&app, PHONE_A, TEST_CODE).await;
    let refresh = body["refreshToken"]
        .as_str()
        .expect("缺 refreshToken")
        .to_owned();

    let (status, rotated) = call(
        &app,
        "POST",
        "/api/auth/refresh",
        None,
        Some(json!({"refreshToken": refresh})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(rotated["token"].as_str().is_some());
    assert_ne!(rotated["refreshToken"], refresh, "refresh 必须轮换");

    let (status, body) = call(
        &app,
        "POST",
        "/api/auth/refresh",
        None,
        Some(json!({"refreshToken": refresh})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["reason"], "AUTH_INVALID");
}

#[tokio::test]
async fn require_auth_gates_lobby_and_binds_seats() {
    let app = App::new(ServerConfig {
        require_auth: true,
        sms_test_whitelist: vec![PHONE_A.to_owned(), PHONE_B.to_owned()],
        ..ServerConfig::default()
    });
    let (status, _) = call(
        &app,
        "POST",
        "/api/room/create",
        None,
        Some(json!({"mode": "casual"})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "强制鉴权时匿名建房必须被拒"
    );

    let token_a = token_of(&app, PHONE_A).await;
    let token_b = token_of(&app, PHONE_B).await;
    let (status, body) = call(
        &app,
        "POST",
        "/api/room/create",
        Some(&token_a),
        Some(json!({"mode": "casual"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let code = body["inviteCode"]
        .as_str()
        .expect("缺 inviteCode")
        .to_owned();

    let (status, body) = call(
        &app,
        "POST",
        "/api/room/join",
        Some(&token_a),
        Some(json!({"inviteCode": code})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let seat_a = body["seat"].as_u64().expect("缺 seat");
    assert_eq!(seat_a, 0);

    let (status, body) = call(
        &app,
        "POST",
        "/api/room/join",
        Some(&token_b),
        Some(json!({"inviteCode": code})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["seat"], 1, "第二个玩家不能拿到 A 的预留座位");

    // 强制鉴权下匿名玩家连预占座位都不允许。
    let (status, body) = call(
        &app,
        "POST",
        "/api/room/join",
        None,
        Some(json!({"inviteCode": code})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["reason"], "AUTH_INVALID");
}
