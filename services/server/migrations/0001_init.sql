-- gf-server 初始化表（M2 切片④）
--
-- 字段口径以 design_draft.md §5.8 为准；时间一律用 epoch 毫秒 bigint，
-- 避免服务端与 PG 时区/精度差导致的断言口径分叉。
-- 本文件由 `Store::connect` 在启动时执行（幂等：全部 IF NOT EXISTS）。

CREATE TABLE IF NOT EXISTS player_profile (
    player_id     text PRIMARY KEY,
    phone_hash    text NOT NULL UNIQUE,
    phone_tail4   char(4) NOT NULL,
    nickname      text NOT NULL,
    created_at    bigint NOT NULL,
    status        text NOT NULL DEFAULT 'active'
);

CREATE TABLE IF NOT EXISTS player_stat (
    player_id       text NOT NULL,
    game_id         text NOT NULL,
    rating          integer NOT NULL DEFAULT 0,
    rating_games    integer NOT NULL DEFAULT 0,
    placement_left  integer NOT NULL DEFAULT 10,
    rank_level      integer NOT NULL DEFAULT 0,
    rounds_total    integer NOT NULL DEFAULT 0,
    wins            integer NOT NULL DEFAULT 0,
    updated_at      bigint NOT NULL,
    PRIMARY KEY (player_id, game_id)
);

CREATE TABLE IF NOT EXISTS match_record (
    match_id     text PRIMARY KEY,
    game_id      text NOT NULL,
    match_type   text NOT NULL,
    winner_team  smallint,
    end_reason   text,
    seats_count  smallint NOT NULL,
    started_at   bigint NOT NULL,
    ended_at     bigint
);

CREATE TABLE IF NOT EXISTS round_record (
    match_id            text NOT NULL,
    round_no            integer NOT NULL,
    players             jsonb NOT NULL,
    result              jsonb NOT NULL,
    levels_after        jsonb NOT NULL,
    ruleset_version     text NOT NULL,
    engine_version      text NOT NULL DEFAULT '',
    interpreter_version text NOT NULL DEFAULT '',
    started_at          bigint NOT NULL DEFAULT 0,
    ended_at            bigint NOT NULL,
    PRIMARY KEY (match_id, round_no)
);

CREATE TABLE IF NOT EXISTS settlement_event (
    settlement_id text PRIMARY KEY,
    match_id      text NOT NULL,
    round_no      integer NOT NULL,
    created_at    bigint NOT NULL
);

CREATE TABLE IF NOT EXISTS settlement_dead_letter (
    settlement_id text PRIMARY KEY,
    match_id      text NOT NULL,
    round_no      integer NOT NULL,
    payload       jsonb NOT NULL,
    failed_at     bigint NOT NULL,
    retries       integer NOT NULL DEFAULT 0,
    resolved_at   bigint
);

CREATE TABLE IF NOT EXISTS settlement_compensation (
    compensation_id text PRIMARY KEY,
    match_id        text NOT NULL,
    reason          text NOT NULL,
    issued          boolean NOT NULL DEFAULT false,
    created_at      bigint NOT NULL
);
