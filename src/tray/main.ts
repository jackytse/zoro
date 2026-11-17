import "../assets/css/main.css";

import { createApp } from "vue";
import ui from "@nuxt/ui/vue-plugin";
import TrayPopup from "./TrayPopup.vue";

// 托盘弹窗是独立入口，体量小、生命周期短，不引入路由
const app = createApp(TrayPopup);
app.use(ui);
app.mount("#tray");
