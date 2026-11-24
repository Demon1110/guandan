// M0 golden wasm 宿主：Node v22 内置 WebAssembly 裸加载，零 import 对象。
// 链路：gf_m0_golden_report() -> 48 字节堆缓冲指针 -> 按布局解码终局诊断字段。
// 缓冲布局（与 cases.rs REPORT_BUF_LEN 注释两端共用）：
//   [0..32]  stateHash 原始 32 字节
//   [32..36] steps u32 LE
//   [36..40] phase u32 LE
//   [40..44] finish_order_len u32 LE
//   [44..48] finish_order 4×u8，不足位填 0xFF
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
// here = crates/engine/tests/wasm；workspace 根向上四级。
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
// 坑①：必须先 new WebAssembly.Module(bytes)，直接调 Instance 构造器会抛
// TypeError: must be invoked with 'new'。
const module = new WebAssembly.Module(bytes);
const imports = WebAssembly.Module.imports(module);
if (imports.length > 0) {
  console.error('unexpected imports:', JSON.stringify(imports));
  process.exit(2);
}
// 空 import 对象实例化（plain Rust cdylib 零 import，无需 WASI/env shim）。
const instance = new WebAssembly.Instance(module, {});
const { memory, gf_m0_golden_report, gf_free } = instance.exports;

const ptr = gf_m0_golden_report();
if (typeof ptr !== 'number' || ptr === 0) {
  console.error('gf_m0_golden_report returned null pointer');
  process.exit(3);
}
// 坑②：读内存前重新取 memory.buffer；若中途 memory.grow 过，旧 Uint8Array 视图会 detach。
const view = new Uint8Array(memory.buffer, ptr, 48);
const dv = new DataView(view.buffer, view.byteOffset, 48);
const hash = Buffer.from(view.subarray(0, 32)).toString('hex');
const steps = dv.getUint32(32, true);
const phase = dv.getUint32(36, true);
const finishLen = dv.getUint32(40, true);
const finishOrder = [...view.subarray(44, 48)].filter((_, i) => i < finishLen);
gf_free(ptr, 48);

// m0-minimal 夹具 seats.teams = [[0,2],[1,3]]；terminalTeam 口径与 cases.rs 一致
// （§5.13 第 902 行）：全员进入 finishOrder 的队伍中，最晚名次（最大下标）最小者，
// 并列按队伍下标小者；1+4 形态下是对家队，不等于头游所在队。报告缓冲未单独传输队位。
const TEAMS = [
  [0, 2],
  [1, 3],
];
const terminalTeam = TEAMS.map((team, i) => ({ i, team }))
  .filter(({ team }) => team.every((s) => finishOrder.includes(s)))
  .map(({ i, team }) => ({
    i,
    last: Math.max(...team.map((s) => finishOrder.indexOf(s))),
  }))
  .sort((a, b) => a.last - b.last || a.i - b.i)
  .map((x) => x.i)[0] ?? -1;

console.log(
  JSON.stringify({ hash, steps, phase, finishOrder, terminalTeam }),
);
