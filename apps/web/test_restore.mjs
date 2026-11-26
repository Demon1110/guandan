// 无头续局用例：在 Node 里跑「存档 → 杀进程 → 重开回放」全链路。
//
// 形态与浏览器一致：房间跑在 wasm 里（1 人类座 + 3 个 L2 bot），每落一手就把
// `{roundNo, seat, pass, cards}` 记进「存档」；跑满两局后**丢弃整个 Instance**（= 杀进程），
// 再用同一个种子新建 Instance，按存档逐条重演，最终 `stateHash` 必须与存档哈希逐字节相等。
// 相等即证明续局不依赖进程内存，只需要那份动作日志。
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = join(here, '..', '..', 'target', 'wasm32-unknown-unknown', 'release', 'gf_client_wasm.wasm');

function loadInstance() {
  const bytes = readFileSync(wasmPath);
  const module = new WebAssembly.Module(bytes);
  const imports = WebAssembly.Module.imports(module);
  if (imports.length > 0) {
    throw new Error(`零 import 约定被破坏：${JSON.stringify(imports)}`);
  }
  return new WebAssembly.Instance(module, {}).exports;
}

function cstr(ex, ptr) {
  if (!ptr) throw new Error('wasm 返回空指针');
  const view = new Uint8Array(ex.memory.buffer, ptr);
  let end = 0;
  while (end < view.length && view[end] !== 0) end += 1;
  const text = new TextDecoder().decode(view.subarray(0, end));
  ex.gf_str_free(ptr);
  return text;
}

/** 把牌 id 写进 wasm 内存，回调结束后释放。 */
function withCards(ex, cards, fn) {
  if (!cards || cards.length === 0) return fn(0, 0);
  const len = cards.length * 2;
  const ptr = ex.gf_alloc(len);
  const view = new Uint16Array(ex.memory.buffer, ptr, cards.length);
  view.set(cards);
  try {
    return fn(ptr, cards.length);
  } finally {
    ex.gf_mem_free(ptr, len);
  }
}

const SEED_HI = 0x51ed;
const SEED_LO = 0xa101;
const HUMAN_MASK = 0b0001; // 座位 0 是人类，可随时接管；这里交给 L2 代打。

function newRoom(ex) {
  if (ex.gf_room_new(SEED_HI, SEED_LO, HUMAN_MASK) !== 1) throw new Error('建桌失败');
  if (ex.gf_room_start(0n) !== 1) throw new Error('发牌失败');
}

function submit(ex, seat, action, now) {
  const cards = action.pass ? [] : action.cards;
  const code = action.tributeReturn ? 2 : action.pass ? 1 : 0;
  return withCards(ex, cards, (ptr, len) =>
    cstr(ex, ex.gf_room_submit(seat, BigInt(now), code, ptr, len)),
  );
}

function replay(ex, seat, pass, cards, tributeReturn = false) {
  const code = tributeReturn ? 2 : pass ? 1 : 0;
  return withCards(ex, cards, (ptr, len) =>
    ex.gf_room_replay_action(seat, code, ptr, len) === 1,
  );
}

/** 人类座代打：tribute 相位从合法集里挑一张可还贡牌，其余交给 L2 probe。 */
function humanAction(ex, seat) {
  const info = JSON.parse(cstr(ex, ex.gf_room_info()));
  if (info.phase === 'tribute') {
    const legal = JSON.parse(cstr(ex, ex.gf_room_legal()));
    const ret = legal.find((a) => a.tributeReturn);
    if (!ret) throw new Error(`还贡相位没有合法还贡牌：${JSON.stringify(legal)}`);
    return { pass: false, tributeReturn: true, cards: ret.cards };
  }
  return JSON.parse(cstr(ex, ex.gf_l0_probe(seat)));
}

/** 跑一局，返回 {records, summary}；人类座由 L2 代打（与浏览器「超时托管」同一条路径）。 */
function playRound(ex, now, records) {
  for (let guard = 0; guard < 2000; guard += 1) {
    const step = JSON.parse(cstr(ex, ex.gf_room_step(BigInt(now))));
    now += 1000;
    switch (step.kind) {
      case 'applied':
        records.push({
          roundNo: step.roundNo,
          seat: step.seat,
          pass: step.action.pass,
          tributeReturn: step.action.tributeReturn ?? false,
          cards: step.action.cards ?? [],
        });
        break;
      case 'waiting': {
        // 人类座位：tribute 相位挑一张可还贡牌，其余交给 L2 probe，再以人类身份提交。
        const action = humanAction(ex, step.seat);
        const out = JSON.parse(submit(ex, step.seat, action, now));
        if (out.kind !== 'applied') throw new Error(`代打未生效：${JSON.stringify(out)}`);
        records.push({
          roundNo: out.roundNo,
          seat: out.seat,
          pass: out.action.pass,
          tributeReturn: out.action.tributeReturn ?? false,
          cards: out.action.cards ?? [],
        });
        break;
      }
      case 'settled':
      case 'matchEnd': {
        // 结算那一手由 Settled 分支带出，未走 Applied，所以要补记（否则回放少一手，无法续局）。
        const meta = JSON.parse(cstr(ex, ex.gf_room_meta()));
        if (meta.records > records.length) {
          const last = JSON.parse(cstr(ex, ex.gf_room_last_record()));
          records.push({
            roundNo: last.roundNo,
            seat: last.seat,
            pass: last.action.pass,
            cards: last.action.cards ?? [],
          });
        }
        return { summary: step.summary, records };
      }
      case 'error':
        throw new Error(`房间报错：${step.error}`);
      default:
        throw new Error(`未知 step 结果：${JSON.stringify(step)}`);
    }
  }
  throw new Error('一局动作数超上界');
}

function main() {
  // ---- 会话 A：开局 + 两局 + 存档 ----
  const exA = loadInstance();
  newRoom(exA);
  const archive = [];
  const rounds = [];
  let now = 0;
  for (let i = 0; i < 2; i += 1) {
    const { summary } = playRound(exA, now, archive);
    rounds.push(summary);
    now += 10_000;
    const next = JSON.parse(cstr(exA, exA.gf_room_next_round(BigInt(now))));
    if (next.kind !== 'dealt') throw new Error(`再来一局失败：${JSON.stringify(next)}`);
    now += 10_000;
  }
  // 第 2 局刚发完，先推进若干手，让存档停在一个「局中途」的现场（更贴近杀进程）。
  // 人类座同样由 L2 代打：发牌可能把首出判给人类座，只 step 不代打的话第一步就停在
  // Waiting，存档会落在「本局 0 手」的边界上，回放按日志最多到上一局，两边哈希必然不等。
  for (let i = 0; i < 40; i += 1) {
    const step = JSON.parse(cstr(exA, exA.gf_room_step(BigInt(now))));
    now += 1000;
    if (step.kind === 'applied') {
      archive.push({
        roundNo: step.roundNo,
        seat: step.seat,
        pass: step.action.pass,
        tributeReturn: step.action.tributeReturn ?? false,
        cards: step.action.cards ?? [],
      });
      continue;
    }
    if (step.kind === 'waiting') {
      const action = humanAction(exA, step.seat);
      const out = JSON.parse(submit(exA, step.seat, action, now));
      if (out.kind !== 'applied') throw new Error(`代打未生效：${JSON.stringify(out)}`);
      archive.push({
        roundNo: out.roundNo,
        seat: out.seat,
        pass: out.action.pass,
        tributeReturn: out.action.tributeReturn ?? false,
        cards: out.action.cards ?? [],
      });
      continue;
    }
    if (step.kind === 'settled' || step.kind === 'matchEnd') {
      const meta = JSON.parse(cstr(exA, exA.gf_room_meta()));
      if (meta.records > archive.length) {
        const last = JSON.parse(cstr(exA, exA.gf_room_last_record()));
        archive.push({
          roundNo: last.roundNo,
          seat: last.seat,
          pass: last.action.pass,
          tributeReturn: last.action.tributeReturn ?? false,
          cards: last.action.cards ?? [],
        });
      }
      break;
    }
    throw new Error(`未知 step 结果：${JSON.stringify(step)}`);
  }
  const savedHash = cstr(exA, exA.gf_room_hash());
  const savedMeta = JSON.parse(cstr(exA, exA.gf_room_meta()));
  const replayCheck = cstr(exA, exA.gf_room_replay_hash());
  if (replayCheck !== savedHash) throw new Error(`会话内回放自证失败：${replayCheck} != ${savedHash}`);

  // ---- 「杀进程」：不释放、不复用 exA，直接建新 Instance ----
  const exB = loadInstance();
  newRoom(exB);
  let currentRound = 1;
  for (const rec of archive) {
    while (rec.roundNo > currentRound) {
      const out = JSON.parse(cstr(exB, exB.gf_room_next_round(BigInt(0))));
      if (out.kind !== 'dealt') throw new Error(`回放时续局失败：${JSON.stringify(out)}`);
      currentRound += 1;
    }
    if (!replay(exB, rec.seat, rec.pass, rec.cards, rec.tributeReturn)) {
      throw new Error(`回放动作失败 @round=${rec.roundNo} seat=${rec.seat} cards=${rec.cards}`);
    }
  }
  const restoredHash = cstr(exB, exB.gf_room_hash());
  if (restoredHash !== savedHash) {
    throw new Error(`续局哈希不一致：存档=${savedHash} 恢复=${restoredHash}`);
  }
  // 恢复后必须还能继续打（不是只读快照）。
  const cont = JSON.parse(cstr(exB, exB.gf_room_step(BigInt(0))));
  if (cont.kind === 'error') throw new Error(`恢复后无法推进：${cont.error}`);

  const report = {
    ok: true,
    records: archive.length,
    rounds: rounds.map((r) => ({
      roundNo: r.roundNo,
      ranks: r.ranks,
      winnerTeam: r.winnerTeam,
      completedTeam: r.completedTeam,
      points: r.points,
    })),
    savedHash,
    restoredHash,
    meta: savedMeta,
    resumeKind: cont.kind,
  };
  console.log(JSON.stringify(report, null, 2));
}

main();
