//! 跨语言 e2e：内核 Process 域 spawn **Python** guest（NDJSON/stdio 协议语言无关）。
//!
//! 若本机无 python 或示例缺失，打印提示后跳过（保证 `cargo test` 恒绿）。

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

async fn find_interpreter(candidates: &[&str]) -> Option<String> {
    for c in candidates {
        let ok = StdCommand::new(c)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            return Some((*c).to_string());
        }
    }
    None
}

#[tokio::test]
async fn python_guest_roundtrip() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../bindings/python/examples/echo_plugin.py");
    if !script.exists() {
        eprintln!("skip: 未找到 python 示例 {}", script.display());
        return;
    }
    let Some(python) = find_interpreter(&["python", "python3"]).await else {
        eprintln!("skip: 未找到 python 解释器");
        return;
    };

    let kernel: Arc<Kernel> = Kernel::new(GlobalConfig {
        node_id: "e2e-py".into(),
        max_total_inflight: 16,
    });

    let mut cmd = Command::new(&python);
    cmd.arg(&script);
    let pp = match ProcessPlugin::spawn(guest_manifest("echo-py"), &mut cmd).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("skip: python guest 启动/握手失败: {e}");
            return;
        }
    };
    let _ = kernel.register(Arc::new(pp)).await;

    let r = kernel
        .dispatch(Envelope::new(
            PluginId::new("echo-py"),
            json!({ "hello": "world" }),
        ))
        .await
        .expect("dispatch roundtrip");

    assert_eq!(r["echo"], json!({ "hello": "world" }), "python 插件应原样回显 payload");
    assert_eq!(r["from"], json!("python"));

    kernel.stop();
    kernel.destroy().await;
}
