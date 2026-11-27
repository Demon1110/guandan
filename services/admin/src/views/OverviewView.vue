<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { RouterLink } from "vue-router";
import { ApiError, adminApi } from "../api";
import DataState from "../components/DataState.vue";
import StatusPill from "../components/StatusPill.vue";
import { formatDuration, formatNumber, formatRelative, modeLabel, nextDealLabel } from "../lib/format";
import type { Overview } from "../types";

const data = ref<Overview | null>(null);
const loading = ref(false);
const error = ref("");

async function load() {
  loading.value = true;
  error.value = "";
  try {
    data.value = await adminApi.overview();
  } catch (err) {
    error.value = err instanceof ApiError ? err.message : "总览加载失败。";
  } finally {
    loading.value = false;
  }
}

const gameModes = computed(() => data.value?.game.modes.map(modeLabel).join(" · ") ?? "—");

onMounted(load);
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <p class="eyebrow">OVERVIEW</p>
        <h2>运行总览</h2>
        <p class="page-head__desc">账号规模、对局量与玩法状态一屏看完；数据每 24 小时滚动统计。</p>
      </div>
      <div class="page-head__actions">
        <span v-if="data" class="muted">更新于 {{ formatRelative(data.generatedAtMs) }}</span>
        <button class="btn btn--ghost btn--sm" type="button" :disabled="loading" @click="load">
          {{ loading ? "刷新中…" : "刷新数据" }}
        </button>
      </div>
    </header>

    <DataState
      v-if="!data && loading"
      kind="loading"
      title="正在读取运营数据"
      message="首次打开会并行拉取账号、对局与玩法状态。"
    />
    <DataState
      v-else-if="!data && error"
      kind="error"
      title="暂时读不到总览"
      :message="error"
    >
      <button class="btn btn--primary btn--sm" type="button" @click="load">重新加载</button>
    </DataState>

    <template v-else-if="data">
      <section class="metric-grid">
        <article class="metric-card">
          <p class="metric-card__label">账号总数</p>
          <p class="metric-card__value">{{ formatNumber(data.users.total) }}</p>
          <p class="metric-card__note">
            <span class="accent-text">{{ formatNumber(data.users.active) }}</span> 个启用 ·
            {{ formatNumber(data.users.disabled) }} 个禁用
          </p>
        </article>
        <article class="metric-card">
          <p class="metric-card__label">近 24h 新增</p>
          <p class="metric-card__value">{{ formatNumber(data.users.recent24h) }}</p>
          <p class="metric-card__note">新注册账号，按建档时间统计</p>
        </article>
        <article class="metric-card">
          <p class="metric-card__label">近 24h 对局</p>
          <p class="metric-card__value">{{ formatNumber(data.matches.recent24h) }}</p>
          <p class="metric-card__note">累计 {{ formatNumber(data.matches.total) }} 场 · {{ formatNumber(data.matches.rounds) }} 副</p>
        </article>
        <article class="metric-card metric-card--accent">
          <p class="metric-card__label">在线房间</p>
          <p class="metric-card__value">{{ formatNumber(data.matches.activeRooms) }}</p>
          <p class="metric-card__note">进程内尚未回收的房间</p>
        </article>
      </section>

      <section class="split-grid">
        <article class="panel">
          <header class="panel__head">
            <div>
              <p class="eyebrow">RUNTIME</p>
              <h3>系统与存储</h3>
            </div>
            <span class="muted">运行 {{ formatDuration(data.uptimeMs) }}</span>
          </header>
          <dl class="kv-list">
            <div class="kv-row">
              <dt>玩法开关</dt>
              <dd>
                <StatusPill
                  :tone="data.game.enabled ? 'on' : 'off'"
                  :label="data.game.enabled ? '运行中' : '已关闭'"
                  dot
                />
              </dd>
            </div>
            <div class="kv-row">
              <dt>PostgreSQL</dt>
              <dd>
                <StatusPill
                  :tone="data.storage.postgres ? 'on' : 'neutral'"
                  :label="data.storage.postgres ? '已接入' : '未配置'"
                  dot
                />
              </dd>
            </div>
            <div class="kv-row">
              <dt>Redis</dt>
              <dd>
                <StatusPill
                  :tone="data.storage.redis ? 'on' : 'neutral'"
                  :label="data.storage.redis ? '已接入' : '未配置'"
                  dot
                />
              </dd>
            </div>
            <div class="kv-row">
              <dt>玩家在线口径</dt>
              <dd class="kv-row__text">活跃账号 {{ formatNumber(data.users.active) }} / 总账号 {{ formatNumber(data.users.total) }}</dd>
            </div>
          </dl>
          <p class="panel__footnote">
            未接 PostgreSQL 时，用户列表走内存档案、对局记录为空；配置改动只保留在当前进程。
          </p>
        </article>

        <article class="panel">
          <header class="panel__head">
            <div>
              <p class="eyebrow">GAME</p>
              <h3>当前玩法</h3>
            </div>
            <RouterLink class="text-link" to="/games">去配置 →</RouterLink>
          </header>
          <div class="game-banner">
            <div class="game-banner__mark">掼</div>
            <div>
              <p class="game-banner__name">{{ data.game.displayName }}</p>
              <p class="muted">{{ data.game.gameId }} · {{ gameModes || "未开放模式" }}</p>
            </div>
          </div>
          <dl class="kv-list">
            <div class="kv-row">
              <dt>结算续局</dt>
              <dd class="kv-row__text">{{ nextDealLabel(data.game.nextDealMs) }}</dd>
            </div>
            <div class="kv-row">
              <dt>开放模式</dt>
              <dd class="kv-row__text">{{ gameModes || "—" }}</dd>
            </div>
          </dl>
          <p class="panel__footnote">续局间隔只作用于之后新建的房间，已在跑的房间保留原节奏。</p>
        </article>
      </section>

      <section class="quick-grid">
        <RouterLink class="quick-card" to="/users">
          <span class="quick-card__index">01</span>
          <div>
            <strong>巡检账号</strong>
            <p>按玩家 ID、昵称或手机尾号检索，启用 / 禁用即时生效。</p>
          </div>
          <span class="quick-card__arrow">→</span>
        </RouterLink>
        <RouterLink class="quick-card" to="/matches">
          <span class="quick-card__index">02</span>
          <div>
            <strong>查对局</strong>
            <p>按开始时间倒序查看已结束场次、胜方与结束原因。</p>
          </div>
          <span class="quick-card__arrow">→</span>
        </RouterLink>
        <RouterLink class="quick-card" to="/audits">
          <span class="quick-card__index">03</span>
          <div>
            <strong>查审计</strong>
            <p>所有后台写操作留痕，含变更前后的关键字段。</p>
          </div>
          <span class="quick-card__arrow">→</span>
        </RouterLink>
      </section>
    </template>
  </div>
</template>
