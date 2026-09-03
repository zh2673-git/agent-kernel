//! 骨架插件：LLM 适配器。
//!
//! 演示要点：
//! - **A1**：全部方法 `&self`；内部可变性用 `Mutex`（插件自管状态）。
//! - **A3**：in-process + `Serial` 语义（执行域内部加锁，保证顺序）。
//! - 热替换：用不同 `version` 构造，dispatch 结果随版本变化，演示不停机替换。

use agent_kernel_sdk::*;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

pub struct LlmPlugin {
    version: &'static str,
    calls: Mutex<u32>,
    manifest: Manifest,
}

impl LlmPlugin {
    /// 构造一个 LLM 适配器实例（句柄即 `Arc<dyn Plugin>`）。
    pub fn new(version: &'static str) -> PluginInstance {
        let manifest = Manifest {
            name: PluginId::new("llm-adapter"),
            kind: PluginKind::Capability,
            version: Version::new(0, 1, 0),
            api_version: ApiVersion::new(1, 0),
            capabilities: vec![Capability::new("llm")],
            dependencies: vec![],
            domain: Domain::InProcess,
            semantics: Semantics::Serial,
            priority: 1,
            max_inflight: Some(8),
            fuel_limit: None,
            host_timeout_ms: None,
            epoch_interval_ms: None,
            subscriptions: vec!["chat".to_string()],
        };
        Arc::new(Self {
            version,
            calls: Mutex::new(0),
            manifest,
        })
    }
}

#[async_trait]
impl Plugin for LlmPlugin {
    fn id(&self) -> PluginId {
        self.manifest.name.clone()
    }
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    async fn init(&self, _ctx: &PluginContext) -> KernelResult<()> {
        Ok(())
    }
    async fn on_event(&self, env: Envelope) -> KernelResult<Value> {
        // 模拟一次 LLM 调用（串行语义下此临界区不会并发）
        let mut c = self.calls.lock().unwrap();
        *c += 1;
        let prompt = env.payload.get("prompt").cloned().unwrap_or(json!(null));
        Ok(json!({
            "model": self.version,
            "prompt": prompt,
            "reply": format!("echo[{}]: {}", self.version, prompt),
            "calls": *c,
        }))
    }
    fn destroy(&self) -> KernelResult<()> {
        Ok(())
    }
}
