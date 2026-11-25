// 双产物（native / wasm）同解比对：同一 seed 跑「四 bot 两局」，wasm 侧输出终局 stateHash。
//
// native 侧由 `crates/ai/examples/local_match.rs` 打印 `fullHash=`；两侧相等即证明
// 房间主干 + AI-L1 bot + 结算在 wasm 目标上与 native 逐字节同解。
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = join(here, '..', '..', 'target', 'wasm32-unknown-unknown', 'release', 'gf_client_wasm.wasm');

function loadInstance() {
  const bytes = readFileSync(wasmPath);
  const module = new WebAssembly.Module(bytes);
  if (WebAssembly.Module.imports(module).length > 0) throw new Error('零 import 约定被破坏');
  return new WebAssembly.Instance(module, {}).exports;
}

function cstr(ex, ptr) {
  const view = new Uint8Array(ex.memory.buffer, ptr);
  let end = 0;
  while (end < view.length && view[end] !== 0) end += 1;
  const text = new TextDecoder().decode(view.subarray(0, end));
  ex.gf_str_free(ptr);
  return text;
}

const seed = Number(process.argv[2] ?? 0x51eda101);
const seedHi = Math.floor(seed / 0x100000000);
const seedLo = seed >>> 0;

const ex = loadInstance();
if (ex.gf_room_new(seedHi, seedLo, 0) !== 1) throw new Error('建桌失败');
if (ex.gf_room_start(0n) !== 1) throw new Error('发牌失败');

let now = 0;
let records = 0;
for (let round = 1; round <= 2; round += 1) {
  for (let guard = 0; guard < 3000; guard += 1) {
    const step = JSON.parse(cstr(ex, ex.gf_room_step(BigInt(now))));
    now += 1000;
    if (step.kind === 'applied') {
      records += 1;
      continue;
    }
    if (step.kind === 'settled') {
      if (round === 1) {
        const out = JSON.parse(cstr(ex, ex.gf_room_next_round(BigInt(now))));
        if (out.kind !== 'dealt') throw new Error(`再来一局失败：${JSON.stringify(out)}`);
        now += 1000;
      }
      break;
    }
    if (step.kind === 'waiting') throw new Error(`纯 bot 局不得等待人类：${JSON.stringify(step)}`);
    if (step.kind === 'matchEnd') throw new Error('场次提前终止');
    throw new Error(`未知结果：${JSON.stringify(step)}`);
  }
}

const meta = JSON.parse(cstr(ex, ex.gf_room_meta()));
const hash = cstr(ex, ex.gf_room_hash());
const replayHash = cstr(ex, ex.gf_room_replay_hash());
if (hash !== replayHash) throw new Error(`wasm 回放自证失败：${replayHash} != ${hash}`);

console.log(JSON.stringify({ hash, records: meta.records, settledRecords: records, roundNo: meta.roundNo }, null, 2));
