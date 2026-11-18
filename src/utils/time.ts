/**
 * 相对时间展示，让「最近」有排序的直观感受。
 * 托盘弹窗与设置页的项目列表共用，保证两处文案一致。
 */

/** 把毫秒时间戳转成口语化相对时间；null/0 表示从未打开过 */
export function formatRelativeTime(ts: number | null): string {
  if (!ts) return "未打开过";
  const diff = Date.now() - ts;
  const minute = 60_000;
  if (diff < minute) return "刚刚";
  if (diff < 60 * minute) return `${Math.floor(diff / minute)} 分钟前`;
  if (diff < 24 * 60 * minute) return `${Math.floor(diff / (60 * minute))} 小时前`;
  return new Date(ts).toLocaleDateString("zh-CN");
}
