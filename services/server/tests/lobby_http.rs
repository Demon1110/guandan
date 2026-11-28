//! 大厅 HTTP 接口（房间码 / 匹配队列）的行为测试。
//!
//! 只测 HTTP 面：房码分配到座位、满员拒绝、码失效、四人撮合成房、退队与超时。
//! 撮合只触发一次（入队时），不依赖后台线程，因此这里的断言是确定性的。

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gf_server::{App, ServerConfig};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

fn make_app(cfg: ServerConfig) -> Arc<App> {
    App::new(cfg)
}

async fn call(app: &Arc<App>, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(body) => {
            builder = builder.header("content-type", "application/json");
            builder
                .body(Body::from(body.to_string()))
                .expect("请求构造失败")
        }
        None => builder.body(Body::empty()).expect("请求构造失败"),
    };
    let response = app
        .clone()
        .router()
        .oneshot(request)
        .await
        .expect("路由调用失败");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("读响应体失败")
        .to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

async fn create_room(app: &Arc<App>, mode: &str) -> (String, String) {
    let (status, body) = call(
        app,
        "POST",
        "/api/room/create",
        Some(json!({"gameId": "guandan", "mode": mode, "seatsCount": 4})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "建房失败：{body}");
    (
        body["matchId"].as_str().expect("缺 matchId").to_owned(),
        body["inviteCode"]
            .as_str()
            .expect("缺 inviteCode")
            .to_owned(),
    )
}

async fn join(app: &Arc<App>, code: &str) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/api/room/join",
        Some(json!({"inviteCode": code})),
    )
    .await
}

#[tokio::test]
async fn room_code_assigns_spare_seats_and_rejects_overflow() {
    let app = make_app(ServerConfig::default());
    let (match_id, code) = create_room(&app, "casual").await;
    assert_eq!(code.len(), 6, "房间码 6 位");
    assert!(code
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()));

    let mut seats = Vec::new();
    for _ in 0..4 {
        let (status, body) = join(&app, &code.to_ascii_lowercase()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["matchId"], match_id);
        assert_eq!(body["accept"], true);
        seats.push(body["seat"].as_u64().expect("缺 seat") as u8);
    }
    seats.sort_unstable();
    assert_eq!(seats, vec![0, 1, 2, 3], "四个座位必须各分配一次");

    let (status, body) = join(&app, &code).await;
    assert_eq!(status, StatusCode::OK, "满员不经 HTTP 409（§5.15）");
    assert_eq!(body["accept"], false);
    assert_eq!(body["reason"], "RS_SEAT_TAKEN");
    assert_eq!(body["availableSeats"], json!([]), "满员时可用座位为空表");
}

#[tokio::test]
async fn unknown_or_expired_code_is_rejected() {
    let app = make_app(ServerConfig::default());
    let (status, body) = join(&app, "ZZZZZZ").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["reason"], "INVITE_CODE_UNKNOWN");

    // 码有效期为 0：出码后立即失效。
    let cfg = ServerConfig {
        room_code_ttl_ms: 0,
        ..Default::default()
    };
    let app = make_app(cfg);
    let (_, code) = create_room(&app, "casual").await;
    let (status, body) = join(&app, &code).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["reason"], "INVITE_CODE_EXPIRED");
}

#[tokio::test]
async fn invalid_create_arguments_are_rejected() {
    let app = make_app(ServerConfig::default());
    for body in [
        json!({"gameId": "mahjong", "mode": "casual"}),
        json!({"gameId": "guandan", "mode": "ranked2"}),
        json!({"gameId": "guandan", "mode": "casual", "seatsCount": 8}),
    ] {
        let (status, response) = call(&app, "POST", "/api/room/create", Some(body.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "入参 {body} 应被拒");
        assert_eq!(response["reason"], "RS_SCHEMA_INVALID");
    }
}

#[tokio::test]
async fn four_queued_tickets_form_one_room() {
    let app = make_app(ServerConfig::default());
    let mut tickets = Vec::new();
    for _ in 0..4 {
        let (status, body) = call(
            &app,
            "POST",
            "/api/match/queue",
            Some(json!({"gameId": "guandan", "mode": "casual"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        tickets.push(body["ticketId"].as_str().expect("缺 ticketId").to_owned());
    }

    let mut match_ids = Vec::new();
    for ticket in &tickets {
        let (status, body) = call(&app, "GET", &format!("/api/match/queue/{ticket}"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["state"], "matched");
        match_ids.push(
            body["matchId"]
                .as_str()
                .expect("matched 必须带 matchId")
                .to_owned(),
        );
    }
    assert!(
        match_ids.windows(2).all(|w| w[0] == w[1]),
        "四张票必须落到同一场次"
    );
}

#[tokio::test]
async fn queue_does_not_mix_modes_and_cancel_leaves_waiting() {
    let app = make_app(ServerConfig::default());
    let mut casual = Vec::new();
    for _ in 0..3 {
        let (_, body) = call(
            &app,
            "POST",
            "/api/match/queue",
            Some(json!({"gameId": "guandan", "mode": "casual"})),
        )
        .await;
        casual.push(body["ticketId"].as_str().expect("缺 ticketId").to_owned());
    }
    let (_, body) = call(
        &app,
        "POST",
        "/api/match/queue",
        Some(json!({"gameId": "guandan", "mode": "ranked"})),
    )
    .await;
    let ranked = body["ticketId"].as_str().expect("缺 ticketId").to_owned();

    for ticket in &casual {
        let (_, body) = call(&app, "GET", &format!("/api/match/queue/{ticket}"), None).await;
        assert_eq!(body["state"], "waiting", "三张 casual 票不该成房");
    }
    let (_, body) = call(&app, "GET", &format!("/api/match/queue/{ranked}"), None).await;
    assert_eq!(body["state"], "waiting", "ranked 票不得混进 casual 房");

    let (status, body) = call(
        &app,
        "DELETE",
        &format!("/api/match/queue/{}", casual[0]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    let (_, body) = call(
        &app,
        "GET",
        &format!("/api/match/queue/{}", casual[0]),
        None,
    )
    .await;
    assert_eq!(body["state"], "timeout", "退队后状态转 timeout");
}

#[tokio::test]
async fn queue_timeout_is_reported_by_poll() {
    let cfg = ServerConfig {
        match_timeout_ms: 5,
        ..Default::default()
    };
    let app = make_app(cfg);
    let (_, body) = call(
        &app,
        "POST",
        "/api/match/queue",
        Some(json!({"gameId": "guandan", "mode": "mixed"})),
    )
    .await;
    let ticket = body["ticketId"].as_str().expect("缺 ticketId").to_owned();
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let (_, body) = call(&app, "GET", &format!("/api/match/queue/{ticket}"), None).await;
    assert_eq!(body["state"], "timeout");
}

#[tokio::test]
async fn oversized_json_body_is_rejected() {
    let app = make_app(ServerConfig::default());
    let oversized = json!({
        "gameId": "guandan",
        "mode": "x".repeat(300 * 1024),
    });
    let (status, _) = call(&app, "POST", "/api/match/queue", Some(oversized)).await;
    assert_eq!(
        status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "超过 256 KiB 的 JSON 必须在进入 handler 前被拒"
    );
}
