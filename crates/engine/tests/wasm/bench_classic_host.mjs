// classic 满牌型基准宿主（§10.5 第 1505 行）：Node `process.hrtime.bigint()` 计时，
// 批处理口径与 native `classic_bench.rs` 对照。每批 1000 次 × 100 批 = 10 万次；
// 单次 µs = 批耗时 ns / 1000(ns→µs) / 批内次数；分位数在「批均值」上取（最近秩法）。
//
// 口径差异：native bench 是逐次样本取分位数，wasm 侧只有批均值，两端不可按
// 分位数绝对值严格对齐，回报中写明。本宿主只做计时，不改 ABI、不动 golden 宿主。
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = join(
  here,
  '..',
  '..',
  '..',
  '..',
  'target',
  'wasm32-unknown-unknown',
  'release',
  'gf_engine.wasm',
);

const bytes = readFileSync(wasmPath);
const module = new WebAssembly.Module(bytes);
const imports = WebAssembly.Module.imports(module);
if (imports.length > 0) {
  console.error('unexpected imports:', JSON.stringify(imports));
  process.exit(2);
}
const instance = new WebAssembly.Instance(module, {});
const {
  gf_bench_classic_legal_actions,
  gf_bench_classic_legal_actions_lead,
  gf_bench_classic_apply,
} = instance.exports;

const BATCH = 1000;
const BATCHES = 100;

function runBatches(fn) {
  const means = [];
  let checksum = 0;
  for (let i = 0; i < BATCHES; i++) {
    const t0 = process.hrtime.bigint();
    const c = fn(BATCH);
    const t1 = process.hrtime.bigint();
    checksum += Number(c);
    means.push(Number(t1 - t0) / 1000 / BATCH);
  }
  return { means, checksum };
}

function percentile(arr, p) {
  const s = [...arr].sort((a, b) => a - b);
  const n = s.length;
  const rank = Math.ceil((p / 100) * n);
  return s[Math.min(n, Math.max(1, rank)) - 1];
}

function summarize(name, result, { candidates = false } = {}) {
  const avg = result.means.reduce((a, b) => a + b, 0) / result.means.length;
  const out = {
    name,
    iterations: BATCH * BATCHES,
    batches: BATCHES,
    avg,
    p50: percentile(result.means, 50),
    p90: percentile(result.means, 90),
    p99: percentile(result.means, 99),
    max: Math.max(...result.means),
    checksum: result.checksum,
  };
  if (candidates) {
    out.candidatesPerCall = result.checksum / (BATCH * BATCHES);
  }
  return out;
}

const legalMid = runBatches(gf_bench_classic_legal_actions);
const legalLead = runBatches(gf_bench_classic_legal_actions_lead);
const apply = runBatches(gf_bench_classic_apply);

const memoryBytes = instance.exports.memory
  ? instance.exports.memory.buffer.byteLength
  : 0;

// 容量推算同 native 口径：10Hz、20 动作/秒/房间，lead legal + apply 均值。
const leadAvg = legalLead.means.reduce((a, b) => a + b, 0) / legalLead.means.length;
const applyAvg = apply.means.reduce((a, b) => a + b, 0) / apply.means.length;
const avgAction = (leadAvg + applyAvg) / 2;
const capacityRooms = Math.round(1_000_000 / (20 * avgAction));

console.log(
  JSON.stringify({
    legalMid: summarize('classic legal@mid', legalMid, { candidates: true }),
    legalLead: summarize('classic legal@lead', legalLead, { candidates: true }),
    apply: summarize('classic apply', apply),
    capacity: {
      avgAction,
      leadLegalAvg: leadAvg,
      applyAvg,
      rooms: capacityRooms,
    },
    wasmMemoryBytes: memoryBytes,
  }),
);
