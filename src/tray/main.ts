import "../assets/css/main.css";
// 必须在 main.css 之后引入：覆盖 preflight 给 body 设的不透明背景，
// 否则透明窗口会画出直角黑底（圆角被盖住、四周出黑边）
import "./tray.css";

import { createApp } from "vue";
import ui from "@nuxt/ui/vue-plugin";
import TrayPopup from "./TrayPopup.vue";
import { i18n } from "../i18n";

// 托盘弹窗是独立入口，体量小、生命周期短，不引入路由
const app = createApp(TrayPopup);
app.use(ui);
app.use(i18n);
app.mount("#tray");
