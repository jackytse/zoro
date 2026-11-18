import { createRouter, createWebHistory } from "vue-router";
import SettingsView from "../views/SettingsView.vue";

/**
 * 主窗口只承载「设置」一个功能页：
 * 托盘弹窗的「设置」入口和 Dock 重新唤起都落到根路径。
 */
const router = createRouter({
  // Tauri 在 macOS/Linux 上以 file:// 加载构建产物，BASE_URL 用于兼容子路径部署
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    {
      path: "/",
      name: "settings",
      component: SettingsView,
    },
    {
      // 桌面端没有地址栏，出现未知路径时回到设置页而不是留白
      path: "/:pathMatch(.*)*",
      redirect: "/",
    },
  ],
});

export default router;
