<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { ApiError, adminApi, clearToken, getToken, setToken } from "../api";

const router = useRouter();
const route = useRoute();

const token = ref("");
const loading = ref(false);
const error = ref("");
const expired = computed(() => route.query.expired === "1");

onMounted(() => {
  // 已经有令牌时由路由守卫直接放行，这里只把残留值带进输入框，避免误导。
  token.value = getToken();
});

async function submit() {
  const value = token.value.trim();
  if (!value) {
    error.value = "请填写运营后台管理令牌。";
    return;
  }
  loading.value = true;
  error.value = "";
  setToken(value);
  try {
    await adminApi.session();
    const redirect = typeof route.query.redirect === "string" ? route.query.redirect : "/";
    // 只接受站内路径，避免把登录页当开放跳板。
    const target = redirect.startsWith("/") && !redirect.startsWith("//") ? redirect : "/";
    await router.replace(target);
  } catch (err) {
    clearToken();
    error.value = err instanceof ApiError ? err.message : "登录失败，请稍后重试。";
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <main class="login-page">
    <section class="login-brand">
      <div class="login-brand__glow" aria-hidden="true" />
      <div class="brand-lockup">
        <span class="brand-mark">掼</span>
        <div>
          <p class="brand-lockup__name">掼蛋 · 运营后台</p>
          <p class="brand-lockup__sub">GUANDAN OPS CONSOLE</p>
        </div>
      </div>

      <div class="login-brand__copy">
        <p class="eyebrow">M2 · 最小管理台</p>
        <h1>把牌桌之外的事，<br />收进一个安静的界面。</h1>
        <p class="login-brand__desc">
          账号巡检、玩法开关、对局与审计集中在一处。所有写操作留痕，配置改动即时作用到新建房间与匹配，
          不需要重启服务。
        </p>
      </div>

      <ul class="login-brand__points">
        <li><span>01</span>账号启停与状态巡检</li>
        <li><span>02</span>玩法模式与续局节奏</li>
        <li><span>03</span>对局记录与操作审计</li>
      </ul>

      <p class="login-brand__foot">内网管理入口 · 令牌独立于玩家登录态</p>
    </section>

    <section class="login-panel">
      <div class="login-card">
        <header class="login-card__head">
          <p class="eyebrow">ADMIN ACCESS</p>
          <h2>进入运营后台</h2>
          <p>使用服务端配置的 <code>GF_ADMIN_TOKEN</code> 登录。</p>
        </header>

        <div v-if="expired" class="inline-alert inline-alert--warn" role="status">
          登录状态已失效，请重新输入管理令牌。
        </div>

        <form class="login-form" @submit.prevent="submit">
          <label class="field">
            <span class="field__label">管理令牌</span>
            <div class="input-wrap">
              <input
                v-model="token"
                class="input input--mono"
                type="password"
                name="adminToken"
                autocomplete="off"
                spellcheck="false"
                placeholder="gf-dev-admin-token"
                :disabled="loading"
              />
            </div>
            <span class="field__hint">本地默认令牌：<code>gf-dev-admin-token</code>；生产环境请以部署配置为准。</span>
          </label>

          <p v-if="error" class="form-error" role="alert">{{ error }}</p>

          <button class="btn btn--primary btn--block" type="submit" :disabled="loading">
            <span v-if="loading" class="spinner spinner--inline" aria-hidden="true" />
            {{ loading ? "校验令牌…" : "进入后台" }}
          </button>
        </form>

        <footer class="login-card__foot">
          <span class="dot-sep" aria-hidden="true" />
          令牌仅保存在当前浏览器的 sessionStorage，关闭标签页即清除。
        </footer>
      </div>
    </section>
  </main>
</template>
