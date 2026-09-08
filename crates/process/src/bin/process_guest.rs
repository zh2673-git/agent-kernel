//! 示例 guest：echo 插件跑在独立进程里，供 e2e 测试与人工验证。
//! 运行后被内核 spawn，经 gRPC（`schema/kernel.proto`）与内核通信。

use agent_kernel_core::{ApiVersion, Domain, KernelResult, Manifest, PluginId, Semantics, Version};
use agent_kernel_sdk::{Envelope, Plugin, PluginInstance};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

struct EchoPlugin {
    manifest: Manifest,
}

impl EchoPlugin {
    fn new() -> Self {
        Self {
            manifest: Manifest {
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
            },
        }
    }
}

#[async_trait]
impl Plugin for EchoPlugin {
    fn id(&self) -> PluginId {
        self.manifest.name.clone()
    }
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    async fn init(&self, _ctx: &agent_kernel_sdk::PluginContext) -> KernelResult<()> {
        Ok(())
    }
    async fn on_event(&self, env: Envelope) -> KernelResult<Value> {
        // 支持 `{"delay_ms": n}`：人为延迟后回显（并发语义 e2e 用）。
        if let Some(d) = env.payload.get("delay_ms").and_then(|v| v.as_u64()) {
            tokio::time::sleep(std::time::Duration::from_millis(d)).await;
        }
        Ok(json!({ "echo": env.payload, "pid": std::process::id() }))
    }
    fn destroy(&self) -> KernelResult<()> {
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let plugin: PluginInstance = Arc::new(EchoPlugin::new());
    if let Err(e) = agent_kernel_process::guest::serve(plugin).await {
        eprintln!("process-guest error: {e}");
        std::process::exit(1);
    }
}
