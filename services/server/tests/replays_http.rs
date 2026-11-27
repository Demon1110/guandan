//! 回放接口（`GET /api/replays/{matchId}`）的鉴权与拒绝路径。
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gf_server::{App, ServerConfig, SmsConfig};
use serde_json::{json, Value};
use tower::ServiceExt;

const PHONE: &str = "13800138000";
const CODE: &str = "123456";

fn make_app() -> Arc<App> {
    App::new(ServerConfig {
        sms: SmsConfig {
            test_whitelist: vec![PHONE.to_owned()],
            test_code: CODE.to_owned(),
            ..SmsConfig::default()
        },
        ..ServerConfig::default()
    })
}

async fn call(app: &Arc<App>, uri: &str, token: Option<&str>) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .router()
        .oneshot(builder.body(Body::empty()).expect("请求构造"))
        .await
        .expect("路由");
    let status = response.status();
    let bytes = http_body_util::BodyExt::collect(response.into_body())
        .await
        .expect("读响应")
        .to_bytes()
        .to_vec();
    (status, bytes)
}

async fn login(app: &Arc<App>) -> String {
    let (status, bytes) = {
        let request = Request::builder()
            .method("POST")
            .uri("/api/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"phone": PHONE, "code": CODE, "clientVersion": "0.1.0"}).to_string(),
            ))
            .expect("请求构造");
        let response = app.clone().router().oneshot(request).await.expect("路由");
        let status = response.status();
        let bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .expect("读响应")
            .to_bytes()
            .to_vec();
        (status, bytes)
    };
    assert_eq!(status, StatusCode::OK);
    let value: Value = serde_json::from_slice(&bytes).expect("JSON");
    value["token"].as_str().expect("token").to_owned()
}

#[tokio::test]
async fn replay_requires_login_and_seat_ownership() {
    let app = make_app();
    let (status, _) = call(&app, "/api/replays/m-none", None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "回放包含他人手牌，不能匿名读"
    );

    let token = login(&app).await;
    let (status, _) = call(&app, "/api/replays/m-none", Some(&token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "没有这桌就是 404");

    app.create_room("m-exists").await.expect("建房");
    let (status, _) = call(&app, "/api/replays/m-exists", Some(&token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "没坐这桌不能导出");
}
