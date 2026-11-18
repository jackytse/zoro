/**
 * 相对时间展示，让「最近」有排序的直观感受。
 * 托盘弹窗与设置页的项目列表共用，保证两处文案一致。
 *
 * 文案来自 i18n 语言包，日期部分按当前 locale 格式化。
 */
import { i18n, currentLocale } from "../i18n";

/** 各语言对应的日期格式化 locale（IETF 语言标签） */
const DATE_LOCALES: Record<string, string> = {
  "zh-CN": "zh-CN",
  "zh-TW": "zh-TW",
  en: "en-US",
};

/** 把毫秒时间戳转成口语化相对时间；null/0 表示从未打开过 */
export function formatRelativeTime(ts: number | null): string {
  const t = i18n.global.t;
  if (!ts) return t("time.never");
  const diff = Date.now() - ts;
  const minute = 60_000;
  if (diff < minute) return t("time.justNow");
  if (diff < 60 * minute)
    return t("time.minutesAgo", { n: Math.floor(diff / minute) });
  if (diff < 24 * 60 * minute)
    return t("time.hoursAgo", { n: Math.floor(diff / (60 * minute)) });
  return new Date(ts).toLocaleDateString(
    DATE_LOCALES[currentLocale()] ?? "zh-CN",
  );
}
