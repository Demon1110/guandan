<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { clearToken } from "../api";
import NavIcon from "../components/NavIcon.vue";

const route = useRoute();
const router = useRouter();
const navOpen = ref(false);

const navItems = [
  { name: "overview", path: "/", title: "总览", hint: "账号、对局与系统状态" },
  { name: "users", path: "/users", title: "用户管理", hint: "检索、启用与禁用" },
  { name: "games", path: "/games", title: "玩法配置", hint: "模式开关与续局节奏" },
  { name: "matches", path: "/matches", title: "对局记录", hint: "已结束场次只读视图" },
  { name: "audits", path: "/audits", title: "审计日志", hint: "后台写操作留痕" },
];

const title = computed(() => String(route.meta.title ?? "运营后台"));

watch(
  () => route.fullPath,
  () => {
    navOpen.value = false;
  },
);

function logout() {
  clearToken();
  router.replace({ name: "login" });
}
</script>

<template>
  <div class="admin-shell">
    <aside class="sidebar" :class="{ 'sidebar--open': navOpen }">
      <div class="sidebar__brand">
        <span class="brand-mark brand-mark--sm">掼</span>
        <div>
          <p class="sidebar__name">掼蛋运营台</p>
          <p class="sidebar__sub">OPS CONSOLE</p>
        </div>
      </div>

      <nav class="sidebar__nav" aria-label="后台导航">
        <RouterLink
          v-for="item in navItems"
          :key="item.name"
          :to="item.path"
          class="nav-link"
          :class="{ 'nav-link--active': route.name === item.name }"
        >
          <NavIcon :name="item.name" />
          <span class="nav-link__text">
            <strong>{{ item.title }}</strong>
            <small>{{ item.hint }}</small>
          </span>
        </RouterLink>
      </nav>

      <div class="sidebar__foot">
        <div class="sidebar__status">
          <span class="live-dot" aria-hidden="true" />
          <div>
            <strong>管理会话</strong>
            <small>令牌存于 sessionStorage</small>
          </div>
        </div>
        <button class="btn btn--quiet btn--block" type="button" @click="logout">退出登录</button>
        <p class="sidebar__version">gf-admin 0.1.0 · M2</p>
      </div>
    </aside>

    <div v-if="navOpen" class="sidebar-scrim" aria-hidden="true" @click="navOpen = false" />

    <div class="shell-main">
      <header class="topbar">
        <button class="icon-btn topbar__menu" type="button" aria-label="打开导航" @click="navOpen = true">
          <span /><span /><span />
        </button>
        <div class="topbar__title">
          <p class="topbar__crumb">掼蛋 / 运营后台</p>
          <h1>{{ title }}</h1>
        </div>
        <div class="topbar__side">
          <span class="topbar__env"><span class="live-dot" aria-hidden="true" />接口服务</span>
          <button class="btn btn--ghost btn--sm" type="button" @click="logout">退出</button>
        </div>
      </header>

      <main class="content">
        <RouterView />
      </main>
    </div>
  </div>
</template>
