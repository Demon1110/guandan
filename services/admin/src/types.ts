export type UserStatus = "active" | "disabled";

export interface AdminSession {
  ok: boolean;
  gameId: string;
  rulesetVersion: string;
  serverTimeMs: number;
}

export interface Overview {
  generatedAtMs: number;
  users: {
    total: number;
    active: number;
    disabled: number;
    recent24h: number;
  };
  matches: {
    total: number;
    recent24h: number;
    rounds: number;
    activeRooms: number;
  };
  game: {
    gameId: string;
    displayName: string;
    enabled: boolean;
    modes: string[];
    nextDealMs: number;
  };
  storage: {
    postgres: boolean;
    redis: boolean;
  };
  uptimeMs: number;
}

export interface AdminUser {
  playerId: string;
  nickname: string;
  phoneTail4: string;
  status: UserStatus;
  createdAtMs: number;
}

export interface UserPage {
  items: AdminUser[];
  total: number;
  page: number;
  pageSize: number;
}

export interface GameSetting {
  gameId: string;
  displayName: string;
  enabled: boolean;
  modes: string[];
  nextDealMs: number;
  rulesetVersion: string;
  seats: number;
  updatedAtMs: number;
}

export interface AdminMatch {
  matchId: string;
  gameId: string;
  matchType: string;
  winnerTeam: number | null;
  endReason: string | null;
  seatsCount: number;
  startedAtMs: number;
  endedAtMs: number | null;
}

export interface MatchPage {
  items: AdminMatch[];
  total: number;
  page: number;
  pageSize: number;
}

export interface AuditEntry {
  auditId: string;
  action: string;
  target: string;
  actor: string;
  detail: Record<string, unknown>;
  createdAtMs: number;
}
