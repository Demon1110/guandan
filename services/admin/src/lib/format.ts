export const MODE_LABELS: Record<string, string> = {
  casual: "休闲场",
  ranked: "排位场",
  mixed: "混合场",
};

export const MODE_HINTS: Record<string, string> = {
  casual: "不计分，适合好友房与内测",
  ranked: "计入排位分，匹配同段位玩家",
  mixed: "两种场次共用同一入口",
};

export function formatDateTime(ms: number | null | undefined): string {
  if (!ms) return "—";
  const date = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(
    date.getHours(),
  )}:${pad(date.getMinutes())}`;
}

export function formatRelative(ms: number | null | undefined): string {
  if (!ms) return "—";
  const diff = Date.now() - ms;
  if (diff < 60_000) return "刚刚";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`;
  if (diff < 7 * 86_400_000) return `${Math.floor(diff / 86_400_000)} 天前`;
  return formatDateTime(ms);
}

export function formatDuration(ms: number): string {
  if (ms < 1_000) return `${ms} ms`;
  const seconds = Math.floor(ms / 1_000);
  if (seconds < 60) return `${seconds} 秒`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} 分 ${seconds % 60} 秒`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时 ${minutes % 60} 分`;
  return `${Math.floor(hours / 24)} 天 ${hours % 24} 小时`;
}

export function formatNumber(value: number | null | undefined): string {
  return (value ?? 0).toLocaleString("zh-CN");
}

export function modeLabel(mode: string): string {
  return MODE_LABELS[mode] ?? mode;
}

export function nextDealLabel(ms: number): string {
  return ms % 1000 === 0 ? `${ms / 1000} 秒` : `${ms} 毫秒`;
}

export function endReasonLabel(reason: string | null): string {
  if (!reason) return "—";
  const labels: Record<string, string> = {
    normal: "正常结束",
    timeout: "超时结束",
    abandoned: "中途解散",
    error: "异常结束",
  };
  return labels[reason] ?? reason;
}

export function actionLabel(action: string): string {
  const labels: Record<string, string> = {
    "user.status": "用户状态变更",
    "game.setting": "玩法配置变更",
  };
  return labels[action] ?? action;
}
