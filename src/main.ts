import "./assets/css/main.css";

import { createApp } from "vue";
import ui from "@nuxt/ui/vue-plugin";
import App from "./App.vue";
import router from "./router";

const app = createApp(App);

// 顺序无关，但 router 必须先于 mount 注册，否则首屏路由解析会丢失
app.use(router);
app.use(ui);

app.mount("#app");
