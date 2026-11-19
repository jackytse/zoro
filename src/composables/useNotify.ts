/**
 * 统一的通知入口。
 *
 * 为什么不直接用 useToast().add：
 * Nuxt UI 的 Toast 组件不会按 color 自动配图标（icon 必须显式传入），
 * 散落各处的调用容易漏。这里按「通知类型」统一收口：
 * 类型决定颜色与图标，成功/失败/警告/提示一眼可辨。
 */
import { useToast } from "@nuxt/ui/composables";

export type NotifyType = "success" | "error" | "warning" | "info";

/** 每种通知类型对应的图标（lucide 线性图标，与全站图标风格一致） */
const NOTIFY_ICONS: Record<NotifyType, string> = {
  success: "i-lucide-circle-check",
  error: "i-lucide-circle-alert",
  warning: "i-lucide-triangle-alert",
  info: "i-lucide-info",
};

export function useNotify() {
  const toast = useToast();

  /**
   * 弹一条 toast。
   * @param type 通知类型，决定颜色与图标
   * @param title 主文案
   * @param opts 可选：详细描述（错误原因等）、自定义展示时长（毫秒）、
   *             操作按钮（如「立即更新」，点击执行回调后可自行再弹后续通知）
   */
  function notify(
    type: NotifyType,
    title: string,
    opts?: {
      description?: string;
      duration?: number;
      actions?: { label: string; onClick: () => void | Promise<void> }[];
    },
  ) {
    toast.add({
      title,
      description: opts?.description,
      duration: opts?.duration,
      color: type,
      icon: NOTIFY_ICONS[type],
      actions: opts?.actions,
    });
  }

  return notify;
}

/**
 * 注册「窗口失焦即关闭限时 toast」。
 *
 * 背景：WebView 失去焦点后（用户切到别的应用），toast 的自动关闭计时器
 * 会被系统节流/暂停——切走再切回来时 toast 还挂在原地，观感就是「失焦后
 * 不关闭」。这里在 blur 时主动清掉所有限时 toast（本来就该自动消失的）。
 *
 * duration === 0 的持久 toast 例外保留：那是更新提示这类需要用户决策的
 * 消息，不能因为切了个窗口就丢。
 *
 * 返回解绑函数，调用方在卸载时移除监听。
 */
export function useToastBlurDismiss(): () => void {
  const toast = useToast();
  const onBlur = () => {
    for (const t of toast.toasts.value) {
      if (t.duration !== 0) toast.remove(t.id);
    }
  };
  window.addEventListener("blur", onBlur);
  return () => window.removeEventListener("blur", onBlur);
}
