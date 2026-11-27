import type {
    AdminSession,
    AdminUser,
    AuditEntry,
    GameSetting,
    MatchPage,
    Overview,
    UserPage,
    UserStatus,
} from "./types";

const TOKEN_KEY = "gf.admin.token";
const API_BASE = (import.meta.env.VITE_API_BASE ?? "/api").replace(/\/$/, "");

export class ApiError extends Error {
  readonly status: number;
  readonly reason: string;

  constructor(status: number, reason: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.reason = reason;
  }
}

export function getToken(): string {
  return sessionStorage.getItem(TOKEN_KEY) ?? "";
}

export function setToken(token: string): void {
  sessionStorage.setItem(TOKEN_KEY, token);
}

export function clearToken(): void {
  sessionStorage.removeItem(TOKEN_KEY);
}

interface RequestOptions {
  method?: "GET" | "POST" | "PATCH" | "PUT" | "DELETE";
  body?: unknown;
  query?: Record<string, string | number | undefined>;
}

async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const token = getToken();
  const url = new URL(API_BASE + path, window.location.origin);
  for (const [key, value] of Object.entries(options.query ?? {})) {
    if (value !== undefined && value !== "") {
      url.searchParams.set(key, String(value));
    }
  }

  let response: Response;
  try {
    response = await fetch(url.toString(), {
      method: options.method ?? "GET",
      headers: {
        ...(options.body === undefined ? {} : { "content-type": "application/json" }),
        ...(token ? { authorization: `Bearer ${token}` } : {}),
      },
      body: options.body === undefined ? undefined : JSON.stringify(options.body),
    });
  } catch {
    throw new ApiError(0, "NETWORK", "连接不到服务端，请确认接口服务已启动。");
  }

  const text = await response.text();
  let payload: unknown = null;
  if (text) {
    try {
      payload = JSON.parse(text);
    } catch {
      payload = null;
    }
  }

  if (!response.ok) {
    const reason =
      payload && typeof payload === "object" && "reason" in payload
        ? String((payload as { reason: unknown }).reason)
        : "";
    if (response.status === 401 && reason === "ADMIN_UNAUTHORIZED") {
      clearToken();
      window.dispatchEvent(new CustomEvent("gf-admin-unauthorized"));
    }
    throw new ApiError(response.status, reason, describeError(response.status, reason));
  }
  return payload as T;
}

function describeError(status: number, reason: string): string {
  switch (reason) {
    case "ADMIN_DISABLED":
      return "服务端未配置运营后台令牌（GF_ADMIN_TOKEN），后台接口处于关闭状态。";
    case "ADMIN_UNAUTHORIZED":
      return "管理令牌无效或已变更，请重新登录。";
    case "USER_NOT_FOUND":
      return "玩家不存在，可能已被删除或数据已刷新。";
    case "USER_STATUS_INVALID":
      return "用户状态不合法，仅支持启用 / 禁用。";
    case "GAME_NAME_INVALID":
      return "玩法名称需为 1–24 个字符。";
    case "GAME_MODE_INVALID":
      return "模式取值不合法，仅支持休闲、排位、混合。";
    case "GAME_MODES_REQUIRED":
      return "启用玩法时至少要保留一个模式。";
    case "NEXT_DEAL_INVALID":
      return "续局间隔需在 1 秒到 10 分钟之间。";
    case "GAME_NOT_FOUND":
      return "玩法不存在。";
    case "STORAGE_UNAVAILABLE":
      return "存储层暂时不可用，请稍后重试。";
    default:
      if (status === 401) return "登录已失效，请重新登录。";
      if (status === 503) return "服务暂不可用，请稍后重试。";
      return `请求失败（HTTP ${status}${reason ? ` · ${reason}` : ""}）。`;
  }
}

export const adminApi = {
  session(): Promise<AdminSession> {
    return request<AdminSession>("/admin/session");
  },

  overview(): Promise<Overview> {
    return request<Overview>("/admin/overview");
  },

  users(params: {
    keyword: string;
    status: UserStatus | "";
    page: number;
    pageSize: number;
  }): Promise<UserPage> {
    return request<UserPage>("/admin/users", { query: params });
  },

  updateUser(playerId: string, status: UserStatus): Promise<{ ok: boolean; user: AdminUser }> {
    return request(`/admin/users/${encodeURIComponent(playerId)}`, {
      method: "PATCH",
      body: { status },
    });
  },

  games(): Promise<{ items: GameSetting[] }> {
    return request("/admin/games");
  },

  saveGame(
    gameId: string,
    payload: Pick<GameSetting, "displayName" | "enabled" | "modes" | "nextDealMs">,
  ): Promise<{ ok: boolean; game: GameSetting }> {
    return request(`/admin/games/${encodeURIComponent(gameId)}`, {
      method: "PUT",
      body: payload,
    });
  },

  matches(page: number, pageSize: number): Promise<MatchPage> {
    return request("/admin/matches", { query: { page, pageSize } });
  },

  audits(): Promise<{ items: AuditEntry[] }> {
    return request("/admin/audits");
  },
};
