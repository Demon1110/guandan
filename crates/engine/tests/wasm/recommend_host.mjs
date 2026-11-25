// 组牌推荐同解宿主：加载 wasm，调 `gf_recommend_digest_lo/hi` 还原 u64 摘要，
// 与 native `recommend_golden` example 的 `recommendDigest` 逐值比对。
// 独立探针：不碰既有 64 字节 golden ABI，不加字段到旧报告。
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
const { gf_recommend_digest_lo, gf_recommend_digest_hi } = instance.exports;

const lo = BigInt(gf_recommend_digest_lo()) & 0xffffffffn;
const hi = BigInt(gf_recommend_digest_hi()) & 0xffffffffn;
const digest = (hi << 32n) | lo;

console.log(`recommendDigest: ${digest}`);
