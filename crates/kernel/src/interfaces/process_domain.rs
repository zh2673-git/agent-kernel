//! Process 执行域适配器（feature = "process"，03 §2.6 / 04 D3）。
//!
//! 实际 IPC 由 `agent-kernel-process::ProcessPlugin` 承担：它实现 `trait Plugin`，
//! `on_event` 内部经 stdio NDJSON 与子进程往返（seq 关联）。本适配器与
//! `InProcessDomain` 同构，只负责 TimeSemantics（Serial 语义载体 = 插件级锁，A3）。

use super::domains::ExecutionDomain;
use agent_kernel_core::{Domain as DomainKind, Envelope, KernelError, PluginId};
use agent_kernel_sdk::PluginInstance;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

pub struct ProcessDomain {
    serial: bool,
    locks: Mutex<HashMap<PluginId, Arc<AsyncMutex<()>>>>,
}

impl ProcessDomain {
    pub fn new(serial: bool) -> Self {
        Self {
            serial,
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

#[async_trait]
impl ExecutionDomain for ProcessDomain {
    fn domain(&self) -> DomainKind {
        DomainKind::Process
    }
    async fn dispatch(&self, plugin: &PluginInstance, env: Envelope) -> Result<Value, KernelError> {
        let lock = self.lock_for(&env.target);
        let _g = if self.serial {
            Some(lock.lock().await)
        } else {
            None
        };
        plugin.on_event(env).await
    }
}
