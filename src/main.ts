import "./assets/css/main.css";

import { createApp } from "vue";
import ui from "@nuxt/ui/vue-plugin";
import App from "./App.vue";
import router from "./router";
import { i18n } from "./i18n";

const app = createApp(App);

// 主窗口禁用 WebView 右键菜单（避免暴露浏览器菜单与「检查元素」入口）。
// 只拦截默认行为，不阻断页面自身对 contextmenu 的业务监听。
document.addEventListener("contextmenu", (e) => e.preventDefault());

// 顺序无关，但 router 必须先于 mount 注册，否则首屏路由解析会丢失
app.use(router);
app.use(ui);
app.use(i18n);

app.mount("#app");
