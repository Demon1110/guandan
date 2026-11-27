<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { ApiError, adminApi } from "../api";
import DataState from "../components/DataState.vue";
import { actionLabel, formatDateTime, formatRelative, modeLabel, nextDealLabel } from "../lib/format";
import type { AuditEntry } from "../types";

const items = ref<AuditEntry[] | null>(null);
const loading = ref(false);
const error = ref("");
const filter = ref<"all" | "user" | "game">("all");

const filtered = computed(() => {
  const list = items.value ?? [];
  if (filter.value === "all") return list;
  return list.filter((item) => item.action.startsWith(`${filter.value}.`));
});

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function statusText(value: unknown): string {
  if (value === "active") return "启用";
  if (value === "disabled") return "禁用";
  return String(value ?? "—");
}

function detailText(entry: AuditEntry): string {
  const detail = asRecord(entry.detail);
  if (entry.action === "user.status") {
    return `状态 ${statusText(detail.from)} → ${statusText(detail.to)}`;
  }
  if (entry.action === "game.setting") {
    const from = asRecord(detail.from);
    const to = asRecord(detail.to);
    const modes = Array.isArray(to.modes) ? to.modes.map((mode) => modeLabel(String(mode))).join("、") : "—";
    const delay = typeof to.nextDealMs === "number" ? nextDealLabel(to.nextDealMs) : "—";
    return `开启 ${from.enabled ? "是" : "否"} → ${to.enabled ? "是" : "否"}；模式 ${modes || "无"}；续局 ${delay}`;
  }
  return JSON.stringify(detail);
}

async function load() {
  loading.value = true;
  error.value = "";
  try {
    const response = await adminApi.audits();
    items.value = response.items;
  } catch (err) {
    error.value = err instanceof ApiError ? err.message : "审计日志加载失败。";
  } finally {
    loading.value = false;
  }
}

onMounted(load);
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <p class="eyebrow">AUDIT TRAIL</p>
        <h2>审计日志</h2>
        <p class="page-head__desc">后台写操作按时间倒序保留最近 100 条；接 PG 时同步落库，重启后恢复最近 200 条。</p>
      </div>
      <div class="page-head__actions">
        <span v-if="items" class="muted">{{ items.length }} 条记录</span>
        <button class="btn btn--ghost btn--sm" type="button" :disabled="loading" @click="load">
          {{ loading ? "刷新中…" : "刷新" }}
        </button>
      </div>
    </header>

    <div class="segmented segmented--inline" role="tablist" aria-label="按操作类型筛选">
      <button
        v-for="tab in [
          { value: 'all', label: '全部' },
          { value: 'user', label: '用户' },
          { value: 'game', label: '玩法' },
        ]"
        :key="tab.value"
        class="segmented__item"
        :class="{ 'segmented__item--active': filter === tab.value }"
        type="button"
        role="tab"
        :aria-selected="filter === tab.value"
        @click="filter = tab.value as 'all' | 'user' | 'game'"
      >
        {{ tab.label }}
      </button>
    </div>

    <DataState v-if="!items && loading" kind="loading" title="正在读取审计记录" />
    <DataState v-else-if="!items && error" kind="error" title="审计日志加载失败" :message="error">
      <button class="btn btn--primary btn--sm" type="button" @click="load">重新加载</button>
    </DataState>

    <template v-else-if="items">
      <div v-if="error" class="inline-alert inline-alert--error" role="alert">
        {{ error }}
        <button class="text-link" type="button" @click="load">重试</button>
      </div>

      <div v-if="filtered.length === 0" class="panel">
        <DataState
          kind="empty"
          title="没有对应的审计记录"
          message="用户启停或玩法配置保存后，这里会自动出现一条流水。"
        />
      </div>

      <ul v-else class="audit-list">
        <li v-for="entry in filtered" :key="entry.auditId" class="audit-item">
          <span class="audit-item__dot" :class="{ 'audit-item__dot--game': entry.action.startsWith('game.') }" />
          <div class="audit-item__body">
            <div class="audit-item__head">
              <strong>{{ actionLabel(entry.action) }}</strong>
              <span class="audit-item__target mono">{{ entry.target }}</span>
              <span class="audit-item__time">{{ formatRelative(entry.createdAtMs) }}</span>
            </div>
            <p class="audit-item__detail">{{ detailText(entry) }}</p>
            <p class="audit-item__meta">
              操作人 {{ entry.actor }} · {{ formatDateTime(entry.createdAtMs) }} · {{ entry.auditId }}
            </p>
          </div>
        </li>
      </ul>
    </template>
  </div>
</template>
