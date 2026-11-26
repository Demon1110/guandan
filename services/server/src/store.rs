//! 持久化：PostgreSQL（战绩/账号）与 Redis（房间快照、动作流、匹配队列镜像）。
//!
//! 设计口径见 §5.8（表结构）与 §6.5（Redis 三类键）。本切片的取舍：
//! - PG/Redis 连接串从环境变量给（`GF_DATABASE_URL` / `GF_REDIS_URL`），缺省即关闭；
//!   关闭时全部方法退化为 no-op，联机对局不受影响（不因存储不可用而拒客）。
//! - 写失败只打日志、不冒泡：结算的三次重试与死信表属 §6.6，排在 M3 的
//!   `settlement_dead_letter` 巡检里；本切片保证「表在、字段对、事务边界明确」。
//! - 匹配队列以内存为准，Redis 里是镜像（`match:queue:{gameId}` zset），
//!   多实例共享队列要等网关粘性路由落地后再把内存态整个换掉。

use gf_engine::contract::Action;
use gf_table_runtime::ActionRecord;
use redis::aio::MultiplexedConnection;
use sqlx::PgPool;

use crate::auth::Profile;

/// 房间快照在 Redis 的存活时间（设计：每 30 秒刷一次）。
pub const SNAPSHOT_TTL_MS: u64 = 60_000;

/// 初始化表结构（幂等）。文件随二进制编译进来，部署不额外拷 SQL。
const SCHEMA_SQL: &str = include_str!("../migrations/0001_init.sql");

/// 一局的落库行（§5.8 `round_record`）。
#[derive(Clone, Debug)]
pub struct RoundRow {
    pub match_id: String,
    pub round_no: u16,
    /// `(seat, player_id, rank)`；`player_id` 为 `None` 表示该座位是匿名入座。
    pub players: Vec<(u8, Option<String>, u8)>,
    /// 名次顺序的座位表（`SettleSummary.ranks`），用于回填 `players`。
    pub ranks: Vec<u8>,
    /// `SettleResult.result` 的 canonical JSON 原样入库。
    pub result_json: String,
    pub levels_after: Vec<u8>,
    pub ruleset_version: String,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
}

/// 存档事件：房间把「不可丢的事实」投给持久化任务，避免在房间锁里 await 数据库。
#[derive(Clone, Debug)]
pub enum StorageEvent {
    RoundSettled {
        match_id: String,
        row: RoundRow,
        /// 本局动作（`ActionRecord` 同构，写入 Redis Stream）。
        actions: Vec<ActionRecord>,
    },
    MatchEnded {
        match_id: String,
        winner_team: Option<u8>,
        end_reason: &'static str,
        ended_at_ms: u64,
    },
}

/// 存储句柄；PG 与 Redis 各自可独立缺席。
pub struct Store {
    pg: Option<PgPool>,
    redis: Option<MultiplexedConnection>,
}

impl Store {
    /// 不落库（单测与本地联调默认走这条）。
    pub fn disabled() -> Self {
        Self {
            pg: None,
            redis: None,
        }
    }

    /// 按连接串建连；任一失败都只降级该半边，不阻塞启动。
    pub async fn connect(pg_url: Option<&str>, redis_url: Option<&str>) -> Self {
        let pg = match pg_url.map(str::trim).filter(|s| !s.is_empty()) {
            Some(url) => match PgPool::connect(url).await {
                Ok(pool) => {
                    if let Err(err) = apply_schema(&pool).await {
                        eprintln!("[store] 建表失败（已降级为不落库）：{err}");
                        None
                    } else {
                        println!("[store] PostgreSQL 已连接");
                        Some(pool)
                    }
                }
                Err(err) => {
                    eprintln!("[store] PostgreSQL 连接失败（已降级为不落库）：{err}");
                    None
                }
            },
            None => None,
        };
        let redis = match redis_url.map(str::trim).filter(|s| !s.is_empty()) {
            Some(url) => match redis::Client::open(url) {
                Ok(client) => match client.get_multiplexed_tokio_connection().await {
                    Ok(conn) => {
                        println!("[store] Redis 已连接");
                        Some(conn)
                    }
                    Err(err) => {
                        eprintln!("[store] Redis 连接失败（已降级为不写缓存）：{err}");
                        None
                    }
                },
                Err(err) => {
                    eprintln!("[store] Redis 连接串非法（已降级为不写缓存）：{err}");
                    None
                }
            },
            None => None,
        };
        Self { pg, redis }
    }

    pub fn enabled(&self) -> bool {
        self.pg.is_some() || self.redis.is_some()
    }

    /// 账号建档（`player_profile`）。`phone_hash` 唯一，重复登录只更新展示字段。
    pub async fn upsert_profile(&self, profile: &Profile) {
        let Some(pg) = &self.pg else { return };
        let sql = "INSERT INTO player_profile \
             (player_id, phone_hash, phone_tail4, nickname, created_at, status) \
             VALUES ($1, $2, $3, $4, $5, 'active') \
             ON CONFLICT (phone_hash) DO UPDATE SET phone_tail4 = EXCLUDED.phone_tail4";
        if let Err(err) = sqlx::query(sql)
            .bind(&profile.player_id)
            .bind(&profile.phone_hash)
            .bind(&profile.phone_tail4)
            .bind(&profile.nickname)
            .bind(profile.created_at_ms as i64)
            .execute(pg)
            .await
        {
            eprintln!("[store] 写 player_profile 失败：{err}");
        }
    }

    /// 场次开始（`match_record`）。
    pub async fn insert_match(
        &self,
        match_id: &str,
        game_id: &str,
        match_type: &str,
        seats_count: u8,
        started_at_ms: u64,
    ) {
        let Some(pg) = &self.pg else { return };
        let sql = "INSERT INTO match_record \
             (match_id, game_id, match_type, seats_count, started_at) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (match_id) DO NOTHING";
        if let Err(err) = sqlx::query(sql)
            .bind(match_id)
            .bind(game_id)
            .bind(match_type)
            .bind(i16::from(seats_count))
            .bind(started_at_ms as i64)
            .execute(pg)
            .await
        {
            eprintln!("[store] 写 match_record 失败：{err}");
        }
    }

    /// 场次终止（过 A / 安全上限 / 中止）。
    pub async fn finish_match(
        &self,
        match_id: &str,
        winner_team: Option<u8>,
        end_reason: &str,
        ended_at_ms: u64,
    ) {
        let Some(pg) = &self.pg else { return };
        let sql = "UPDATE match_record \
             SET winner_team = $2, end_reason = $3, ended_at = $4 \
             WHERE match_id = $1";
        if let Err(err) = sqlx::query(sql)
            .bind(match_id)
            .bind(winner_team.map(i16::from))
            .bind(end_reason)
            .bind(ended_at_ms as i64)
            .execute(pg)
            .await
        {
            eprintln!("[store] 更新 match_record 失败：{err}");
        }
    }

    /// 一局落库：`settlement_event` 先插（结算事件计数，§6.6），再写 `round_record`，
    /// 两条在同一事务里，避免「有事件无战绩」的单边状态。
    pub async fn insert_round(&self, row: &RoundRow) {
        let Some(pg) = &self.pg else { return };
        let settlement_id = settlement_id(&row.match_id, row.round_no);
        let players = players_json(&row.players);
        let levels = levels_json(&row.levels_after);
        let mut tx = match pg.begin().await {
            Ok(tx) => tx,
            Err(err) => {
                eprintln!("[store] 开事务失败：{err}");
                return;
            }
        };
        let event_sql = "INSERT INTO settlement_event \
             (settlement_id, match_id, round_no, created_at) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (settlement_id) DO NOTHING";
        let round_sql = "INSERT INTO round_record \
             (match_id, round_no, players, result, levels_after, ruleset_version, \
              engine_version, interpreter_version, started_at, ended_at) \
             VALUES ($1, $2, to_jsonb($3::text), to_jsonb($4::text), to_jsonb($5::text), $6, $7, $8, $9, $10) \
             ON CONFLICT (match_id, round_no) DO NOTHING";
        let result = async {
            sqlx::query(event_sql)
                .bind(&settlement_id)
                .bind(&row.match_id)
                .bind(i32::from(row.round_no))
                .bind(row.ended_at_ms as i64)
                .execute(&mut *tx)
                .await?;
            sqlx::query(round_sql)
                .bind(&row.match_id)
                .bind(i32::from(row.round_no))
                .bind(players)
                .bind(&row.result_json)
                .bind(levels)
                .bind(&row.ruleset_version)
                .bind("")
                .bind("")
                .bind(row.started_at_ms as i64)
                .bind(row.ended_at_ms as i64)
                .execute(&mut *tx)
                .await?;
            Ok::<_, sqlx::Error>(())
        }
        .await;
        match result {
            Ok(()) => {
                if let Err(err) = tx.commit().await {
                    eprintln!("[store] 结算事务提交失败：{err}");
                }
            }
            Err(err) => eprintln!("[store] 写结算记录失败（已回滚）：{err}"),
        }
    }

    /// 动作流落 Redis（`room:{matchId}:actions` Stream，§6.5）。
    pub async fn append_actions(&self, match_id: &str, lines: &[String]) {
        let Some(mut conn) = self.redis.clone() else {
            return;
        };
        let key = actions_key(match_id);

        for line in lines {
            let result = redis::cmd("XADD")
                .arg(&key)
                .arg("MAXLEN")
                .arg("~")
                .arg(4096)
                .arg("*")
                .arg("line")
                .arg(line)
                .query_async::<()>(&mut conn)
                .await;
            if let Err(err) = result {
                eprintln!("[store] 写动作流失败：{err}");
                return;
            }
        }
    }

    /// 房间快照（`room:{matchId}:snapshot`，带 TTL）。
    pub async fn put_snapshot(&self, match_id: &str, snapshot_json: &str) {
        let Some(mut conn) = self.redis.clone() else {
            return;
        };
        let key = snapshot_key(match_id);

        let result = redis::cmd("SET")
            .arg(&key)
            .arg(snapshot_json)
            .arg("PX")
            .arg(SNAPSHOT_TTL_MS as i64)
            .query_async::<()>(&mut conn)
            .await;
        if let Err(err) = result {
            eprintln!("[store] 写房间快照失败：{err}");
        }
    }

    /// 匹配队列镜像：入队/刷新分数（`match:queue:{gameId}` zset）。
    pub async fn queue_upsert(&self, game_id: &str, mode: &str, ticket: &str, now_ms: u64) {
        let Some(mut conn) = self.redis.clone() else {
            return;
        };
        let key = queue_key(game_id);
        let member = format!("{mode}:{ticket}");

        let result = redis::cmd("ZADD")
            .arg(&key)
            .arg(now_ms as i64)
            .arg(&member)
            .query_async::<()>(&mut conn)
            .await;
        if let Err(err) = result {
            eprintln!("[store] 写匹配队列失败：{err}");
        }
    }

    /// 出队（取消 / 已撮合）。
    pub async fn queue_remove(&self, game_id: &str, mode: &str, ticket: &str) {
        let Some(mut conn) = self.redis.clone() else {
            return;
        };
        let key = queue_key(game_id);
        let member = format!("{mode}:{ticket}");

        let result = redis::cmd("ZREM")
            .arg(&key)
            .arg(&member)
            .query_async::<()>(&mut conn)
            .await;
        if let Err(err) = result {
            eprintln!("[store] 清匹配队列失败：{err}");
        }
    }
}

async fn apply_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    for statement in SCHEMA_SQL.split(';') {
        let statement = statement.trim();
        // 只有注释的片段（文件头的说明）直接跳过；带注释的正文交给 PG 自己吃掉。
        let comment_only = statement
            .lines()
            .all(|line| line.trim().is_empty() || line.trim_start().starts_with("--"));
        if statement.is_empty() || comment_only {
            continue;
        }
        sqlx::query(statement).execute(pool).await?;
    }
    Ok(())
}

/// `settlement_event.settlement_id`：`{matchId}:{roundNo}` 足够唯一且可人工核对。
pub fn settlement_id(match_id: &str, round_no: u16) -> String {
    format!("{match_id}:{round_no}")
}

/// `round_record.players`：`[{seat, player_id, rank}]`，未登录座位 `player_id` 为 null。
pub fn players_json(players: &[(u8, Option<String>, u8)]) -> String {
    let items: Vec<String> = players
        .iter()
        .map(|(seat, player_id, rank)| match player_id {
            Some(id) => format!(
                "{{\"seat\":{seat},\"player_id\":{},\"rank\":{rank}}}",
                serde_json::Value::String(id.clone())
            ),
            None => format!("{{\"seat\":{seat},\"player_id\":null,\"rank\":{rank}}}"),
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// `levels_after`：按队伍序号升序的 u8 数组（§5.8）。
pub fn levels_json(levels: &[u8]) -> String {
    let items: Vec<String> = levels.iter().map(u8::to_string).collect();
    format!("[{}]", items.join(","))
}

/// 动作流一行：字段与回放包 `actions.jsonl` 同构（`tsMs` 用房间虚拟时钟）。
pub fn action_line(match_id: &str, record: &ActionRecord) -> String {
    let (kind, payload) = match &record.action {
        Action::Play { cards } => {
            let cards: Vec<String> = cards.iter().map(u16::to_string).collect();
            ("play", format!("{{\"cards\":[{}]}}", cards.join(",")))
        }
        Action::Pass => ("pass", "{}".to_owned()),
        Action::TributeReturn { card } => (
            "tribute-return",
            format!("{{\"card\":{card}}}"),
        ),
    };
    format!(
        "{{\"matchId\":{},\"seatSeq\":{},\"roomSeq\":{},\"roundNo\":{},\"seat\":{},\
         \"requestId\":\"{}\",\"kind\":\"{kind}\",\"payload\":{payload},\"tsMs\":{}}}",
        serde_json::Value::String(match_id.to_owned()),
        record.seat_seq,
        record.room_seq,
        record.round_no,
        record.seat,
        record.request_id,
        record.ts_ms
    )
}

pub fn snapshot_key(match_id: &str) -> String {
    format!("room:{match_id}:snapshot")
}

pub fn actions_key(match_id: &str) -> String {
    format!("room:{match_id}:actions")
}

pub fn queue_key(game_id: &str) -> String {
    format!("match:queue:{game_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(room_seq: u64, action: Action) -> ActionRecord {
        ActionRecord {
            seat_seq: 3,
            room_seq,
            round_no: 2,
            seat: 1,
            request_id: 42,
            action,
            ts_ms: 1234,
        }
    }

    #[test]
    fn settlement_id_is_match_plus_round() {
        assert_eq!(settlement_id("m-1", 3), "m-1:3");
    }

    #[test]
    fn players_json_keeps_null_for_anonymous_seat() {
        let json = players_json(&[(0, Some("p-1".to_owned()), 1), (2, None, 4)]);
        assert_eq!(
            json,
            r#"[{"seat":0,"player_id":"p-1","rank":1},{"seat":2,"player_id":null,"rank":4}]"#
        );
        assert!(serde_json::from_str::<serde_json::Value>(&json).is_ok());
    }

    #[test]
    fn action_line_matches_replay_package_shape() {
        let line = action_line("m-1", &record(9, Action::Play { cards: vec![3, 7] }));
        let value: serde_json::Value = serde_json::from_str(&line).expect("必须是合法 JSON");
        assert_eq!(value["matchId"], "m-1");
        assert_eq!(value["roomSeq"], 9);
        assert_eq!(value["roundNo"], 2);
        assert_eq!(value["seat"], 1);
        assert_eq!(value["requestId"], "42");
        assert_eq!(value["kind"], "play");
        assert_eq!(value["payload"]["cards"], serde_json::json!([3, 7]));
        assert_eq!(value["tsMs"], 1234);

        let pass = action_line("m-1", &record(10, Action::Pass));
        let value: serde_json::Value = serde_json::from_str(&pass).unwrap();
        assert_eq!(value["kind"], "pass");
        assert_eq!(value["payload"], serde_json::json!({}));
    }

    #[test]
    fn redis_keys_follow_design() {
        assert_eq!(snapshot_key("m-9"), "room:m-9:snapshot");
        assert_eq!(actions_key("m-9"), "room:m-9:actions");
        assert_eq!(queue_key("guandan"), "match:queue:guandan");
        assert_eq!(levels_json(&[2, 5]), "[2,5]");
    }

    #[test]
    fn disabled_store_is_noop() {
        let store = Store::disabled();
        assert!(!store.enabled());
    }
}
