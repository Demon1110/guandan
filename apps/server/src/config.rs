//! 服务端配置：默认值可跑本机联调，生产由环境变量覆盖。

use std::env;

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
    /// 手机号哈希盐（§5.8：单盐，轮换按 §9.7 五步；生产必须由环境变量给）。
    pub auth_salt: String,
    /// access/refresh token 的 HS256 签名密钥（§8.5：支持双密钥轮换）。
    pub jwt_secret: String,
    /// 短信通道未批前的测试白名单手机号（规范化 11 位；空表 = 口子关闭）。
    pub sms_test_whitelist: Vec<String>,
    /// 白名单手机号使用的固定验证码。
    pub sms_test_code: String,
    /// 是否强制所有玩家侧接口与 WS 入座都带 access token。
    pub require_auth: bool,
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
            // 开发默认值只为本机联调能跑；生产未覆盖视为配置事故（启动时会告警）。
            auth_salt: "gf-dev-salt-change-me".to_owned(),
            jwt_secret: "gf-dev-jwt-secret-change-me".to_owned(),
            sms_test_whitelist: Vec::new(),
            sms_test_code: crate::auth::DEFAULT_TEST_CODE.to_owned(),
            require_auth: false,
        }
    }
}

impl ServerConfig {
    /// 从环境变量读取（缺省回落默认值）。
    ///
    /// - `GF_BIND`
    /// - `GF_MIN_CLIENT_VERSION`
    /// - `GF_TICK_MS`
    /// - `GF_NEXT_DEAL_MS`
    /// - `GF_HEARTBEAT_TIMEOUT_MS`
    /// - `GF_ROOM_CODE_TTL_MS`
    /// - `GF_MATCH_TIMEOUT_MS`
    /// - `GF_AUTH_SALT` / `GF_JWT_SECRET`（生产必填，缺省值是开发用固定串）
    /// - `GF_SMS_TEST_WHITELIST`（逗号分隔的 11 位手机号）
    /// - `GF_SMS_TEST_CODE`
    /// - `GF_REQUIRE_AUTH`（`1`/`true` 强制玩家侧接口与 WS 带 token）
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(v) = env::var("GF_BIND") {
            cfg.bind = v;
        }
        if let Ok(v) = env::var("GF_MIN_CLIENT_VERSION") {
            cfg.min_client_version = v;
        }
        if let Some(v) = env_u64("GF_TICK_MS") {
            cfg.tick_ms = v.max(5);
        }
        if let Some(v) = env_u64("GF_NEXT_DEAL_MS") {
            cfg.next_deal_ms = v;
        }
        if let Some(v) = env_u64("GF_HEARTBEAT_TIMEOUT_MS") {
            cfg.heartbeat_timeout_ms = v.max(1_000);
        }
        if let Some(v) = env_u64("GF_ROOM_CODE_TTL_MS") {
            cfg.room_code_ttl_ms = v.max(60_000);
        }
        if let Some(v) = env_u64("GF_MATCH_TIMEOUT_MS") {
            cfg.match_timeout_ms = v.max(1_000);
        }
        if let Ok(v) = env::var("GF_AUTH_SALT") {
            cfg.auth_salt = v;
        }
        if let Ok(v) = env::var("GF_JWT_SECRET") {
            cfg.jwt_secret = v;
        }
        if let Ok(v) = env::var("GF_SMS_TEST_WHITELIST") {
            cfg.sms_test_whitelist = v
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
        }
        if let Ok(v) = env::var("GF_SMS_TEST_CODE") {
            if !v.trim().is_empty() {
                cfg.sms_test_code = v.trim().to_owned();
            }
        }
        if let Ok(v) = env::var("GF_REQUIRE_AUTH") {
            cfg.require_auth = matches!(v.trim(), "1" | "true" | "TRUE" | "True" | "yes");
        }
        cfg
    }

    /// 是否还在用开发默认密钥（生产启动时应告警）。
    pub fn uses_dev_secrets(&self) -> bool {
        self.auth_salt == "gf-dev-salt-change-me" || self.jwt_secret == "gf-dev-jwt-secret-change-me"
    }

    /// 客户端版本是否满足下限（§5.16）。下限为空表示不校验。
    pub fn accepts_version(&self, client_version: &str) -> bool {
        if self.min_client_version.trim().is_empty() {
            return true;
        }
        match (parse_version(&self.min_client_version), parse_version(client_version)) {
            (Some(min), Some(v)) => v >= min,
            // 下限写了但客户端版本解析不出来：按 CLIENT_TOO_OLD 处理（失败闭合）。
            _ => false,
        }
    }
}

fn env_u64(name: &str) -> Option<u64> {
    env::var(name).ok()?.trim().parse::<u64>().ok()
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
        assert!(!cfg.accepts_version("not-a-version"), "解析失败必须失败闭合");
    }

    #[test]
    fn version_parser_tolerates_short_and_suffixed_forms() {
        assert_eq!(parse_version("2"), Some((2, 0, 0)));
        assert_eq!(parse_version("2.3"), Some((2, 3, 0)));
        assert_eq!(parse_version("2.3.4-beta.1"), Some((2, 3, 4)));
    }
}
