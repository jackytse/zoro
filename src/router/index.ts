import { createRouter, createWebHistory } from "vue-router";
import HomeView from "../views/HomeView.vue";

const router = createRouter({
  // Tauri 在 macOS/Linux 上以 file:// 加载构建产物，BASE_URL 用于兼容子路径部署
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    {
      path: "/",
      name: "home",
      component: HomeView,
    },
    {
      path: "/stack",
      name: "stack",
      // 路由级懒加载：技术栈页单独切 chunk，首屏不必下载
      component: () => import("../views/StackView.vue"),
    },
    {
      path: "/settings",
      name: "settings",
      // 托盘弹窗的「设置」入口指向这里
      component: () => import("../views/SettingsView.vue"),
    },
    {
      // 桌面端没有地址栏，出现未知路径时回到首页而不是留白
      path: "/:pathMatch(.*)*",
      redirect: "/",
    },
  ],
});

export default router;
