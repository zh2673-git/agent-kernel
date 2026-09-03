//! 端到端：内核（process feature）→ spawn process-guest → 握手 → 注册 → dispatch 往返。

use agent_kernel_core::{ApiVersion, Domain, Manifest, Semantics, Version};
use agent_kernel_kernel::Kernel;
use agent_kernel_process::ProcessPlugin;
use agent_kernel_sdk::{Envelope, GlobalConfig, PluginId};
use serde_json::json;
use std::sync::Arc;
use tokio::process::Command;

fn echo_manifest() -> Manifest {
    Manifest {
        name: PluginId::new("echo-proc"),
        kind: Default::default(),
        version: Version::new(0, 1, 0),
        api_version: ApiVersion::new(0, 1),
        capabilities: vec![],
        dependencies: vec![],
        domain: Domain::Process,
        semantics: Semantics::Serial,
        priority: 0,
        max_inflight: None,
        fuel_limit: None,
        host_timeout_ms: None,
        epoch_interval_ms: None,
        subscriptions: vec![],
    }
}

#[tokio::test]
async fn process_domain_roundtrip() {
    let kernel: Arc<Kernel> = Kernel::new(GlobalConfig {
        node_id: "e2e".into(),
        max_total_inflight: 16,
    });

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_process-guest"));
    let pp = ProcessPlugin::spawn(echo_manifest(), &mut cmd)
        .await
        .expect("spawn + handshake");

    let _ = kernel.register(Arc::new(pp)).await;

    let r = kernel
        .dispatch(Envelope::new(
            PluginId::new("echo-proc"),
            json!({ "hello": "world" }),
        ))
        .await
        .expect("dispatch roundtrip");

    assert_eq!(r["echo"], json!({ "hello": "world" }), "子进程应原样回显 payload");
    assert!(r["pid"].as_u64().is_some(), "响应应携带 guest 进程 pid");

    // 第二次分发（串行语义下的重复调用）
    let r2 = kernel
        .dispatch(Envelope::new(PluginId::new("echo-proc"), json!({ "n": 2 })))
        .await
        .expect("second dispatch");
    assert_eq!(r2["echo"], json!({ "n": 2 }));

    kernel.stop();
    kernel.destroy().await;
}
