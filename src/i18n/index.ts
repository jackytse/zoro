/**
 * 多语言支持：简体中文 / 繁體中文 / English。
 *
 * 设计要点：
 * - 默认跟随系统语言（navigator.language 探测），用户可在设置页显式选择；
 * - 用户选择持久化在后端 config.json 的 locale 字段（None = 跟随系统），
 *   托盘弹窗与主窗口两个独立入口共用同一份配置；
 * - 配置变更广播（config-changed）触达后，各入口读 locale 并热切换，
 *   无需重启应用；Rust 侧托盘菜单同样按 locale 重建。
 */
import { createI18n } from "vue-i18n";
import zhCN from "./locales/zh-CN";
import zhTW from "./locales/zh-TW";
import en from "./locales/en";

export type AppLocale = "zh-CN" | "zh-TW" | "en";

/** 各语言的展示名（设置页选择器用，始终以各自语言显示自己） */
export const LOCALE_LABELS: Record<AppLocale, string> = {
  "zh-CN": "简体中文",
  "zh-TW": "繁體中文",
  en: "English",
};

/** 三个合法取值，用于归一化配置文件里可能出现的脏数据 */
const SUPPORTED: AppLocale[] = ["zh-CN", "zh-TW", "en"];

/** 从任意字符串归一化出受支持的 locale；不认识的一律回落英文 */
export function normalizeLocale(raw: string | null | undefined): AppLocale | null {
  if (!raw) return null;
  const hit = SUPPORTED.find((l) => l.toLowerCase() === raw.toLowerCase());
  return hit ?? null;
}

/** 探测系统语言：中文按简繁分流，其余统一英文 */
export function detectLocale(): AppLocale {
  const lang = navigator.language ?? "";
  if (/^zh/i.test(lang)) {
    // 繁体地区/书写标识：台湾、香港、澳门、Hant
    return /^zh-(tw|hk|mo|hant)/i.test(lang) ? "zh-TW" : "zh-CN";
  }
  return "en";
}

/**
 * 当前应生效的 locale：配置里有显式选择就用它，否则跟随系统。
 * 后端 config 的 locale 为 null/空 表示「跟随系统」。
 */
export function resolveLocale(configured: string | null | undefined): AppLocale {
  return normalizeLocale(configured) ?? detectLocale();
}

/** 创建 i18n 实例：初始 locale 先按系统语言渲染，配置加载后再热切换 */
export const i18n = createI18n({
  legacy: false, // 组合式 API（useI18n / t()）
  locale: detectLocale(),
  fallbackLocale: "zh-CN", // 缺 key 时回落简体中文（文案最全的语言）
  messages: {
    "zh-CN": zhCN,
    "zh-TW": zhTW,
    en,
  },
});

/** 切换语言并同步到 <html lang>（影响浏览器级排版/无障碍朗读） */
export function applyLocale(locale: AppLocale) {
  i18n.global.locale.value = locale;
  document.documentElement.lang = locale;
}

/** 当前 locale（供时间格式化等非组件模块读取） */
export function currentLocale(): AppLocale {
  return i18n.global.locale.value as AppLocale;
}
