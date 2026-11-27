-- 运营后台（M2 admin 骨架）
--
-- 只放后台需要、而玩家侧不关心的两张表：
-- - game_setting：运营可改的玩法开关/模式/续局间隔，建房与匹配读这份；
-- - admin_audit：后台写操作的审计流水，按时间倒序最多保留最近 500 条。
--
-- 时间口径同前：epoch 毫秒 bigint。

CREATE TABLE IF NOT EXISTS game_setting (
    game_id       text PRIMARY KEY,
    display_name  text NOT NULL,
    enabled       boolean NOT NULL DEFAULT true,
    modes         jsonb NOT NULL DEFAULT '["casual","ranked","mixed"]'::jsonb,
    next_deal_ms  bigint NOT NULL DEFAULT 5000,
    updated_at    bigint NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS admin_audit (
    audit_id    text PRIMARY KEY,
    action      text NOT NULL,
    target      text NOT NULL,
    detail      jsonb NOT NULL DEFAULT '{}'::jsonb,
    actor       text NOT NULL DEFAULT 'admin',
    created_at  bigint NOT NULL
);

CREATE INDEX IF NOT EXISTS admin_audit_created_idx
    ON admin_audit (created_at DESC);
