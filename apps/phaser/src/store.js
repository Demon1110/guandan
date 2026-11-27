// 本地回放存档：只存「动作日志 + 场次元信息」，不存 whole state。
// 参考 apps/web/app.js 的 IndexedDB 方案：恢复时用同种子重建房间、按 roundNo 跨局重放动作，
// 最后用 wasm 的权威 hash 自证，避免坏存档带着错误状态继续打。
//
// 两个 objectStore：
//   matches  keyPath = matchId        场次元信息（种子/座位/难度/lastHash/lastRoomSeq/lastRoundNo）
//   actions  keyPath = [matchId, seq] 动作日志（seq 为引擎 roomSeq，天然单调递增且不跨局重置）
// IndexedDB 不可用（隐私模式等）时所有写操作静默失败、读操作返回空，游戏本身不受影响。

const DB_NAME = 'gf-phaser-replay';
const DB_VERSION = 1;
const STORE_MATCH = 'matches';
const STORE_ACTION = 'actions';

/** 场次 ID：种子 + 人类座位是续局指纹；同种子换座位是另一盘牌，不能命中旧存档。 */
export function matchIdOf(seedHi, seedLo, humanSeat) {
  const hi = Number(seedHi) >>> 0;
  const lo = Number(seedLo) >>> 0;
  const s = Number.isInteger(Number(humanSeat)) ? Number(humanSeat) & 0x3 : 0;
  return `phaser-${hi.toString(16)}${lo.toString(16).padStart(8, '0')}-s${s}`;
}

function reqToPromise(req) {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

function txDone(tx) {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error ?? new Error('IndexedDB 事务中止'));
  });
}

export class ReplayStore {
  constructor() {
    this.db = null;
    this.error = null;
  }

  async open() {
    if (this.db) return this.db;
    if (typeof indexedDB === 'undefined') {
      this.error = 'IndexedDB 不可用';
      return null;
    }
    try {
      this.db = await new Promise((resolve, reject) => {
        const req = indexedDB.open(DB_NAME, DB_VERSION);
        req.onupgradeneeded = () => {
          const db = req.result;
          if (!db.objectStoreNames.contains(STORE_MATCH)) {
            const store = db.createObjectStore(STORE_MATCH, { keyPath: 'matchId' });
            store.createIndex('byUpdatedAt', 'updatedAt');
          }
          if (!db.objectStoreNames.contains(STORE_ACTION)) {
            db.createObjectStore(STORE_ACTION, { keyPath: ['matchId', 'seq'] });
          }
        };
        req.onsuccess = () => resolve(req.result);
        req.onerror = () => reject(req.error);
        req.onblocked = () => reject(new Error('IndexedDB 升级被其他标签页阻塞'));
      });
    } catch (e) {
      this.error = e?.message ?? String(e);
      console.warn('[replay] IndexedDB 打开失败，本局仍可玩但无法续局', e);
    }
    return this.db;
  }

  async loadMatch(matchId) {
    if (!this.db || !matchId) return null;
    try {
      return await reqToPromise(this.db.transaction(STORE_MATCH, 'readonly')
        .objectStore(STORE_MATCH).get(matchId));
    } catch (e) {
      console.warn('[replay] 读取场次元信息失败', e);
      return null;
    }
  }

  async loadActions(matchId) {
    if (!this.db || !matchId) return [];
    try {
      // 复合键 [matchId, seq]：用范围只取本场记录，不把整库读进 JS 再过滤。
      const range = IDBKeyRange.bound([matchId, 0], [matchId, Number.MAX_SAFE_INTEGER]);
      const acts = await reqToPromise(this.db.transaction(STORE_ACTION, 'readonly')
        .objectStore(STORE_ACTION).getAll(range));
      return acts.sort((a, b) => a.seq - b.seq);
    } catch (e) {
      console.warn('[replay] 读取动作日志失败', e);
      return [];
    }
  }

  /** 取最近更新的场次，用于「继续上一局」——不依赖 localStorage 指针。 */
  async latestMatch() {
    if (!this.db) return null;
    try {
      const idx = this.db.transaction(STORE_MATCH, 'readonly')
        .objectStore(STORE_MATCH).index('byUpdatedAt');
      return await new Promise((resolve, reject) => {
        const req = idx.openCursor(null, 'prev');
        req.onsuccess = () => resolve(req.result?.value ?? null);
        req.onerror = () => reject(req.error);
      });
    } catch (e) {
      console.warn('[replay] 查找最近场次失败', e);
      return null;
    }
  }

  async putMatch(meta) {
    if (!this.db || !meta?.matchId) return false;
    try {
      const tx = this.db.transaction(STORE_MATCH, 'readwrite');
      tx.objectStore(STORE_MATCH).put(meta);
      await txDone(tx);
      return true;
    } catch (e) {
      console.warn('[replay] 写入场次元信息失败', e);
      return false;
    }
  }

  /** 动作与「最新权威哈希」同一事务落盘：杀进程时两者必然一致，恢复后才有比对基线。 */
  async appendAction(matchId, rec, meta) {
    if (!this.db || !matchId) return false;
    try {
      const tx = this.db.transaction([STORE_ACTION, STORE_MATCH], 'readwrite');
      tx.objectStore(STORE_ACTION).put({ matchId, ...rec });
      if (meta) tx.objectStore(STORE_MATCH).put(meta);
      await txDone(tx);
      return true;
    } catch (e) {
      console.warn('[replay] 写入动作日志失败', e);
      return false;
    }
  }

  async clearMatch(matchId) {
    if (!this.db || !matchId) return false;
    try {
      // 单个事务内按键范围删除：重开同种子时旧动作不会残留污染新局。
      const tx = this.db.transaction([STORE_ACTION, STORE_MATCH], 'readwrite');
      const range = IDBKeyRange.bound([matchId, 0], [matchId, Number.MAX_SAFE_INTEGER]);
      tx.objectStore(STORE_ACTION).delete(range);
      tx.objectStore(STORE_MATCH).delete(matchId);
      await txDone(tx);
      return true;
    } catch (e) {
      console.warn('[replay] 清除存档失败', e);
      return false;
    }
  }
}
