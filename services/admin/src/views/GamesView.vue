<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { ApiError, adminApi } from "../api";
import DataState from "../components/DataState.vue";
import StatusPill from "../components/StatusPill.vue";
import {
    MODE_HINTS,
    MODE_LABELS,
    formatDateTime,
    formatRelative,
    nextDealLabel,
} from "../lib/format";
import { toast } from "../lib/toast";
import type { GameSetting } from "../types";

const allModes = ["casual", "ranked", "mixed"] as const;

const game = ref<GameSetting | null>(null);
const loading = ref(false);
const error = ref("");
const saving = ref(false);
const formError = ref("");

const form = reactive({
  displayName: "",
  enabled: true,
  modes: [] as string[],
  nextDealMs: 5_000,
});

const dirty = computed(() => {
  const current = game.value;
  if (!current) return false;
  return (
    form.displayName.trim() !== current.displayName ||
    form.enabled !== current.enabled ||
    form.nextDealMs !== current.nextDealMs ||
    form.modes.slice().sort().join(",") !== current.modes.slice().sort().join(",")
  );
});

const delayLabel = computed(() => nextDealLabel(form.nextDealMs || 0));

function applyGame(value: GameSetting) {
  game.value = value;
  form.displayName = value.displayName;
  form.enabled = value.enabled;
  form.modes = value.modes.filter((mode) => (allModes as readonly string[]).includes(mode));
  form.nextDealMs = value.nextDealMs;
}

async function load() {
  loading.value = true;
  error.value = "";
  try {
    const response = await adminApi.games();
    const first = response.items[0];
    if (!first) {
      error.value = "服务端没有返回可管理的玩法。";
      return;
    }
    applyGame(first);
  } catch (err) {
    error.value = err instanceof ApiError ? err.message : "玩法配置加载失败。";
  } finally {
    loading.value = false;
  }
}

function toggleMode(mode: string) {
  const index = form.modes.indexOf(mode);
  if (index >= 0) {
    form.modes.splice(index, 1);
  } else {
    form.modes.push(mode);
  }
}

function setDelay(ms: number) {
  form.nextDealMs = ms;
}

function reset() {
  if (game.value) applyGame(game.value);
  formError.value = "";
}

async function save() {
  if (!game.value) return;
  formError.value = "";
  const name = form.displayName.trim();
  if (!name || Array.from(name).length > 24) {
    formError.value = "玩法名称需为 1–24 个字符。";
    return;
  }
  if (form.enabled && form.modes.length === 0) {
    formError.value = "启用玩法时至少要保留一个模式。";
    return;
  }
  if (!Number.isFinite(form.nextDealMs) || form.nextDealMs < 1_000 || form.nextDealMs > 600_000) {
    formError.value = "续局间隔需在 1 秒到 10 分钟之间。";
    return;
  }
  saving.value = true;
  try {
    const response = await adminApi.saveGame(game.value.gameId, {
      displayName: name,
      enabled: form.enabled,
      modes: allModes.filter((mode) => form.modes.includes(mode)),
      nextDealMs: Math.round(form.nextDealMs),
    });
    applyGame(response.game);
    toast("玩法配置已保存，并即时作用于新房间与匹配。");
  } catch (err) {
    const message = err instanceof ApiError ? err.message : "保存失败。";
    formError.value = message;
    toast(message, "error");
  } finally {
    saving.value = false;
  }
}

onMounted(load);
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <p class="eyebrow">GAME SETTINGS</p>
        <h2>玩法配置</h2>
        <p class="page-head__desc">控制玩法开关、开放模式与结算后的续局间隔。保存后立即生效，不需要重启服务。</p>
      </div>
      <div v-if="game" class="page-head__actions">
        <span class="muted">最近保存 {{ formatRelative(game.updatedAtMs) }}</span>
      </div>
    </header>

    <DataState v-if="!game && loading" kind="loading" title="正在读取玩法配置" />
    <DataState v-else-if="!game && error" kind="error" title="玩法配置加载失败" :message="error">
      <button class="btn btn--primary btn--sm" type="button" @click="load">重新加载</button>
    </DataState>

    <div v-else-if="game" class="settings-layout">
      <section class="panel settings-panel" :class="{ 'is-loading': loading }">
        <header class="panel__head">
          <div>
            <p class="eyebrow">GUANDAN · V1</p>
            <h3>{{ game.displayName }}</h3>
          </div>
          <div class="panel__head-side">
            <StatusPill
              :tone="form.enabled ? 'on' : 'off'"
              :label="form.enabled ? '对外开启' : '暂停开放'"
              dot
            />
            <span v-if="dirty" class="dirty-badge">未保存</span>
          </div>
        </header>

        <div class="form-stack">
          <label class="field">
            <span class="field__label">玩法名称</span>
            <input
              v-model="form.displayName"
              class="input"
              type="text"
              maxlength="24"
              placeholder="掼蛋"
              :disabled="saving"
            />
            <span class="field__hint">展示在玩家侧大厅与后台标题，最多 24 个字符。</span>
          </label>

          <div class="setting-row">
            <div>
              <p class="setting-row__title">对外开启</p>
              <p class="setting-row__desc">
                关闭后新建房间与匹配入队返回 <code>GAME_DISABLED</code>；已在跑的房间仍可重连收尾。
              </p>
            </div>
            <button
              class="switch"
              type="button"
              role="switch"
              :aria-checked="form.enabled"
              :class="{ 'switch--on': form.enabled }"
              :disabled="saving"
              @click="form.enabled = !form.enabled"
            >
              <span class="switch__thumb" />
            </button>
          </div>

          <div class="setting-block">
            <div class="setting-block__head">
              <p class="setting-row__title">开放模式</p>
              <p class="setting-row__desc">至少保留一个；被关闭的模式在客户端建房 / 入队时会被拒绝。</p>
            </div>
            <div class="mode-grid">
              <label
                v-for="mode in allModes"
                :key="mode"
                class="mode-option"
                :class="{ 'mode-option--active': form.modes.includes(mode) }"
              >
                <input
                  class="mode-option__input"
                  type="checkbox"
                  :value="mode"
                  :checked="form.modes.includes(mode)"
                  :disabled="saving"
                  @change="toggleMode(mode)"
                />
                <span class="mode-option__check" aria-hidden="true">✓</span>
                <span class="mode-option__title">{{ MODE_LABELS[mode] }}</span>
                <span class="mode-option__hint">{{ MODE_HINTS[mode] }}</span>
              </label>
            </div>
          </div>

          <div class="setting-block">
            <div class="setting-block__head">
              <p class="setting-row__title">结算续局间隔</p>
              <p class="setting-row__desc">一局结束后等待多久自动开下一局，范围 1 秒–10 分钟。</p>
            </div>
            <div class="delay-editor">
              <div class="delay-editor__value">
                <input
                  v-model.number="form.nextDealMs"
                  class="input input--number"
                  type="number"
                  min="1000"
                  max="600000"
                  step="500"
                  :disabled="saving"
                />
                <span class="muted">毫秒</span>
                <strong>{{ delayLabel }}</strong>
              </div>
              <div class="delay-editor__presets">
                <button
                  v-for="preset in [3000, 5000, 8000, 12000]"
                  :key="preset"
                  class="chip"
                  type="button"
                  :class="{ 'chip--active': form.nextDealMs === preset }"
                  :disabled="saving"
                  @click="setDelay(preset)"
                >
                  {{ preset / 1000 }} 秒
                </button>
              </div>
            </div>
          </div>

          <p v-if="formError" class="form-error" role="alert">{{ formError }}</p>

          <div class="form-actions">
            <button class="btn btn--ghost" type="button" :disabled="saving || !dirty" @click="reset">
              放弃修改
            </button>
            <button class="btn btn--primary" type="button" :disabled="saving || !dirty" @click="save">
              <span v-if="saving" class="spinner spinner--inline" aria-hidden="true" />
              {{ saving ? "保存中…" : "保存配置" }}
            </button>
          </div>
        </div>
      </section>

      <aside class="settings-aside">
        <article class="panel panel--tight">
          <p class="eyebrow">READ ONLY</p>
          <h3>只读信息</h3>
          <dl class="kv-list kv-list--compact">
            <div class="kv-row">
              <dt>gameId</dt>
              <dd class="mono">{{ game.gameId }}</dd>
            </div>
            <div class="kv-row">
              <dt>规则集版本</dt>
              <dd class="mono">{{ game.rulesetVersion }}</dd>
            </div>
            <div class="kv-row">
              <dt>座位数</dt>
              <dd>{{ game.seats }} 人</dd>
            </div>
            <div class="kv-row">
              <dt>上次保存</dt>
              <dd>{{ formatDateTime(game.updatedAtMs) }}</dd>
            </div>
          </dl>
        </article>

        <article class="panel panel--tight note-panel">
          <p class="eyebrow">EFFECT</p>
          <h3>变更影响范围</h3>
          <ul class="note-list">
            <li>玩法开关与模式：下一次建房 / 入队即生效。</li>
            <li>续局间隔：只影响之后新建的房间。</li>
            <li>规则集版本由编译产物决定，后台只能查看。</li>
            <li>保存成功会写一条 <code>game.setting</code> 审计。</li>
          </ul>
          <p class="panel__footnote">规则集注册、dry-run、灰度与回滚属 M3 范围，不在当前管理台。</p>
        </article>
      </aside>
    </div>
  </div>
</template>
