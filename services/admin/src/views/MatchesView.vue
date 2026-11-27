<script setup lang="ts">
import { onMounted, ref } from "vue";
import { ApiError, adminApi } from "../api";
import DataState from "../components/DataState.vue";
import PagerBar from "../components/PagerBar.vue";
import { endReasonLabel, formatDateTime, formatDuration, modeLabel } from "../lib/format";
import type { AdminMatch, MatchPage } from "../types";

const pageSize = 20;
const page = ref(1);
const data = ref<MatchPage | null>(null);
const loading = ref(false);
const error = ref("");

async function load() {
  loading.value = true;
  error.value = "";
  try {
    data.value = await adminApi.matches(page.value, pageSize);
  } catch (err) {
    error.value = err instanceof ApiError ? err.message : "对局记录加载失败。";
  } finally {
    loading.value = false;
  }
}

function changePage(next: number) {
  page.value = next;
  load();
}

function duration(match: AdminMatch): string {
  if (!match.endedAtMs) return "进行中 / 未归档";
  return formatDuration(Math.max(0, match.endedAtMs - match.startedAtMs));
}

onMounted(load);
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <p class="eyebrow">MATCHES</p>
        <h2>对局记录</h2>
        <p class="page-head__desc">读取 PG 的 <code>match_record</code>，按开始时间倒序。用于核对场次、胜方与结束原因。</p>
      </div>
      <div class="page-head__actions">
        <span v-if="data" class="muted">累计 {{ data.total }} 场</span>
        <button class="btn btn--ghost btn--sm" type="button" :disabled="loading" @click="load">
          {{ loading ? "刷新中…" : "刷新" }}
        </button>
      </div>
    </header>

    <DataState
      v-if="!data && loading"
      kind="loading"
      title="正在读取对局记录"
      message="没有配置 PostgreSQL 时，这里会保持为空。"
    />
    <DataState v-else-if="!data && error" kind="error" title="对局记录加载失败" :message="error">
      <button class="btn btn--primary btn--sm" type="button" @click="load">重新加载</button>
    </DataState>

    <template v-else-if="data">
      <div v-if="error" class="inline-alert inline-alert--error" role="alert">
        {{ error }}
        <button class="text-link" type="button" @click="load">重试</button>
      </div>

      <div v-if="data.items.length === 0" class="panel">
        <DataState
          kind="empty"
          title="还没有已归档的对局"
          message="服务端未接 PostgreSQL 时不会写入 match_record；接库并打完至少一局后，这里才有记录。"
        />
      </div>

      <div v-else class="table-card" :class="{ 'is-loading': loading }">
        <table class="data-table">
          <thead>
            <tr>
              <th>场次</th>
              <th>模式</th>
              <th>胜方</th>
              <th>结束原因</th>
              <th>开始时间</th>
              <th>时长</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="match in data.items" :key="match.matchId">
              <td data-label="场次">
                <div class="stacked">
                  <strong class="mono">{{ match.matchId }}</strong>
                  <small class="muted">{{ match.gameId }} · {{ match.seatsCount }} 人</small>
                </div>
              </td>
              <td data-label="模式">{{ modeLabel(match.matchType) }}</td>
              <td data-label="胜方">
                <span v-if="match.winnerTeam !== null" class="team-tag">第 {{ match.winnerTeam }} 队</span>
                <span v-else class="muted">—</span>
              </td>
              <td data-label="结束原因">{{ endReasonLabel(match.endReason) }}</td>
              <td data-label="开始时间">{{ formatDateTime(match.startedAtMs) }}</td>
              <td data-label="时长"><span class="mono">{{ duration(match) }}</span></td>
            </tr>
          </tbody>
        </table>

        <PagerBar :page="page" :page-size="pageSize" :total="data.total" @change="changePage" />
      </div>
    </template>
  </div>
</template>
