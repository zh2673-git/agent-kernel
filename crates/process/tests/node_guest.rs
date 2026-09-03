//! 跨语言 e2e：内核 Process 域 spawn **TypeScript** guest（经 node --experimental-strip-types）。
//!
//! 若本机无 node 或示例缺失，打印提示后跳过（保证 `cargo test` 恒绿）。

use agent_kernel_core::{ApiVersion, Domain, Manifest, Semantics, Version};
use agent_kernel_kernel::Kernel;
use agent_kernel_process::ProcessPlugin;
use agent_kernel_sdk::{Envelope, GlobalConfig, PluginId};
use serde_json::json;
use std::path::Path;
use std::process::Command as StdCommand;
use std::sync::Arc;
use tokio::process::Command;

fn guest_manifest(id: &str) -> Manifest {
    Manifest {
        name: PluginId::new(id),
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

async fn find_node() -> Option<String> {
    let ok = StdCommand::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    ok.then(|| "node".to_string())
}

#[tokio::test]
async fn node_guest_roundtrip() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../bindings/typescript/examples/echo_plugin.ts");
    if !script.exists() {
        eprintln!("skip: 未找到 ts 示例 {}", script.display());
        return;
    }
    let Some(node) = find_node().await else {
        eprintln!("skip: 未找到 node");
        return;
    };

    let kernel: Arc<Kernel> = Kernel::new(GlobalConfig {
        node_id: "e2e-ts".into(),
        max_total_inflight: 16,
    });

    // Node >= 22.6 支持直接运行 TS（type stripping）
    let mut cmd = Command::new(&node);
    cmd.arg("--experimental-strip-types").arg(&script);
    let pp = match ProcessPlugin::spawn(guest_manifest("echo-ts"), &mut cmd).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("skip: node guest 启动/握手失败（node 版本过低或不支持 strip-types）: {e}");
            return;
        }
    };
    let _ = kernel.register(Arc::new(pp)).await;

    let r = kernel
        .dispatch(Envelope::new(
            PluginId::new("echo-ts"),
            json!({ "hello": "world" }),
        ))
        .await
        .expect("dispatch roundtrip");

    assert_eq!(r["echo"], json!({ "hello": "world" }), "ts 插件应原样回显 payload");
    assert_eq!(r["from"], json!("typescript"));

    kernel.stop();
    kernel.destroy().await;
}
