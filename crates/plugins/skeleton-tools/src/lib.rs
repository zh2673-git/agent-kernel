//! 骨架插件：工具注册表。
//!
//! 演示要点：
//! - **A1**：`&self` + 内部 `Mutex` 状态（工具表）。
//! - **跨插件通信一律走 HostApi**（规则契约：插件间禁止直接内存互访）。
//!   收到 `action == "call-llm"` 时通过 `ctx.kernel.host.call_plugin` 转发到 `llm-adapter`。
//!   host 在 `init` 时从 `PluginContext` 捕获并保存（&self 下用 OnceLock）。

use agent_kernel_sdk::*;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, OnceLock};

pub struct ToolsPlugin {
    tools: Mutex<Vec<String>>,
    host: OnceLock<Arc<dyn HostApi>>,
    manifest: Manifest,
}

impl ToolsPlugin {
    pub fn new() -> PluginInstance {
        let manifest = Manifest {
            name: PluginId::new("tools-registry"),
            kind: PluginKind::Capability,
            version: Version::new(0, 1, 0),
            api_version: ApiVersion::new(1, 0),
            capabilities: vec![Capability::new("tools")],
            dependencies: vec![DependencySpec {
                capability: Capability::new("llm"),
                hard: false, // 软依赖：llm 缺失可补挂（B1）
            }],
            domain: Domain::InProcess,
            semantics: Semantics::Serial,
            priority: 1,
            max_inflight: Some(8),
            fuel_limit: None,
            host_timeout_ms: None,
            epoch_interval_ms: None,
            subscriptions: vec![], // 由内核按 action 路由，不订阅事件类型
        };
        Arc::new(Self {
            tools: Mutex::new(vec!["search".into(), "calc".into()]),
            host: OnceLock::new(),
            manifest,
        })
    }
}

#[async_trait]
impl Plugin for ToolsPlugin {
    fn id(&self) -> PluginId {
        self.manifest.name.clone()
    }
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    async fn init(&self, ctx: &PluginContext) -> KernelResult<()> {
        // A1：&self 下捕获 host（OnceLock 幂等）
        let _ = self.host.set(ctx.kernel.host.clone());
        Ok(())
    }
    async fn on_event(&self, env: Envelope) -> KernelResult<Value> {
        let action = env.payload.get("action").and_then(|v| v.as_str()).unwrap_or("");
        match action {
            "list-tools" => {
                let tools = self.tools.lock().unwrap().clone();
                Ok(json!({ "tools": tools }))
            }
            "call-llm" => {
                let host = self
                    .host
                    .get()
                    .ok_or_else(|| KernelError::Internal("tools: host not initialized".into()))?;
                let mut fwd = env.clone();
                fwd.target = PluginId::new("llm-adapter");
                fwd.payload = json!({ "prompt": env.payload.get("prompt").cloned().unwrap_or(json!("hi")) });
                host.call_plugin(fwd).await
            }
            _ => Ok(json!({ "ok": true })),
        }
    }
    fn destroy(&self) -> KernelResult<()> {
        Ok(())
    }
}
