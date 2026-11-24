//! 构建期：用 prost-build 把 `proto/gf/v1.proto` 编译为 crate 内受版本控制的生成代码。
//!
//! 产物固定落到 `src/generated/`（不进 OUT_DIR 临时目录），这样服务端与 wasm 客户端
//! 共用仓库里同一份生成字节（§2.4 第 184 行：服务端与 wasm 共用同一份生成代码）。
//!
//! 不新增依赖：protoc 取系统 PATH 上的 libprotoc（必要时可经 `PROTOC` 环境变量指定）。

use std::path::PathBuf;

fn main() {
    let proto = "proto/gf/v1.proto";
    println!("cargo:rerun-if-changed={proto}");

    let out_dir = PathBuf::from("src/generated");
    std::fs::create_dir_all(&out_dir).expect("create crates/protocol/src/generated");

    prost_build::Config::new()
        .out_dir(&out_dir)
        .compile_protos(&[proto], &["proto"])
        .expect("prost-build: compile proto/gf/v1.proto");
}
