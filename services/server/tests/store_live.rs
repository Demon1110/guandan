//! 落库的实库冒烟：只有给了 `GF_TEST_DATABASE_URL` / `GF_TEST_REDIS_URL` 才跑。
//!
//! 本机与 CI 默认都没有 PG/Redis，所以这里必须「缺环境即跳过」，不能变成
//! 一条只能靠人工记忆的 `#[ignore]`。跑法：
//!
//! ```powershell
//! $env:GF_TEST_DATABASE_URL="postgres://gf:gf@127.0.0.1:5432/gf"
//! $env:GF_TEST_REDIS_URL="redis://127.0.0.1:6379"
//! cargo test --test store_live -- --nocapture
//! ```

use gf_server::Store;

fn pg_url() -> Option<String> {
    std::env::var("GF_TEST_DATABASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
}

fn redis_url() -> Option<String> {
    std::env::var("GF_TEST_REDIS_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
}

#[tokio::test]
async fn settlement_lands_in_pg_and_redis_when_configured() {
    let Some(pg) = pg_url() else {
        eprintln!("跳过：未设置 GF_TEST_DATABASE_URL");
        return;
    };
    let store = Store::connect(Some(&pg), redis_url().as_deref()).await;
    assert!(store.enabled(), "给了连接串却两侧都没连上");

    let match_id = format!("m-test-{}", std::process::id());
    store
        .insert_match(&match_id, "guandan", "human-casual", 4, 1_700_000_000_000)
        .await;

    let row = gf_server::RoundRow {
        match_id: match_id.clone(),
        round_no: 1,
        players: vec![
            (0, Some("p-1".to_owned()), 1),
            (1, None, 2),
            (2, Some("p-2".to_owned()), 3),
            (3, None, 4),
        ],
        ranks: vec![0, 1, 2, 3],
        result_json: r#"{"roundNo":1}"#.to_owned(),
        levels_after: vec![2, 2],
        ruleset_version: "guandan-classic-1.0.0".to_owned(),
        started_at_ms: 1_700_000_000_000,
        ended_at_ms: 1_700_000_120_000,
    };
    let actions = vec!["{\"kind\":\"pass\"}".to_owned()];
    // 直接走 `Store` 的公开方法与 App 的事件路径同构，避免测试复制一遍补玩家的逻辑。
    store.insert_round(&row).await;
    store.append_actions(&match_id, &actions).await;
    store.put_snapshot(&match_id, r#"{"roomSeq":3}"#).await;
    store.queue_upsert("guandan", "casual", "t-1", 1).await;
    store.queue_remove("guandan", "casual", "t-1").await;

    // 读回：`round_record` 与 `settlement_event` 必须成对出现。
    let pool = sqlx::PgPool::connect(&pg).await.expect("回读连接失败");
    let rounds: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM round_record WHERE match_id = $1 AND round_no = 1",
    )
    .bind(&match_id)
    .fetch_one(&pool)
    .await
    .expect("查 round_record 失败");
    assert_eq!(rounds, 1, "一局应恰好落一行");
    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM settlement_event WHERE settlement_id = $1")
            .bind(format!("{match_id}:1"))
            .fetch_one(&pool)
            .await
            .expect("查 settlement_event 失败");
    assert_eq!(events, 1, "结算事件应与战绩同事务写入");
    // 幂等：再写一次不应多出行。
    store.insert_round(&row).await;
    let rounds_again: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM round_record WHERE match_id = $1 AND round_no = 1",
    )
    .bind(&match_id)
    .fetch_one(&pool)
    .await
    .expect("复查 round_record 失败");
    assert_eq!(rounds_again, 1, "重复落库必须被 ON CONFLICT 吃掉");
    // jsonb 直接解到 `serde_json::Value` 要 sqlx 的 `json` feature；这里取 text 再解析，
    // 既少一个 feature，也顺便验证「库里存的确实是合法 JSON 文本」。
    let players_text: String = sqlx::query_scalar(
        "SELECT players::text FROM round_record WHERE match_id = $1 AND round_no = 1",
    )
    .bind(&match_id)
    .fetch_one(&pool)
    .await
    .expect("查 players 失败");
    let players: serde_json::Value =
        serde_json::from_str(&players_text).expect("players 必须是合法 JSON");
    assert_eq!(players.as_array().map(Vec::len), Some(4));
    assert_eq!(players[0]["rank"], 1);
}

/// 账号主数据与会话表：建档幂等、显式注册冲突、refresh 轮换、撤销表回读。
#[tokio::test]
async fn account_state_roundtrips_through_pg_when_configured() {
    let Some(pg) = pg_url() else {
        eprintln!("跳过：未设置 GF_TEST_DATABASE_URL");
        return;
    };
    let store = Store::connect(Some(&pg), None).await;
    assert!(store.pg_enabled(), "给了连接串却没连上 PG");
    let now = 1_700_000_000_000u64;
    let tag = format!("{}", std::process::id());
    let phone_a = format!("live-a-{tag}");
    let phone_b = format!("live-b-{tag}");

    let first = store
        .find_or_create_profile(&phone_a, "1234", now, "p-00000001")
        .await
        .expect("首登建档");
    assert_eq!(first.player_id, "p-00000001");
    let again = store
        .find_or_create_profile(&phone_a, "1234", now + 1, "p-00000002")
        .await
        .expect("重复登录");
    assert_eq!(again.player_id, first.player_id, "同一手机号必须复用档案");

    let conflict = store
        .create_profile(&phone_a, "1234", now, "p-00000003")
        .await
        .expect("冲突查询");
    assert!(conflict.is_none(), "已注册手机号显式注册必须返回冲突");
    let created = store
        .create_profile(&phone_b, "5678", now, "p-00000004")
        .await
        .expect("显式注册")
        .expect("新手机号必须建档");
    assert_eq!(created.player_id, "p-00000004");

    let old_hash = format!("refresh-old-{tag}");
    let new_hash = format!("refresh-new-{tag}");
    store
        .insert_refresh_token(&old_hash, &first.player_id, now + 1_000, now)
        .await
        .expect("写 refresh");
    store
        .rotate_refresh_token(
            &old_hash,
            &new_hash,
            &first.player_id,
            now + 20_000,
            now + 1,
        )
        .await
        .expect("轮换 refresh");
    let tokens = store
        .load_refresh_tokens(now + 2)
        .await
        .expect("读 refresh");
    assert!(tokens
        .iter()
        .any(|(hash, player_id, _)| hash == &new_hash && player_id == &first.player_id));
    assert!(
        !tokens.iter().any(|(hash, _, _)| hash == &old_hash),
        "轮换后旧行必须删除"
    );
    assert!(
        store
            .rotate_refresh_token(&old_hash, "x", &first.player_id, now + 3_000, now + 2)
            .await
            .is_err(),
        "旧 refresh 不能再轮换"
    );

    store
        .insert_access_revocation(&format!("access-{tag}"), now + 100)
        .await
        .expect("写撤销");
    let revoked = store.load_access_revocations(now).await.expect("读撤销");
    assert!(revoked
        .iter()
        .any(|(hash, _)| hash == &format!("access-{tag}")));
    assert!(
        !store
            .load_access_revocations(now + 200)
            .await
            .expect("读过期撤销")
            .iter()
            .any(|(hash, _)| hash == &format!("access-{tag}")),
        "过期撤销不应回灌"
    );

    // 收尾，避免测试库累积行（失败也不影响其它断言）。
    let pool = sqlx::PgPool::connect(&pg).await.expect("清理连接失败");
    let _ = sqlx::query("DELETE FROM player_refresh_token WHERE player_id = $1")
        .bind(&first.player_id)
        .execute(&pool)
        .await;
    for phone in [&phone_a, &phone_b] {
        let _ = sqlx::query("DELETE FROM player_profile WHERE phone_hash = $1")
            .bind(phone)
            .execute(&pool)
            .await;
    }
    let _ = sqlx::query("DELETE FROM access_revocation WHERE token_hash = $1")
        .bind(format!("access-{tag}"))
        .execute(&pool)
        .await;
}
