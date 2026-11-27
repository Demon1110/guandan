//! 持久化：PostgreSQL（战绩/账号）与 Redis（房间快照、动作流、匹配队列镜像）。
//!
//! 设计口径见 §5.8（表结构）与 §6.5（Redis 三类键）。本切片的取舍：
//! - PG/Redis 连接串从环境变量给（`GF_DATABASE_URL` / `GF_REDIS_URL`），缺省即关闭；
//!   关闭时全部方法退化为 no-op，联机对局不受影响（不因存储不可用而拒客）。
//! - 写失败不冒泡（不阻塞对局），但结算/场次/快照均带短退避重试；结算三次仍失败
//!   转 `settlement_dead_letter`，不再只打日志丢数据。
//! - 匹配队列以内存为准，Redis 里是镜像（`match:queue:{gameId}` zset），
//!   多实例共享队列要等网关粘性路由落地后再把内存态整个换掉。

use crate::config::{DatabaseConfig, RedisConfig};
use gf_engine::contract::Action;
use gf_table_runtime::ActionRecord;
use redis::aio::MultiplexedConnection;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use crate::admin::{AdminAudit, AdminMatchRow, AdminStats, GameSetting};
use crate::auth::Profile;

/// 房间快照的兜底 TTL（配置缺省时用；设计口径 §6.5 是 60 秒）。
pub const SNAPSHOT_TTL_MS: u64 = 60_000;

/// 初始化表结构（幂等）。文件随二进制编译进来，部署不额外拷 SQL。
const SCHEMA_SQL: &str = concat!(
    include_str!("../migrations/0001_init.sql"),
    "\n",
    include_str!("../migrations/0002_auth.sql"),
    "\n",
    include_str!("../migrations/0003_admin.sql")
);

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
    snapshot_ttl_ms: u64,
    actions_max_len: u64,
}

impl Store {
    /// 不落库（单测与本地联调默认走这条）。
    pub fn disabled() -> Self {
        Self {
            pg: None,
            redis: None,
            snapshot_ttl_ms: SNAPSHOT_TTL_MS,
            actions_max_len: 4_096,
        }
    }

    /// 按配置文件里的 `[database]` / `[redis]` 建连；任一失败只降级该半边。
    pub async fn connect_with(db: &DatabaseConfig, cache: &RedisConfig) -> Self {
        let pg = match db.url.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(url) => {
                let options = PgPoolOptions::new()
                    .max_connections(db.max_connections.max(1))
                    .acquire_timeout(std::time::Duration::from_millis(
                        db.connect_timeout_ms.max(100),
                    ));
                match options.connect(url).await {
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
                }
            }
            None => None,
        };
        let redis = match cache
            .url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
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
        Self {
            pg,
            redis,
            snapshot_ttl_ms: cache.snapshot_ttl_ms,
            actions_max_len: cache.actions_max_len,
        }
    }

    /// 只用连接串建连（测试/旧调用点），其余参数取默认值。
    pub async fn connect(pg_url: Option<&str>, redis_url: Option<&str>) -> Self {
        let db = DatabaseConfig {
            url: pg_url.map(str::to_owned),
            ..DatabaseConfig::default()
        };
        let cache = RedisConfig {
            url: redis_url.map(str::to_owned),
            ..RedisConfig::default()
        };
        Self::connect_with(&db, &cache).await
    }

    pub fn enabled(&self) -> bool {
        self.pg.is_some() || self.redis.is_some()
    }

    pub fn redis_enabled(&self) -> bool {
        self.redis.is_some()
    }

    /// PG 是否可用；账号持久化只在 PG 在场时启用。
    pub fn pg_enabled(&self) -> bool {
        self.pg.is_some()
    }

    /// 给账号缓存做启动恢复用；连接句柄 clone 一次即可。
    pub fn pg_pool(&self) -> Option<PgPool> {
        self.pg.clone()
    }

    /// 账号建档（`player_profile`）。保留给旧调用点；新登录路径走
    /// [`Self::find_or_create_profile`]，返回值带 `status` 且失败会上抛。
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

    /// 按手机号哈希查档案（不含创建）。
    pub async fn find_profile_by_phone(
        &self,
        phone_hash: &str,
    ) -> Result<Option<Profile>, sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok(None);
        };
        let row = sqlx::query(
            "SELECT player_id, phone_hash, phone_tail4, nickname, created_at, status \
             FROM player_profile WHERE phone_hash = $1",
        )
        .bind(phone_hash)
        .fetch_optional(pg)
        .await?;
        Ok(row.as_ref().map(profile_from_row))
    }

    /// 登录用「有则返回、无则建档」：手机号哈希唯一，并发首登只有一行会活下来。
    pub async fn find_or_create_profile(
        &self,
        phone_hash: &str,
        phone_tail4: &str,
        now_ms: u64,
        new_player_id: &str,
    ) -> Result<Profile, sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Err(sqlx::Error::RowNotFound);
        };
        let row = sqlx::query(
            "INSERT INTO player_profile \
             (player_id, phone_hash, phone_tail4, nickname, created_at, status) \
             VALUES ($1, $2, $3, $4, $5, 'active') \
             ON CONFLICT (phone_hash) DO UPDATE SET phone_tail4 = EXCLUDED.phone_tail4 \
             RETURNING player_id, phone_hash, phone_tail4, nickname, created_at, status",
        )
        .bind(new_player_id)
        .bind(phone_hash)
        .bind(phone_tail4)
        .bind(format!("掼友{phone_tail4}"))
        .bind(now_ms as i64)
        .fetch_one(pg)
        .await?;
        Ok(profile_from_row(&row))
    }

    /// 显式注册用「只新建」：手机号已存在时返回 `Ok(None)`，由上层映射冲突。
    pub async fn create_profile(
        &self,
        phone_hash: &str,
        phone_tail4: &str,
        now_ms: u64,
        new_player_id: &str,
    ) -> Result<Option<Profile>, sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok(None);
        };
        let row = sqlx::query(
            "INSERT INTO player_profile \
             (player_id, phone_hash, phone_tail4, nickname, created_at, status) \
             VALUES ($1, $2, $3, $4, $5, 'active') \
             ON CONFLICT (phone_hash) DO NOTHING \
             RETURNING player_id, phone_hash, phone_tail4, nickname, created_at, status",
        )
        .bind(new_player_id)
        .bind(phone_hash)
        .bind(phone_tail4)
        .bind(format!("掼友{phone_tail4}"))
        .bind(now_ms as i64)
        .fetch_optional(pg)
        .await?;
        Ok(row.as_ref().map(profile_from_row))
    }

    /// 启动恢复：一次性把全部档案灌进 `Auth` 内存索引。
    pub async fn load_profiles(&self) -> Result<Vec<Profile>, sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok(Vec::new());
        };
        let rows = sqlx::query(
            "SELECT player_id, phone_hash, phone_tail4, nickname, created_at, status \
             FROM player_profile",
        )
        .fetch_all(pg)
        .await?;
        Ok(rows.iter().map(profile_from_row).collect())
    }

    /// 新会话的 refresh token（只存哈希）。
    pub async fn insert_refresh_token(
        &self,
        token_hash: &str,
        player_id: &str,
        expires_at_ms: u64,
        created_at_ms: u64,
    ) -> Result<(), sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok(());
        };
        sqlx::query(
            "INSERT INTO player_refresh_token \
             (token_hash, player_id, expires_at, created_at) VALUES ($1, $2, $3, $4)",
        )
        .bind(token_hash)
        .bind(player_id)
        .bind(expires_at_ms as i64)
        .bind(created_at_ms as i64)
        .execute(pg)
        .await?;
        Ok(())
    }

    /// 登出/轮换时删一条 refresh；不存在也不报错。
    pub async fn delete_refresh_token(&self, token_hash: &str) -> Result<(), sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok(());
        };
        sqlx::query("DELETE FROM player_refresh_token WHERE token_hash = $1")
            .bind(token_hash)
            .execute(pg)
            .await?;
        Ok(())
    }

    /// refresh 轮换事务：旧行必须还在且未过期，删旧 + 插新同事务提交。
    ///
    /// 返回 `RowNotFound` 表示旧 refresh 已不在服务端权威表里（过期、
    /// 已被别的端轮换或登出），调用方按 `AUTH_INVALID` 处理。
    pub async fn rotate_refresh_token(
        &self,
        old_hash: &str,
        new_hash: &str,
        player_id: &str,
        expires_at_ms: u64,
        now_ms: u64,
    ) -> Result<(), sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok(());
        };
        let mut tx = pg.begin().await?;
        let deleted = sqlx::query(
            "DELETE FROM player_refresh_token \
             WHERE token_hash = $1 AND expires_at > $2",
        )
        .bind(old_hash)
        .bind(now_ms as i64)
        .execute(&mut *tx)
        .await?;
        if deleted.rows_affected() == 0 {
            tx.rollback().await?;
            return Err(sqlx::Error::RowNotFound);
        }
        sqlx::query(
            "INSERT INTO player_refresh_token \
             (token_hash, player_id, expires_at, created_at) VALUES ($1, $2, $3, $4)",
        )
        .bind(new_hash)
        .bind(player_id)
        .bind(expires_at_ms as i64)
        .bind(now_ms as i64)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// 启动恢复：只加载未过期 refresh。
    pub async fn load_refresh_tokens(
        &self,
        now_ms: u64,
    ) -> Result<Vec<(String, String, u64)>, sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok(Vec::new());
        };
        let rows = sqlx::query(
            "SELECT token_hash, player_id, expires_at FROM player_refresh_token \
             WHERE expires_at > $1",
        )
        .bind(now_ms as i64)
        .fetch_all(pg)
        .await?;
        Ok(rows
            .iter()
            .map(|row| {
                (
                    row.get::<String, _>("token_hash"),
                    row.get::<String, _>("player_id"),
                    row.get::<i64, _>("expires_at") as u64,
                )
            })
            .collect())
    }

    /// 登出撤销 access：落库后重启仍生效。
    pub async fn insert_access_revocation(
        &self,
        token_hash: &str,
        expires_at_ms: u64,
    ) -> Result<(), sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok(());
        };
        sqlx::query(
            "INSERT INTO access_revocation (token_hash, expires_at) VALUES ($1, $2) \
             ON CONFLICT (token_hash) DO UPDATE SET expires_at = EXCLUDED.expires_at",
        )
        .bind(token_hash)
        .bind(expires_at_ms as i64)
        .execute(pg)
        .await?;
        Ok(())
    }

    /// 启动恢复：只加载未过期撤销项。
    pub async fn load_access_revocations(
        &self,
        now_ms: u64,
    ) -> Result<Vec<(String, u64)>, sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok(Vec::new());
        };
        let rows = sqlx::query(
            "SELECT token_hash, expires_at FROM access_revocation WHERE expires_at > $1",
        )
        .bind(now_ms as i64)
        .fetch_all(pg)
        .await?;
        Ok(rows
            .iter()
            .map(|row| {
                (
                    row.get::<String, _>("token_hash"),
                    row.get::<i64, _>("expires_at") as u64,
                )
            })
            .collect())
    }

    pub async fn find_profile_by_player_id(
        &self,
        player_id: &str,
    ) -> Result<Option<Profile>, sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok(None);
        };
        let row = sqlx::query(
            "SELECT player_id, phone_hash, phone_tail4, nickname, created_at, status \
             FROM player_profile WHERE player_id = $1",
        )
        .bind(player_id)
        .fetch_optional(pg)
        .await?;
        Ok(row.as_ref().map(profile_from_row))
    }

    /// 后台用户列表：关键词匹配玩家 ID / 昵称 / 尾号，状态精确过滤。
    pub async fn admin_list_users(
        &self,
        keyword: &str,
        status: &str,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Profile>, i64), sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok((Vec::new(), 0));
        };
        const FILTER: &str = "($1 = '' OR player_id ILIKE '%' || $1 || '%' \
             OR nickname ILIKE '%' || $1 || '%' OR phone_tail4::text LIKE '%' || $1 || '%') \
             AND ($2 = '' OR status = $2)";
        let rows = sqlx::query(&format!(
            "SELECT player_id, phone_hash, phone_tail4, nickname, created_at, status \
             FROM player_profile WHERE {FILTER} \
             ORDER BY created_at DESC LIMIT $3 OFFSET $4"
        ))
        .bind(keyword)
        .bind(status)
        .bind(limit)
        .bind(offset)
        .fetch_all(pg)
        .await?;
        let total: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM player_profile WHERE {FILTER}"
        ))
        .bind(keyword)
        .bind(status)
        .fetch_one(pg)
        .await?;
        Ok((rows.iter().map(profile_from_row).collect(), total))
    }

    pub async fn update_profile_status(
        &self,
        player_id: &str,
        status: &str,
    ) -> Result<bool, sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok(false);
        };
        let result = sqlx::query("UPDATE player_profile SET status = $2 WHERE player_id = $1")
            .bind(player_id)
            .bind(status)
            .execute(pg)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// 后台概览统计；`since_ms` 用于「近 24h」。
    pub async fn admin_stats(&self, since_ms: u64) -> Result<AdminStats, sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok(AdminStats::default());
        };
        let row = sqlx::query(
            "SELECT \
             (SELECT count(*) FROM player_profile) AS users_total, \
             (SELECT count(*) FROM player_profile WHERE status = 'active') AS users_active, \
             (SELECT count(*) FROM player_profile WHERE status <> 'active') AS users_disabled, \
             (SELECT count(*) FROM player_profile WHERE created_at >= $1) AS users_recent, \
             (SELECT count(*) FROM match_record) AS matches_total, \
             (SELECT count(*) FROM match_record WHERE started_at >= $1) AS matches_recent, \
             (SELECT count(*) FROM round_record) AS rounds_total",
        )
        .bind(since_ms as i64)
        .fetch_one(pg)
        .await?;
        Ok(AdminStats {
            users_total: row.get::<i64, _>("users_total"),
            users_active: row.get::<i64, _>("users_active"),
            users_disabled: row.get::<i64, _>("users_disabled"),
            users_recent: row.get::<i64, _>("users_recent"),
            matches_total: row.get::<i64, _>("matches_total"),
            matches_recent: row.get::<i64, _>("matches_recent"),
            rounds_total: row.get::<i64, _>("rounds_total"),
        })
    }

    /// 后台对局列表，按开始时间倒序。
    pub async fn admin_list_matches(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<AdminMatchRow>, i64), sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok((Vec::new(), 0));
        };
        let rows = sqlx::query(
            "SELECT match_id, game_id, match_type, winner_team, end_reason, seats_count, \
                    started_at, ended_at \
             FROM match_record ORDER BY started_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pg)
        .await?;
        let total: i64 = sqlx::query_scalar("SELECT count(*) FROM match_record")
            .fetch_one(pg)
            .await?;
        let items = rows
            .iter()
            .map(|row| AdminMatchRow {
                match_id: row.get("match_id"),
                game_id: row.get("game_id"),
                match_type: row.get("match_type"),
                winner_team: row.get("winner_team"),
                end_reason: row.get("end_reason"),
                seats_count: row.get("seats_count"),
                started_at_ms: row.get::<i64, _>("started_at") as u64,
                ended_at_ms: row.get::<Option<i64>, _>("ended_at").map(|v| v as u64),
            })
            .collect();
        Ok((items, total))
    }

    pub async fn load_game_setting(&self) -> Result<Option<GameSetting>, sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok(None);
        };
        let row = sqlx::query(
            "SELECT game_id, display_name, enabled, modes::text AS modes_text, \
                    next_deal_ms, updated_at \
             FROM game_setting WHERE game_id = $1",
        )
        .bind(crate::admin::DEFAULT_GAME_ID)
        .fetch_optional(pg)
        .await?;
        Ok(row.map(|row| {
            let modes_text: String = row.get("modes_text");
            GameSetting {
                game_id: row.get("game_id"),
                display_name: row.get("display_name"),
                enabled: row.get("enabled"),
                modes: serde_json::from_str(&modes_text).unwrap_or_else(|_| {
                    vec!["casual".to_owned(), "ranked".to_owned(), "mixed".to_owned()]
                }),
                next_deal_ms: row.get::<i64, _>("next_deal_ms") as u64,
                updated_at_ms: row.get::<i64, _>("updated_at") as u64,
            }
        }))
    }

    pub async fn upsert_game_setting(&self, setting: &GameSetting) -> Result<(), sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok(());
        };
        let modes = serde_json::to_string(&setting.modes).unwrap_or_else(|_| "[]".to_owned());
        sqlx::query(
            "INSERT INTO game_setting \
             (game_id, display_name, enabled, modes, next_deal_ms, updated_at) \
             VALUES ($1, $2, $3, $4::text::jsonb, $5, $6) \
             ON CONFLICT (game_id) DO UPDATE SET \
               display_name = EXCLUDED.display_name, enabled = EXCLUDED.enabled, \
               modes = EXCLUDED.modes, next_deal_ms = EXCLUDED.next_deal_ms, \
               updated_at = EXCLUDED.updated_at",
        )
        .bind(&setting.game_id)
        .bind(&setting.display_name)
        .bind(setting.enabled)
        .bind(&modes)
        .bind(setting.next_deal_ms as i64)
        .bind(setting.updated_at_ms as i64)
        .execute(pg)
        .await?;
        Ok(())
    }

    pub async fn insert_admin_audit(&self, audit: &AdminAudit) -> Result<(), sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok(());
        };
        let detail = audit.detail.to_string();
        sqlx::query(
            "INSERT INTO admin_audit (audit_id, action, target, detail, actor, created_at) \
             VALUES ($1, $2, $3, $4::text::jsonb, $5, $6) \
             ON CONFLICT (audit_id) DO NOTHING",
        )
        .bind(&audit.audit_id)
        .bind(&audit.action)
        .bind(&audit.target)
        .bind(&detail)
        .bind(&audit.actor)
        .bind(audit.created_at_ms as i64)
        .execute(pg)
        .await?;
        Ok(())
    }

    pub async fn load_admin_audits(&self, limit: i64) -> Result<Vec<AdminAudit>, sqlx::Error> {
        let Some(pg) = &self.pg else {
            return Ok(Vec::new());
        };
        let rows = sqlx::query(
            "SELECT audit_id, action, target, detail::text AS detail_text, actor, created_at \
             FROM admin_audit ORDER BY created_at DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(pg)
        .await?;
        Ok(rows
            .iter()
            .map(|row| {
                let detail_text: String = row.get("detail_text");
                AdminAudit {
                    audit_id: row.get("audit_id"),
                    action: row.get("action"),
                    target: row.get("target"),
                    actor: row.get("actor"),
                    detail: serde_json::from_str(&detail_text)
                        .unwrap_or_else(|_| serde_json::json!({})),
                    created_at_ms: row.get::<i64, _>("created_at") as u64,
                }
            })
            .collect())
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
        for attempt in 1..=3u32 {
            match sqlx::query(sql)
                .bind(match_id)
                .bind(winner_team.map(i16::from))
                .bind(end_reason)
                .bind(ended_at_ms as i64)
                .execute(pg)
                .await
            {
                Ok(_) => return,
                Err(err) if attempt < 3 => {
                    eprintln!("[store] 更新 match_record 失败（第 {attempt}/3 次，将重试）：{err}");
                    tokio::time::sleep(std::time::Duration::from_millis(50 * u64::from(attempt)))
                        .await;
                }
                Err(err) => {
                    eprintln!("[store] 更新 match_record 失败（已用尽重试）：{err}");
                    return;
                }
            }
        }
    }

    /// 一局落库：`settlement_event` 先插（结算事件计数，§6.6），再写 `round_record`，
    /// 两条在同一事务里，避免「有事件无战绩」的单边状态。
    pub async fn insert_round(&self, row: &RoundRow) {
        let Some(pg) = &self.pg else { return };
        let mut last_err = None;
        for attempt in 1..=3u32 {
            match Self::write_round(pg, row).await {
                Ok(()) => return,
                Err(err) => {
                    eprintln!("[store] 写结算记录失败（第 {attempt}/3 次）：{err}");
                    last_err = Some(err);
                    if attempt < 3 {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            50 * u64::from(attempt),
                        ))
                        .await;
                    }
                }
            }
        }
        if let Err(err) = Self::dead_letter_round(pg, row).await {
            eprintln!(
                "[store] 死信写入失败（战绩将丢失，需要人工补偿）：{err}；原始错误：{}",
                last_err.map(|e| e.to_string()).unwrap_or_default()
            );
        }
    }

    async fn write_round(pg: &PgPool, row: &RoundRow) -> Result<(), sqlx::Error> {
        let settlement_id = settlement_id(&row.match_id, row.round_no);
        let players = players_json(&row.players);
        let levels = levels_json(&row.levels_after);
        let mut tx = pg.begin().await?;
        let event_sql = "INSERT INTO settlement_event \
             (settlement_id, match_id, round_no, created_at) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (settlement_id) DO NOTHING";
        let round_sql = "INSERT INTO round_record \
             (match_id, round_no, players, result, levels_after, ruleset_version, \
              engine_version, interpreter_version, started_at, ended_at) \
             VALUES ($1, $2, $3::text::jsonb, $4::text::jsonb, $5::text::jsonb, $6, $7, $8, $9, $10) \
             ON CONFLICT (match_id, round_no) DO NOTHING";
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
        tx.commit().await
    }

    async fn dead_letter_round(pg: &PgPool, row: &RoundRow) -> Result<(), sqlx::Error> {
        let settlement_id = settlement_id(&row.match_id, row.round_no);
        let payload = serde_json::json!({
            "matchId": row.match_id,
            "roundNo": row.round_no,
            "players": row.players,
            "ranks": row.ranks,
            "result": row.result_json,
            "levelsAfter": row.levels_after,
            "rulesetVersion": row.ruleset_version,
            "startedAt": row.started_at_ms,
            "endedAt": row.ended_at_ms,
        });
        let sql = "INSERT INTO settlement_dead_letter \
             (settlement_id, match_id, round_no, payload, failed_at, retries) \
             VALUES ($1, $2, $3, $4::text::jsonb, $5, 1) \
             ON CONFLICT (settlement_id) DO UPDATE SET \
              payload = EXCLUDED.payload, failed_at = EXCLUDED.failed_at, \
              retries = settlement_dead_letter.retries + 1, resolved_at = NULL";
        sqlx::query(sql)
            .bind(&settlement_id)
            .bind(&row.match_id)
            .bind(i32::from(row.round_no))
            .bind(payload.to_string())
            .bind(row.ended_at_ms as i64)
            .execute(pg)
            .await?;
        eprintln!("[store] 结算已转死信：{settlement_id}（等待补偿巡检）");
        Ok(())
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
                .arg(self.actions_max_len)
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

        for attempt in 1..=3u32 {
            let result = redis::cmd("SET")
                .arg(&key)
                .arg(snapshot_json)
                .arg("PX")
                .arg(self.snapshot_ttl_ms as i64)
                .query_async::<()>(&mut conn)
                .await;
            match result {
                Ok(()) => return,
                Err(err) if attempt < 3 => {
                    eprintln!("[store] 写房间快照失败（第 {attempt}/3 次，将重试）：{err}");
                    tokio::time::sleep(std::time::Duration::from_millis(50 * u64::from(attempt)))
                        .await;
                }
                Err(err) => {
                    eprintln!("[store] 写房间快照失败（已用尽重试）：{err}");
                    return;
                }
            }
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

fn profile_from_row(row: &sqlx::postgres::PgRow) -> Profile {
    Profile {
        player_id: row.get("player_id"),
        phone_hash: row.get("phone_hash"),
        phone_tail4: row.get("phone_tail4"),
        nickname: row.get("nickname"),
        created_at_ms: row.get::<i64, _>("created_at") as u64,
        status: row.get("status"),
    }
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
        Action::TributeReturn { card } => ("tribute-return", format!("{{\"card\":{card}}}")),
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
