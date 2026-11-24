// classic 满牌型基准宿主（§10.5 第 1505 行）：Node `process.hrtime.bigint()` 逐次计时，
// 与 native `classic_bench.rs` 同一口径——每个动作一次样本，分位数在逐次样本上取
// （最近秩法）。100,000 次/侧。
//
// apply 对齐 native `bench_apply`：`legal_actions` 枚举与选动作在 prepare 出口完成、
// 不进 commit 计时窗；commit 只计 `apply` 本体。
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
  gf_bench_classic_legal_mid_once,
  gf_bench_classic_legal_lead_once,
  gf_bench_classic_apply_prepare_once,
  gf_bench_classic_apply_commit_once,
} = instance.exports;

const ITERATIONS = 100_000;

function runLegal(fn) {
  const samples = [];
  let checksum = 0;
  for (let i = 0; i < ITERATIONS; i++) {
    const t0 = process.hrtime.bigint();
    const c = fn();
    const t1 = process.hrtime.bigint();
    checksum += Number(c);
    samples.push(Number(t1 - t0) / 1000);
  }
  return { samples, checksum };
}

function runApply(prepare, commit) {
  const samples = [];
  let checksum = 0;
  for (let i = 0; i < ITERATIONS; i++) {
    prepare();
    const t0 = process.hrtime.bigint();
    const c = commit();
    const t1 = process.hrtime.bigint();
    checksum += Number(c);
    samples.push(Number(t1 - t0) / 1000);
  }
  return { samples, checksum };
}

function percentile(arr, p) {
  const s = [...arr].sort((a, b) => a - b);
  const n = s.length;
  const rank = Math.ceil((p / 100) * n);
  return s[Math.min(n, Math.max(1, rank)) - 1];
}

function summarize(name, result, { candidates = false } = {}) {
  const avg = result.samples.reduce((a, b) => a + b, 0) / result.samples.length;
  const out = {
    name,
    iterations: ITERATIONS,
    avg,
    p50: percentile(result.samples, 50),
    p90: percentile(result.samples, 90),
    p99: percentile(result.samples, 99),
    max: Math.max(...result.samples),
    checksum: result.checksum,
  };
  if (candidates) {
    out.candidatesPerCall = result.checksum / ITERATIONS;
  }
  return out;
}

const legalMid = runLegal(gf_bench_classic_legal_mid_once);
const legalLead = runLegal(gf_bench_classic_legal_lead_once);
const apply = runApply(
  gf_bench_classic_apply_prepare_once,
  gf_bench_classic_apply_commit_once,
);

const memoryBytes = instance.exports.memory
  ? instance.exports.memory.buffer.byteLength
  : 0;

// 容量推算同 native 口径：10Hz、20 动作/秒/房间，lead legal + apply 均值。
const leadAvg = legalLead.samples.reduce((a, b) => a + b, 0) / legalLead.samples.length;
const applyAvg = apply.samples.reduce((a, b) => a + b, 0) / apply.samples.length;
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
      applyAvg: applyAvg,
      rooms: capacityRooms,
    },
    wasmMemoryBytes: memoryBytes,
  }),
);
