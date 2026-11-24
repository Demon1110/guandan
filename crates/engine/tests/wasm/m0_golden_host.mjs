// M0 golden wasm 宿主：Node v22 内置 WebAssembly 裸加载，零 import 对象。
// 链路：gf_m0_golden_hash() -> 32 字节堆缓冲指针 -> 按指针读 stateHash。
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
const { memory, gf_m0_golden_hash, gf_free } = instance.exports;

const ptr = gf_m0_golden_hash();
if (typeof ptr !== 'number' || ptr === 0) {
  console.error('gf_m0_golden_hash returned null pointer');
  process.exit(3);
}
// 坑②：读内存前重新取 memory.buffer；若中途 memory.grow 过，旧 Uint8Array 视图会 detach。
const view = new Uint8Array(memory.buffer, ptr, 32);
const hash = Buffer.from(view).toString('hex');
gf_free(ptr, 32);

console.log(hash);
