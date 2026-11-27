//! 服务端配置：默认值可跑本机联调；中间件与短信配置读 `config.toml`，
//! 真实值放本机 `config.local.toml`（已 gitignore），环境变量优先级最高。
//!
//! 读取顺序：内置默认值 → 配置文件 → 环境变量。配置文件路径：
//! 1. `GF_CONFIG_FILE` 指定；
//! 2. 工作目录下的 `config.local.toml`（存在即用）；
//! 3. 工作目录下的 `config.toml`（模板，缺省值也在里面）。
//!
//! 环境变量只覆盖「需要按部署环境变」的部分（绑定地址、密钥、连接串、短信凭据），
//! 其余走配置文件，避免几十个 `GF_*` 变量散在启动脚本里。

use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// 显式指定配置文件路径的环境变量。
pub const CONFIG_FILE_ENV: &str = "GF_CONFIG_FILE";

/// 服务端运行参数（不含规则集：规则集由房间建桌时锁定）。
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// 监听地址，如 `127.0.0.1:8080`。
    pub bind: String,
    /// 客户端版本下限（semver；空串表示不校验，本机联调默认如此）。
    pub min_client_version: String,
    /// 房间主循环节拍（毫秒）。20Hz 足够覆盖 30s 行动窗口与 90s 宽限的判定精度。
    pub tick_ms: u64,
    /// 结算后自动开下一局的间隔（毫秒）。
    ///
    /// TODO(M2-OPEN-01)：联机「再来一局」触发方未裁定（会动 `actions.kind` 闭集），
    /// 当前按服务端定时续局，待裁定后改为客户端触发。
    pub next_deal_ms: u64,
    /// 心跳超时：超过该时长无任何消息即判掉线（§5.9：客户端 15s 一次、30s 判掉线）。
    pub heartbeat_timeout_ms: u64,
    /// 房间码有效期（毫秒）。
    pub room_code_ttl_ms: u64,
    /// 匹配排队超时（毫秒），超时后轮询返回 `timeout`。
    pub match_timeout_ms: u64,
    /// 部署环境：`dev`（默认）或 `production`。生产环境拒绝开发默认密钥。
    pub env: String,
    /// 进程内房间上限（防循环建房把内存/CPU 打满）。
    pub max_rooms: usize,
    /// 已结束房间保留时长（回放导出窗口），过期后回收。
    pub room_retention_ms: u64,
    /// 未开局且无人在线的空房保留时长。
    pub room_idle_ms: u64,
    /// 手机号哈希盐（§5.8：单盐，轮换按 §9.7 五步；生产必须覆盖）。
    pub auth_salt: String,
    /// access/refresh token 的 HS256 签名密钥（§8.5：支持双密钥轮换）。
    pub jwt_secret: String,
    /// 运营后台独立令牌（`Authorization: Bearer`）。空串 = 后台接口关闭。
    pub admin_token: String,
    /// 是否强制所有玩家侧接口与 WS 入座都带 access token。
    pub require_auth: bool,
    /// PostgreSQL 连接与池参数。
    pub database: DatabaseConfig,
    /// Redis 连接与键参数。
    pub redis: RedisConfig,
    /// 短信通道与测试白名单。
    pub sms: SmsConfig,
}

/// PostgreSQL 连接参数。`url` 为空即不落库（纯内存跑）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatabaseConfig {
    pub url: Option<String>,
    /// 连接池上限。本机联调 10 够用；线上按 PG 的 `max_connections` 留余量。
    pub max_connections: u32,
    /// 建连超时（毫秒）。超时视为连不上，降级为不落库。
    pub connect_timeout_ms: u64,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: None,
            max_connections: 10,
            connect_timeout_ms: 5_000,
        }
    }
}

/// Redis 连接与键参数。`url` 为空即不写快照/动作流/队列镜像。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedisConfig {
    pub url: Option<String>,
    /// 房间快照 TTL（毫秒）。快照每 `snapshot_interval_ms` 刷一次，TTL 留两倍余量。
    pub snapshot_ttl_ms: u64,
    /// 快照刷新间隔（毫秒）。
    pub snapshot_interval_ms: u64,
    /// 动作流 `XADD MAXLEN ~` 的上限，防止单房间 Stream 无界增长。
    pub actions_max_len: u64,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: None,
            snapshot_ttl_ms: 60_000,
            snapshot_interval_ms: 30_000,
            actions_max_len: 4_096,
        }
    }
}

/// 短信通道参数。默认 `provider = "log"`：只把验证码打进服务端日志。
///
/// `provider = "yunpian"` 时走云片 [`single_send`]（`template_id = 0`，智能匹配已报备模板）
/// 或 [`tpl_single_send`]（指定模板 ID，`template_param` 形如 `#code#`）。
///
/// [`single_send`]: https://www.yunpian.com/official/document/sms/zh_CN/scene_smsverify
/// [`tpl_single_send`]: https://www.yunpian.com/official/document/sms/zh_cn/domestic_tpl_single_send
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmsConfig {
    /// `log`（默认，只打日志）或 `yunpian`。
    pub provider: String,
    /// 云片 apikey（32 位）；`provider = "yunpian"` 时必填。
    pub api_key: String,
    /// 短信签名，不带书名号（形如 `掼蛋`），发送时拼成 `【掼蛋】`。
    pub sign: String,
    /// 智能匹配模板模式的正文模板，`{code}` 会被替换成验证码。
    pub text_template: String,
    /// 指定模板模式的模板 ID；`0` 表示不用指定模板、走正文智能匹配。
    pub template_id: u64,
    /// 指定模板模式的变量名，默认 `#code#`。
    pub template_param: String,
    /// 云片服务地址（只填到域名，路径由发送器按模式拼）。
    pub endpoint: String,
    /// 单次发送超时（毫秒）。
    pub timeout_ms: u64,
    /// 真实发送总开关。`false` = 挡板：短信只打日志、绝不外发（默认，防误发）。
    pub send_enabled: bool,
    /// 允许真实接收短信的号码（规范化 11 位）。空表 = 一个人都不发。
    pub send_whitelist: Vec<String>,
    /// 短信通道未批前的测试白名单手机号（规范化 11 位；空表 = 口子关闭）。
    pub test_whitelist: Vec<String>,
    /// 白名单手机号使用的固定验证码。
    pub test_code: String,
}

impl Default for SmsConfig {
    fn default() -> Self {
        Self {
            provider: "log".to_owned(),
            api_key: String::new(),
            sign: "掼蛋".to_owned(),
            text_template: "您的验证码是{code}，5分钟内有效。".to_owned(),
            template_id: 0,
            template_param: "#code#".to_owned(),
            endpoint: "https://sms.yunpian.com".to_owned(),
            timeout_ms: 5_000,
            send_enabled: false,
            send_whitelist: Vec::new(),
            test_whitelist: Vec::new(),
            test_code: crate::auth::DEFAULT_TEST_CODE.to_owned(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8080".to_owned(),
            min_client_version: String::new(),
            tick_ms: 50,
            next_deal_ms: 5_000,
            heartbeat_timeout_ms: 30_000,
            room_code_ttl_ms: crate::lobby::DEFAULT_CODE_TTL_MS,
            match_timeout_ms: crate::lobby::DEFAULT_QUEUE_TIMEOUT_MS,
            env: "dev".to_owned(),
            max_rooms: 1_024,
            room_retention_ms: 10 * 60 * 1_000,
            room_idle_ms: 10 * 60 * 1_000,
            // 开发默认值只为本机联调能跑；生产未覆盖视为配置事故（`env=production` 时启动失败）。
            auth_salt: "gf-dev-salt-change-me".to_owned(),
            jwt_secret: "gf-dev-jwt-secret-change-me".to_owned(),
            // 本地联调默认开一个显式后台令牌；生产必须由 GF_ADMIN_TOKEN 注入。
            admin_token: "gf-dev-admin-token".to_owned(),
            require_auth: false,
            database: DatabaseConfig::default(),
            redis: RedisConfig::default(),
            sms: SmsConfig::default(),
        }
    }
}

/// 配置加载失败（文件读不到、TOML 语法错、必填项缺失）。
#[derive(Debug)]
pub struct ConfigError {
    path: Option<PathBuf>,
    detail: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.path {
            Some(path) => write!(f, "配置文件 {} 有误：{}", path.display(), self.detail),
            None => write!(f, "配置有误：{}", self.detail),
        }
    }
}

impl std::error::Error for ConfigError {}

impl ServerConfig {
    /// 按「默认值 → 配置文件 → 环境变量」加载，并在返回前做必填校验。
    pub fn load() -> Result<Self, ConfigError> {
        let mut cfg = Self::default();
        if let Some(path) = resolve_config_path() {
            let text = fs::read_to_string(&path).map_err(|err| ConfigError {
                path: Some(path.clone()),
                detail: format!("读取失败：{err}"),
            })?;
            cfg.apply_toml(&text).map_err(|err| ConfigError {
                path: Some(path),
                detail: err,
            })?;
        }
        cfg.apply_env();
        cfg.validate()?;
        Ok(cfg)
    }

    /// 解析一份 TOML（不读文件，便于单测）。
    pub fn apply_toml(&mut self, text: &str) -> Result<(), String> {
        let file: FileConfig =
            toml::from_str(text).map_err(|err| format!("TOML 解析失败：{err}"))?;
        if let Some(v) = file.server.bind {
            self.bind = v;
        }
        if let Some(v) = file.server.min_client_version {
            self.min_client_version = v;
        }
        if let Some(v) = file.server.tick_ms {
            self.tick_ms = v.max(5);
        }
        if let Some(v) = file.server.next_deal_ms {
            self.next_deal_ms = v;
        }
        if let Some(v) = file.server.heartbeat_timeout_ms {
            self.heartbeat_timeout_ms = v.max(1_000);
        }
        if let Some(v) = file.server.room_code_ttl_ms {
            self.room_code_ttl_ms = v.max(60_000);
        }
        if let Some(v) = file.server.match_timeout_ms {
            self.match_timeout_ms = v.max(1_000);
        }
        if let Some(v) = file.server.env {
            self.env = v.trim().to_ascii_lowercase();
        }
        if let Some(v) = file.server.max_rooms {
            self.max_rooms = v.max(1);
        }
        if let Some(v) = file.server.room_retention_ms {
            self.room_retention_ms = v.max(30_000);
        }
        if let Some(v) = file.server.room_idle_ms {
            self.room_idle_ms = v.max(30_000);
        }
        if let Some(v) = file.server.require_auth {
            self.require_auth = v;
        }
        if let Some(v) = file.security.auth_salt {
            self.auth_salt = v;
        }
        if let Some(v) = file.security.jwt_secret {
            self.jwt_secret = v;
        }
        if let Some(v) = file.security.admin_token {
            self.admin_token = v.trim().to_owned();
        }
        if let Some(v) = file.database.url {
            self.database.url = non_empty(v);
        }
        if let Some(v) = file.database.max_connections {
            self.database.max_connections = v.max(1);
        }
        if let Some(v) = file.database.connect_timeout_ms {
            self.database.connect_timeout_ms = v.max(100);
        }
        if let Some(v) = file.redis.url {
            self.redis.url = non_empty(v);
        }
        if let Some(v) = file.redis.snapshot_ttl_ms {
            self.redis.snapshot_ttl_ms = v.max(1_000);
        }
        if let Some(v) = file.redis.snapshot_interval_ms {
            self.redis.snapshot_interval_ms = v.max(1_000);
        }
        if let Some(v) = file.redis.actions_max_len {
            self.redis.actions_max_len = v.max(1);
        }
        if let Some(v) = file.sms.provider {
            self.sms.provider = v.trim().to_ascii_lowercase();
        }
        if let Some(v) = file.sms.api_key {
            self.sms.api_key = v.trim().to_owned();
        }
        if let Some(v) = file.sms.sign {
            self.sms.sign = v.trim().to_owned();
        }
        if let Some(v) = file.sms.text_template {
            self.sms.text_template = v;
        }
        if let Some(v) = file.sms.template_id {
            self.sms.template_id = v;
        }
        if let Some(v) = file.sms.template_param {
            self.sms.template_param = v.trim().to_owned();
        }
        if let Some(v) = file.sms.endpoint {
            let endpoint = v.trim().trim_end_matches('/').to_owned();
            if !endpoint.is_empty() {
                self.sms.endpoint = endpoint;
            }
        }
        if let Some(v) = file.sms.timeout_ms {
            self.sms.timeout_ms = v.max(100);
        }
        if let Some(v) = file.sms.send_enabled {
            self.sms.send_enabled = v;
        }
        if let Some(v) = file.sms.send_whitelist {
            self.sms.send_whitelist = v;
        }
        if let Some(v) = file.sms.test_whitelist {
            self.sms.test_whitelist = v;
        }
        if let Some(v) = file.sms.test_code {
            if !v.trim().is_empty() {
                self.sms.test_code = v.trim().to_owned();
            }
        }
        Ok(())
    }

    /// 环境变量覆盖（部署侧只改这些；其余走配置文件）。
    ///
    /// - `GF_BIND`、`GF_MIN_CLIENT_VERSION`、`GF_TICK_MS`、`GF_NEXT_DEAL_MS`
    /// - `GF_HEARTBEAT_TIMEOUT_MS`、`GF_ROOM_CODE_TTL_MS`、`GF_MATCH_TIMEOUT_MS`
    /// - `GF_AUTH_SALT` / `GF_JWT_SECRET`
    /// - `GF_ADMIN_TOKEN`（运营后台令牌；留空关闭 `/api/admin/*`）
    /// - `GF_REQUIRE_AUTH`
    /// - `GF_DATABASE_URL` / `GF_REDIS_URL`
    /// - `GF_SMS_PROVIDER` / `GF_SMS_API_KEY`
    /// - `GF_SMS_TEST_WHITELIST` / `GF_SMS_TEST_CODE`
    pub fn apply_env(&mut self) {
        if let Ok(v) = env::var("GF_BIND") {
            self.bind = v;
        }
        if let Ok(v) = env::var("GF_MIN_CLIENT_VERSION") {
            self.min_client_version = v;
        }
        if let Some(v) = env_u64("GF_TICK_MS") {
            self.tick_ms = v.max(5);
        }
        if let Some(v) = env_u64("GF_NEXT_DEAL_MS") {
            self.next_deal_ms = v;
        }
        if let Some(v) = env_u64("GF_HEARTBEAT_TIMEOUT_MS") {
            self.heartbeat_timeout_ms = v.max(1_000);
        }
        if let Some(v) = env_u64("GF_ROOM_CODE_TTL_MS") {
            self.room_code_ttl_ms = v.max(60_000);
        }
        if let Some(v) = env_u64("GF_MATCH_TIMEOUT_MS") {
            self.match_timeout_ms = v.max(1_000);
        }
        if let Ok(v) = env::var("GF_ENV") {
            self.env = v.trim().to_ascii_lowercase();
        }
        if let Ok(v) = env::var("GF_MAX_ROOMS") {
            if let Ok(parsed) = v.trim().parse::<usize>() {
                self.max_rooms = parsed.max(1);
            }
        }
        if let Ok(v) = env::var("GF_AUTH_SALT") {
            self.auth_salt = v;
        }
        if let Ok(v) = env::var("GF_JWT_SECRET") {
            self.jwt_secret = v;
        }
        if let Ok(v) = env::var("GF_ADMIN_TOKEN") {
            self.admin_token = v.trim().to_owned();
        }
        if let Ok(v) = env::var("GF_REQUIRE_AUTH") {
            self.require_auth = matches!(v.trim(), "1" | "true" | "TRUE" | "True" | "yes");
        }
        if let Ok(v) = env::var("GF_DATABASE_URL") {
            self.database.url = non_empty(v);
        }
        if let Ok(v) = env::var("GF_REDIS_URL") {
            self.redis.url = non_empty(v);
        }
        if let Ok(v) = env::var("GF_SMS_PROVIDER") {
            self.sms.provider = v.trim().to_ascii_lowercase();
        }
        if let Ok(v) = env::var("GF_SMS_API_KEY") {
            self.sms.api_key = v.trim().to_owned();
        }
        if let Ok(v) = env::var("GF_SMS_SEND_ENABLED") {
            self.sms.send_enabled = matches!(v.trim(), "1" | "true" | "TRUE" | "True" | "yes");
        }
        if let Ok(v) = env::var("GF_SMS_TEST_WHITELIST") {
            self.sms.test_whitelist = v
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
        }
        if let Ok(v) = env::var("GF_SMS_TEST_CODE") {
            if !v.trim().is_empty() {
                self.sms.test_code = v.trim().to_owned();
            }
        }
    }

    /// 必填校验：短信选了云片却没凭据，宁可启动失败，也别静默退回日志发码。
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.env == "production" && self.uses_dev_secrets() {
            return Err(ConfigError {
                path: None,
                detail: "[security] env=production 但仍在使用开发默认 auth_salt / jwt_secret；\
                         必须由 GF_AUTH_SALT / GF_JWT_SECRET 注入 ≥32 字节随机密钥"
                    .to_owned(),
            });
        }
        if self.env == "production" && (self.auth_salt.len() < 32 || self.jwt_secret.len() < 32) {
            return Err(ConfigError {
                path: None,
                detail: "[security] env=production 要求 auth_salt / jwt_secret 各至少 32 字节"
                    .to_owned(),
            });
        }
        if self.env == "production" && !self.sms.test_whitelist.is_empty() {
            return Err(ConfigError {
                path: None,
                detail: "[security] env=production 不允许 sms.test_whitelist 非空；\
                         白名单固定码只用于内测，生产必须走真实短信 + 先发码再登录"
                    .to_owned(),
            });
        }
        match self.sms.provider.as_str() {
            "log" => Ok(()),
            "yunpian" => {
                if self.sms.api_key.is_empty() {
                    return Err(ConfigError {
                        path: None,
                        detail: "sms.provider = \"yunpian\" 但 sms.api_key 为空".to_owned(),
                    });
                }
                if self.sms.sign.trim().is_empty() {
                    return Err(ConfigError {
                        path: None,
                        detail: "sms.provider = \"yunpian\" 但 sms.sign 为空".to_owned(),
                    });
                }
                Ok(())
            }
            other => Err(ConfigError {
                path: None,
                detail: format!("sms.provider 只支持 \"log\" / \"yunpian\"，收到 \"{other}\""),
            }),
        }
    }

    /// 是否还在用开发默认密钥（生产启动时应告警）。
    pub fn uses_dev_secrets(&self) -> bool {
        self.auth_salt == "gf-dev-salt-change-me"
            || self.jwt_secret == "gf-dev-jwt-secret-change-me"
    }

    /// 启动时打一行「哪些中间件是开的」，只报开关与地址，不打印任何密钥。
    pub fn summary(&self) -> String {
        let pg = if self.database.url.is_some() {
            "on"
        } else {
            "off"
        };
        let redis = if self.redis.url.is_some() {
            "on"
        } else {
            "off"
        };
        // 短信必须让人一眼看出是真发还是挡板，避免上线后误以为在发。
        let sms = if self.sms.provider == "yunpian" && self.sms.send_enabled {
            "yunpian(真发)"
        } else if self.sms.provider == "yunpian" {
            "yunpian(挡板)"
        } else {
            self.sms.provider.as_str()
        };
        let admin = if self.admin_token.trim().is_empty() {
            "off"
        } else {
            "on"
        };
        format!(
            "bind={} require_auth={} postgres={pg} redis={redis} sms={sms} admin={admin}",
            self.bind, self.require_auth
        )
    }

    /// 客户端版本是否满足下限（§5.16）。下限为空表示不校验。
    pub fn accepts_version(&self, client_version: &str) -> bool {
        if self.min_client_version.trim().is_empty() {
            return true;
        }
        match (
            parse_version(&self.min_client_version),
            parse_version(client_version),
        ) {
            (Some(min), Some(v)) => v >= min,
            // 下限写了但客户端版本解析不出来：按 CLIENT_TOO_OLD 处理（失败闭合）。
            _ => false,
        }
    }
}

/// 找配置文件：环境变量指定 → `config.local.toml` → `config.toml`。
pub fn resolve_config_path() -> Option<PathBuf> {
    if let Ok(path) = env::var(CONFIG_FILE_ENV) {
        let path = PathBuf::from(path.trim());
        if !path.as_os_str().is_empty() {
            // 显式指定了却不存在：交给调用方报错，不要静默回落。
            return Some(path);
        }
    }
    for name in ["config.local.toml", "config.toml"] {
        let path = Path::new(name).to_path_buf();
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn env_u64(name: &str) -> Option<u64> {
    env::var(name).ok()?.trim().parse::<u64>().ok()
}

// ---------------------------------------------------------------------------
// TOML 结构与运行时结构分开放：文件里每一段都是「可选字段」，缺省项才回落到
// 默认值，避免「文件里漏写一行就把线上参数清成空」。
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileConfig {
    server: ServerSection,
    security: SecuritySection,
    database: DatabaseSection,
    redis: RedisSection,
    sms: SmsSection,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ServerSection {
    bind: Option<String>,
    min_client_version: Option<String>,
    tick_ms: Option<u64>,
    next_deal_ms: Option<u64>,
    heartbeat_timeout_ms: Option<u64>,
    room_code_ttl_ms: Option<u64>,
    match_timeout_ms: Option<u64>,
    env: Option<String>,
    max_rooms: Option<usize>,
    room_retention_ms: Option<u64>,
    room_idle_ms: Option<u64>,
    require_auth: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SecuritySection {
    auth_salt: Option<String>,
    jwt_secret: Option<String>,
    admin_token: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct DatabaseSection {
    url: Option<String>,
    max_connections: Option<u32>,
    connect_timeout_ms: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RedisSection {
    url: Option<String>,
    snapshot_ttl_ms: Option<u64>,
    snapshot_interval_ms: Option<u64>,
    actions_max_len: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SmsSection {
    provider: Option<String>,
    api_key: Option<String>,
    sign: Option<String>,
    text_template: Option<String>,
    template_id: Option<u64>,
    template_param: Option<String>,
    endpoint: Option<String>,
    timeout_ms: Option<u64>,
    send_enabled: Option<bool>,
    send_whitelist: Option<Vec<String>>,
    test_whitelist: Option<Vec<String>>,
    test_code: Option<String>,
}

/// 解析 `MAJOR.MINOR.PATCH`；缺段按 0 补齐，多段只取前三段。
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.trim().split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_refuses_dev_secrets_and_short_keys() {
        let mut cfg = ServerConfig {
            env: "production".to_owned(),
            ..ServerConfig::default()
        };
        assert!(cfg.validate().is_err(), "生产 + 开发默认密钥必须拒绝启动");

        cfg.auth_salt = "short".to_owned();
        cfg.jwt_secret = "x".repeat(32);
        assert!(cfg.validate().is_err(), "生产密钥不足 32 字节必须拒绝");

        cfg.auth_salt = "a".repeat(32);
        cfg.jwt_secret = "b".repeat(32);
        assert!(cfg.validate().is_ok(), "≥32 字节的自定义密钥应放行");

        cfg.sms.test_whitelist = vec!["13800008000".to_owned()];
        assert!(cfg.validate().is_err(), "生产不允许测试白名单");
    }

    #[test]
    fn version_gate_defaults_to_open() {
        let cfg = ServerConfig::default();
        assert!(cfg.accepts_version(""));
        assert!(cfg.accepts_version("0.0.1"));
    }

    #[test]
    fn version_gate_orders_by_numbers_not_lexicographic() {
        let cfg = ServerConfig {
            min_client_version: "1.10.0".to_owned(),
            ..ServerConfig::default()
        };
        assert!(cfg.accepts_version("1.10.0"));
        assert!(cfg.accepts_version("1.11.2"));
        assert!(!cfg.accepts_version("1.9.9"), "字典序会误判 1.9 > 1.10");
        assert!(
            !cfg.accepts_version("not-a-version"),
            "解析失败必须失败闭合"
        );
    }

    #[test]
    fn version_parser_tolerates_short_and_suffixed_forms() {
        assert_eq!(parse_version("2"), Some((2, 0, 0)));
        assert_eq!(parse_version("2.3"), Some((2, 3, 0)));
        assert_eq!(parse_version("2.3.4-beta.1"), Some((2, 3, 4)));
    }

    const SAMPLE: &str = r##"
[server]
bind = "0.0.0.0:9000"
tick_ms = 40
require_auth = true

[security]
auth_salt = "salt-from-file"
jwt_secret = "jwt-from-file"

[database]
url = "postgres://gf:gf@db:5432/gf"
max_connections = 20

[redis]
url = "redis://cache:6379"
snapshot_ttl_ms = 90000
actions_max_len = 8192

[sms]
provider = "yunpian"
api_key = "k"
sign = "掼蛋"
template_id = 376861
template_param = "#code#"
test_whitelist = ["13800008000"]
test_code = "654321"
"##;

    #[test]
    fn toml_sections_override_defaults() {
        let mut cfg = ServerConfig::default();
        cfg.apply_toml(SAMPLE).expect("样例配置应能解析");
        assert_eq!(cfg.bind, "0.0.0.0:9000");
        assert_eq!(cfg.tick_ms, 40);
        assert!(cfg.require_auth);
        assert_eq!(cfg.auth_salt, "salt-from-file");
        assert_eq!(
            cfg.database.url.as_deref(),
            Some("postgres://gf:gf@db:5432/gf")
        );
        assert_eq!(cfg.database.max_connections, 20);
        assert_eq!(cfg.redis.snapshot_ttl_ms, 90_000);
        assert_eq!(cfg.redis.snapshot_interval_ms, 30_000, "没写的项保留默认");
        assert_eq!(cfg.sms.provider, "yunpian");
        assert_eq!(cfg.sms.template_id, 376_861);
        assert_eq!(cfg.sms.test_whitelist, vec!["13800008000".to_owned()]);
        cfg.validate().expect("凭据齐全应能通过校验");
    }

    #[test]
    fn empty_urls_mean_disabled_middleware() {
        let mut cfg = ServerConfig::default();
        cfg.apply_toml("[database]\nurl = \"  \"\n[redis]\nurl = \"\"\n")
            .expect("空串应能解析");
        assert!(cfg.database.url.is_none());
        assert!(cfg.redis.url.is_none());
        assert!(cfg.summary().contains("postgres=off"));
        assert!(cfg.summary().contains("redis=off"));
    }

    #[test]
    fn yunpian_without_api_key_fails_fast() {
        let mut cfg = ServerConfig::default();
        cfg.apply_toml("[sms]\nprovider = \"yunpian\"\n")
            .expect("解析不报错");
        let err = cfg.validate().expect_err("缺 apikey 必须失败");
        assert!(err.to_string().contains("api_key"));
    }

    #[test]
    fn unknown_provider_is_rejected() {
        let mut cfg = ServerConfig::default();
        cfg.apply_toml("[sms]\nprovider = \"aliyun\"\n")
            .expect("解析不报错");
        assert!(cfg.validate().is_err(), "接错供应商要当场报错");
    }

    #[test]
    fn unknown_key_is_rejected_to_catch_typos() {
        let mut cfg = ServerConfig::default();
        let err = cfg
            .apply_toml("[database]\nurll = \"postgres://x\"\n")
            .expect_err("拼错的键不能静默忽略");
        assert!(err.contains("urll") || err.contains("unknown field"));
    }

    #[test]
    fn shipped_config_template_stays_parseable() {
        // 模板是运维照着改的，改坏它等于把服务起不来，必须锁在测试里。
        let mut cfg = ServerConfig::default();
        cfg.apply_toml(include_str!("../config.toml"))
            .expect("config.toml 必须能解析");
        cfg.validate().expect("模板默认值必须合法");
        assert_eq!(cfg.sms.provider, "log", "模板不能默认接真实短信");
        assert!(cfg.database.url.is_none(), "模板不能带真实连接串");
        assert!(cfg.redis.url.is_none());
    }
}
