-- 账号会话持久化（M2 账号链路）
--
-- 账号本身落 `player_profile`；这里只补可以跨进程重启的会话状态：
-- - refresh token：只存 SHA-256，轮换时旧行删除；
-- - access token 撤销表：登出后必须跨重启继续生效，按过期时间清尾。
--
-- 时间一律 epoch 毫秒 bigint，与 0001_init.sql 同口径。
-- 本文件由 `Store::connect` 在启动时执行（幂等：全部 IF NOT EXISTS）。

-- 账号状态：早期库可能没这一列（0001 已带），这里按 IF NOT EXISTS 兜底补齐。
ALTER TABLE player_profile
    ADD COLUMN IF NOT EXISTS status text NOT NULL DEFAULT 'active';

CREATE TABLE IF NOT EXISTS player_refresh_token (
    token_hash  text PRIMARY KEY,
    player_id   text NOT NULL,
    expires_at  bigint NOT NULL,
    created_at  bigint NOT NULL
);

CREATE INDEX IF NOT EXISTS player_refresh_token_player_idx
    ON player_refresh_token (player_id);

CREATE INDEX IF NOT EXISTS player_refresh_token_expiry_idx
    ON player_refresh_token (expires_at);

CREATE TABLE IF NOT EXISTS access_revocation (
    token_hash  text PRIMARY KEY,
    expires_at  bigint NOT NULL
);

CREATE INDEX IF NOT EXISTS access_revocation_expiry_idx
    ON access_revocation (expires_at);
