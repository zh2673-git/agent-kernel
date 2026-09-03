//! 端到端：dlopen 装载 dylib 插件 → `PLUGIN_ABI` 守卫 → 注册进内核 → dispatch 往返。
//!
//! 前置：先构建 dylib 插件
//! ```bash
//! cargo build -p agent-kernel-echo-dylib
//! # 或：cargo run --bin xtask -- build-dylib
//! ```
//! 若产物未构建，测试打印提示后跳过（保证 `cargo test` 恒绿）。

use agent_kernel_kernel::Kernel;
use agent_kernel_sdk::{dylib, Envelope, GlobalConfig, PluginId};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn artifact_path() -> PathBuf {
    let name = if cfg!(windows) {
        "echo_dylib.dll"
    } else if cfg!(target_os = "macos") {
        "libecho_dylib.dylib"
    } else {
        "libecho_dylib.so"
    };
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/debug/{name}"))
}

#[tokio::test]
async fn dylib_roundtrip() {
    let path = artifact_path();
    if !path.exists() {
        eprintln!(
            "skip: 未找到 dylib 插件 {}，先运行 `cargo run --bin xtask -- build-dylib`",
            path.display()
        );
        return;
    }

    // ABI 守卫：不匹配的 dylib 必须被拒绝
    let loaded = dylib::load(&path).expect("load dylib plugin");
    let instance = loaded.instance();

    let kernel: Arc<Kernel> = Kernel::new(GlobalConfig {
        node_id: "e2e-dylib".into(),
        max_total_inflight: 16,
    });
    let _ = kernel.register(instance).await;

    let r = kernel
        .dispatch(Envelope::new(
            PluginId::new("echo-dylib"),
            json!({ "hello": "world" }),
        ))
        .await
        .expect("dispatch roundtrip");

    assert_eq!(r["echo"], json!({ "hello": "world" }), "dylib 插件应原样回显 payload");
    assert_eq!(r["from"], json!("dylib"));

    kernel.stop();
    kernel.destroy().await;
    // LoadedPlugin 在此 drop：FreeLibrary 卸载 dylib
}
