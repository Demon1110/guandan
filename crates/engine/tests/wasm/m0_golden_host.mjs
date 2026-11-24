// M0 golden wasm 宿主：Node v22 内置 WebAssembly 裸加载，零 import 对象。
// 链路：gf_m0_golden_report() -> 64 字节堆缓冲指针 -> 按布局解码终局诊断字段。
// 缓冲布局（与 cases.rs REPORT_BUF_LEN 注释两端共用，小端序）：
//   [0..4]   abiVersion u32 LE（2 = 64 字节布局；1 = 旧 48 字节布局）
//   [4..5]   completedTeam u8，0xFF = 无
//   [5..6]   winnerTeam u8，0xFF = 无
//   [6..8]   reserved u16 LE，恒 0
//   [8..40]  stateHash 原始 32 字节
//   [40..44] steps u32 LE
//   [44..48] phase u32 LE
//   [48..52] finishOrderLen u32 LE
//   [52..56] finishOrder 4×u8，不足位填 0xFF
//   [56..60] discardCount u32 LE
//   [60..64] handsTotal u32 LE
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
const view = new Uint8Array(memory.buffer, ptr, 64);
const dv = new DataView(view.buffer, view.byteOffset, 64);

const abiVersion = dv.getUint32(0, true);
if (abiVersion !== 2) {
  console.error(`abiVersion 不兼容：期望 2（64 字节布局），实际 ${abiVersion}；拒绝按旧布局静默读取`);
  gf_free(ptr, 64);
  process.exit(4);
}

const NULL_TEAM = 0xff;
const completedTeam = view[4] === NULL_TEAM ? null : view[4];
const winnerTeam = view[5] === NULL_TEAM ? null : view[5];
const reserved = dv.getUint16(6, true);
const hash = Buffer.from(view.subarray(8, 40)).toString('hex');
const steps = dv.getUint32(40, true);
const phase = dv.getUint32(44, true);
const finishLen = dv.getUint32(48, true);
const finishOrder = [...view.subarray(52, 56)].filter((_, i) => i < finishLen);
const discardCount = dv.getUint32(56, true);
const handsTotal = dv.getUint32(60, true);
gf_free(ptr, 64);

console.log(
  JSON.stringify({
    abiVersion,
    completedTeam,
    winnerTeam,
    reserved,
    hash,
    steps,
    phase,
    finishOrder,
    discardCount,
    handsTotal,
  }),
);
