<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { ApiError, adminApi } from "../api";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import DataState from "../components/DataState.vue";
import PagerBar from "../components/PagerBar.vue";
import StatusPill from "../components/StatusPill.vue";
import { formatDateTime, formatRelative } from "../lib/format";
import { toast } from "../lib/toast";
import type { AdminUser, UserPage, UserStatus } from "../types";

const pageSize = 20;
const keyword = ref("");
const status = ref<UserStatus | "">("");
const page = ref(1);
const data = ref<UserPage | null>(null);
const loading = ref(false);
const error = ref("");

const pendingUser = ref<AdminUser | null>(null);
const actionBusy = ref(false);

const statusTabs: { value: UserStatus | ""; label: string }[] = [
  { value: "", label: "全部" },
  { value: "active", label: "启用" },
  { value: "disabled", label: "禁用" },
];

const confirmVisible = computed(() => pendingUser.value !== null);
const confirmDanger = computed(() => pendingUser.value?.status === "active");
const confirmTitle = computed(() =>
  confirmDanger.value ? "禁用这个账号？" : "恢复这个账号？",
);
const confirmMessage = computed(() => {
  const user = pendingUser.value;
  if (!user) return "";
  return confirmDanger.value
    ? `禁用后 ${user.nickname}（${user.playerId}）的旧 access token 立即失效，且无法再登录或刷新令牌。`
    : `解禁后 ${user.nickname}（${user.playerId}）可以重新登录，已有 access token 在未撤销时恢复可用。`;
});

async function load() {
  loading.value = true;
  error.value = "";
  try {
    data.value = await adminApi.users({
      keyword: keyword.value.trim(),
      status: status.value,
      page: page.value,
      pageSize,
    });
  } catch (err) {
    error.value = err instanceof ApiError ? err.message : "用户列表加载失败。";
  } finally {
    loading.value = false;
  }
}

function search() {
  page.value = 1;
  load();
}

function switchStatus(value: UserStatus | "") {
  if (status.value === value) return;
  status.value = value;
  page.value = 1;
  load();
}

function changePage(next: number) {
  page.value = next;
  load();
}

function askToggle(user: AdminUser) {
  pendingUser.value = user;
}

async function confirmToggle() {
  const user = pendingUser.value;
  if (!user) return;
  actionBusy.value = true;
  try {
    const target: UserStatus = user.status === "active" ? "disabled" : "active";
    await adminApi.updateUser(user.playerId, target);
    toast(target === "disabled" ? "账号已禁用，旧令牌立即失效。" : "账号已恢复启用。");
    pendingUser.value = null;
    await load();
  } catch (err) {
    toast(err instanceof ApiError ? err.message : "操作失败。", "error");
  } finally {
    actionBusy.value = false;
  }
}

onMounted(load);
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <p class="eyebrow">ACCOUNTS</p>
        <h2>用户管理</h2>
        <p class="page-head__desc">按玩家 ID、昵称或手机尾号检索。启停状态即时生效，并自动写入审计。</p>
      </div>
      <div class="page-head__actions">
        <span v-if="data" class="muted">共 {{ data.total }} 个匹配账号</span>
      </div>
    </header>

    <section class="toolbar">
      <form class="search-box" @submit.prevent="search">
        <span class="search-box__icon" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none">
            <circle cx="10.5" cy="10.5" r="6" />
            <path d="M15.2 15.2 20 20" />
          </svg>
        </span>
        <input
          v-model="keyword"
          class="input input--flush"
          type="search"
          placeholder="玩家 ID / 昵称 / 手机尾号"
          aria-label="搜索用户"
        />
        <button class="btn btn--primary btn--sm" type="submit" :disabled="loading">检索</button>
      </form>

      <div class="segmented" role="tablist" aria-label="按状态筛选">
        <button
          v-for="tab in statusTabs"
          :key="tab.value || 'all'"
          class="segmented__item"
          :class="{ 'segmented__item--active': status === tab.value }"
          type="button"
          role="tab"
          :aria-selected="status === tab.value"
          @click="switchStatus(tab.value)"
        >
          {{ tab.label }}
        </button>
      </div>
    </section>

    <DataState
      v-if="!data && loading"
      kind="loading"
      title="正在读取账号"
      message="没有 PG 时读取的是服务端内存档案，重启后仍会保留已恢复的部分。"
    />
    <DataState v-else-if="!data && error" kind="error" title="账号列表加载失败" :message="error">
      <button class="btn btn--primary btn--sm" type="button" @click="load">重新加载</button>
    </DataState>

    <template v-else-if="data">
      <div v-if="error" class="inline-alert inline-alert--error" role="alert">
        {{ error }}
        <button class="text-link" type="button" @click="load">重试</button>
      </div>

      <div v-if="data.items.length === 0" class="panel">
        <DataState kind="empty" title="没有匹配的账号" message="换一个关键词，或切换状态筛选再试。">
          <button class="btn btn--ghost btn--sm" type="button" @click="keyword = ''; status = ''; search()">
            清空条件
          </button>
        </DataState>
      </div>

      <div v-else class="table-card" :class="{ 'is-loading': loading }">
        <table class="data-table">
          <thead>
            <tr>
              <th>玩家</th>
              <th>手机尾号</th>
              <th>状态</th>
              <th>注册时间</th>
              <th class="data-table__action">操作</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="user in data.items" :key="user.playerId">
              <td data-label="玩家">
                <div class="user-cell">
                  <span class="avatar">{{ user.nickname.slice(0, 1) }}</span>
                  <div>
                    <strong>{{ user.nickname }}</strong>
                    <small class="mono">{{ user.playerId }}</small>
                  </div>
                </div>
              </td>
              <td data-label="手机尾号"><span class="mono">****{{ user.phoneTail4 }}</span></td>
              <td data-label="状态">
                <StatusPill
                  :tone="user.status === 'active' ? 'active' : 'disabled'"
                  :label="user.status === 'active' ? '启用' : '禁用'"
                  dot
                />
              </td>
              <td data-label="注册时间">
                <div class="stacked">
                  <span>{{ formatDateTime(user.createdAtMs) }}</span>
                  <small class="muted">{{ formatRelative(user.createdAtMs) }}</small>
                </div>
              </td>
              <td class="data-table__action" data-label="操作">
                <button
                  class="btn btn--sm"
                  :class="user.status === 'active' ? 'btn--danger-ghost' : 'btn--ghost'"
                  type="button"
                  @click="askToggle(user)"
                >
                  {{ user.status === "active" ? "禁用" : "启用" }}
                </button>
              </td>
            </tr>
          </tbody>
        </table>

        <PagerBar :page="page" :page-size="pageSize" :total="data.total" @change="changePage" />
      </div>
    </template>

    <ConfirmDialog
      :visible="confirmVisible"
      :title="confirmTitle"
      :message="confirmMessage"
      :confirm-text="confirmDanger ? '确认禁用' : '恢复启用'"
      :danger="confirmDanger"
      :busy="actionBusy"
      @close="pendingUser = null"
      @confirm="confirmToggle"
    />
  </div>
</template>
