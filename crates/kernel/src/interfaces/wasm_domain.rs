//! WASM 执行域适配器（feature = "wasm"，03 §2.6 / 04 D2）。
//!
//! 实际装载与调用由 `agent-kernel-wasm::WasmPlugin` 承担（它实现 `trait Plugin`）。
//! 本适配器与 `InProcessDomain` 同构，但 **WASM 域强制 Serial**（A3）：
//! 每个插件一个 `Store` ⇒ 独立线性内存，且 host call await 期间 `Store` 不可重入，
//! 故同时只能有 1 个 in-flight 调用；`Manifest::validate` 已在装载期拒绝
//! `wasm + concurrent` 的组合（K202，不做静默降级）。

use super::domains::ExecutionDomain;
use agent_kernel_core::{Domain as DomainKind, Envelope, KernelError, PluginId};
use agent_kernel_sdk::PluginInstance;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

pub struct WasmDomain {
    locks: Mutex<HashMap<PluginId, Arc<AsyncMutex<()>>>>,
}

impl WasmDomain {
    pub fn new() -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
        }
    }
    fn lock_for(&self, id: &PluginId) -> Arc<AsyncMutex<()>> {
        self.locks
            .lock()
            .unwrap()
            .entry(id.clone())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }
}

impl Default for WasmDomain {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExecutionDomain for WasmDomain {
    fn domain(&self) -> DomainKind {
        DomainKind::Wasm
    }
    async fn dispatch(&self, plugin: &PluginInstance, env: Envelope) -> Result<Value, KernelError> {
        // A3：WASM 恒串行（不读 manifest.semantics，避免与装载期校验不一致）
        let lock = self.lock_for(&env.target);
        let _g = lock.lock().await;
        plugin.on_event(env).await
    }
}
