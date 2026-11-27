import { createRouter, createWebHashHistory } from "vue-router";
import { getToken } from "./api";
import AdminLayout from "./layouts/AdminLayout.vue";
import AuditsView from "./views/AuditsView.vue";
import GamesView from "./views/GamesView.vue";
import LoginView from "./views/LoginView.vue";
import MatchesView from "./views/MatchesView.vue";
import OverviewView from "./views/OverviewView.vue";
import UsersView from "./views/UsersView.vue";

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: "/login",
      name: "login",
      component: LoginView,
      meta: { public: true, title: "登录" },
    },
    {
      path: "/",
      component: AdminLayout,
      children: [
        {
          path: "",
          name: "overview",
          component: OverviewView,
          meta: { title: "总览" },
        },
        {
          path: "users",
          name: "users",
          component: UsersView,
          meta: { title: "用户管理" },
        },
        {
          path: "games",
          name: "games",
          component: GamesView,
          meta: { title: "玩法配置" },
        },
        {
          path: "matches",
          name: "matches",
          component: MatchesView,
          meta: { title: "对局记录" },
        },
        {
          path: "audits",
          name: "audits",
          component: AuditsView,
          meta: { title: "审计日志" },
        },
      ],
    },
    { path: "/:pathMatch(.*)*", redirect: "/" },
  ],
});

router.beforeEach((to) => {
  const authed = Boolean(getToken());
  if (!authed && to.meta.public !== true) {
    return { name: "login", query: { redirect: to.fullPath } };
  }
  if (authed && to.name === "login") {
    return { name: "overview" };
  }
  return true;
});

router.afterEach((to) => {
  document.title = `${String(to.meta.title ?? "运营后台")} · 掼蛋`;
});

export default router;
