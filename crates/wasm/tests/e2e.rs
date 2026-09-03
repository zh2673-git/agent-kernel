//! 端到端：内核（wasm feature）→ 装载 WASM 组件 → 校验 meta → 注册 → dispatch 往返。
//!
//! 前置：先构建 guest 组件
//! ```bash
//! cargo build -p agent-kernel-wasm-guest --target wasm32-wasip2 --release
//! # 或：cargo run --bin xtask -- build-wasm
//! ```
//! 若组件未构建，测试打印提示后跳过（保证 `cargo test` 恒绿）。

use agent_kernel_core::{ApiVersion, Domain, Manifest, Semantics, Version};
use agent_kernel_kernel::Kernel;
use agent_kernel_sdk::{Envelope, GlobalConfig, PluginId};
use agent_kernel_wasm::WasmPlugin;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

fn echo_manifest() -> Manifest {
    Manifest {
        name: PluginId::new("echo-wasm"),
        kind: Default::default(),
        version: Version::new(0, 1, 0),
        api_version: ApiVersion::new(0, 1),
        capabilities: vec![],
        dependencies: vec![],
        domain: Domain::Wasm,
        // A3：WASM 强制 Serial（wasm + concurrent 会被 Manifest::validate 拒绝）
        semantics: Semantics::Serial,
        priority: 0,
        max_inflight: None,
        fuel_limit: Some(50_000_000), // B3-1：指令上限
        host_timeout_ms: None,
        epoch_interval_ms: Some(10), // B3-2：epoch tick
        subscriptions: vec![],
    }
}

#[tokio::test]
async fn wasm_domain_roundtrip() {
    let wasm = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target/wasm32-wasip2/release/agent_kernel_wasm_guest.wasm");
    if !wasm.exists() {
        eprintln!(
            "skip: 未找到 wasm 组件 {}，先运行 `cargo run --bin xtask -- build-wasm`",
            wasm.display()
        );
        return;
    }

    let kernel: Arc<Kernel> = Kernel::new(GlobalConfig {
        node_id: "e2e-wasm".into(),
        max_total_inflight: 16,
    });

    let plugin = WasmPlugin::load(echo_manifest(), &wasm).expect("load wasm component");
    let _ = kernel.register(Arc::new(plugin)).await;

    let r = kernel
        .dispatch(Envelope::new(
            PluginId::new("echo-wasm"),
            json!({ "hello": "world" }),
        ))
        .await
        .expect("dispatch roundtrip");

    assert_eq!(r["echo"], json!({ "hello": "world" }), "wasm guest 应原样回显 payload");

    // 第二次调用（串行语义下复用同一 Store）
    let r2 = kernel
        .dispatch(Envelope::new(PluginId::new("echo-wasm"), json!({ "n": 2 })))
        .await
        .expect("second dispatch");
    assert_eq!(r2["echo"], json!({ "n": 2 }));

    kernel.stop();
    kernel.destroy().await;
}
