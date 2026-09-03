//! 执行域适配器（混合多域统一抽象，03 §2.6 / 05）。
//!
//! - `InProcess`：进程内 Trait 快车道，全量实现（A1 `&self`、串行语义内部加锁）。
//! - `Wasm` / `Process`：跨语言通用道，结构已就位；实际绑定（wit / gRPC stub）在
//!   启用对应 feature 后接入。默认构建返回 `DomainUnavailable`，保证主链路可编译、可测试。

use crate::domain::scheduler::SchedulerRef;
use agent_kernel_core::{Domain as DomainKind, Envelope, KernelError, Manifest, PluginId, Semantics};
use agent_kernel_sdk::PluginInstance;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;
use serde_json::Value;

#[async_trait]
pub trait ExecutionDomain: Send + Sync {
    fn domain(&self) -> DomainKind;
    async fn dispatch(&self, plugin: &PluginInstance, env: Envelope) -> Result<Value, KernelError>;
}

/// 进程内快车道。A3：WASM 由装载期强制 Serial；in-process 按 manifest 决定串行/并发。
pub struct InProcessDomain {
    serial: bool,
    /// 串行语义载体：每插件一个 actor 式互斥锁（A3 / 03 §2.6）。
    locks: Mutex<HashMap<PluginId, Arc<AsyncMutex<()>>>>,
}

impl InProcessDomain {
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
impl ExecutionDomain for InProcessDomain {
    fn domain(&self) -> DomainKind {
        DomainKind::InProcess
    }
    async fn dispatch(&self, plugin: &PluginInstance, env: Envelope) -> Result<Value, KernelError> {
        // 串行语义：持锁跨 await（A3）。锁的 Arc 必须在此作用域存活，否则守卫悬垂。
        let lock = self.lock_for(&env.target);
        let _g = if self.serial {
            Some(lock.lock().await)
        } else {
            None
        };
        plugin.on_event(env).await
    }
}

/// 依据清单选择执行域适配器。
pub fn domain_for(m: &Manifest, _sched: SchedulerRef) -> Result<Arc<dyn ExecutionDomain>, KernelError> {
    match m.domain {
        DomainKind::InProcess => {
            let d: Arc<dyn ExecutionDomain> = Arc::new(InProcessDomain::new(m.semantics == Semantics::Serial));
            Ok(d)
        }
        DomainKind::Wasm => domain_wasm(m),
        DomainKind::Process => domain_process(m),
    }
}

#[cfg(feature = "wasm")]
fn domain_wasm(_m: &Manifest) -> Result<Arc<dyn ExecutionDomain>, KernelError> {
    // 装载与调用由 agent-kernel-wasm::WasmPlugin（wasmtime 组件模型）承担；
    // 本适配器强制 Serial（A3）。
    Ok(Arc::new(super::wasm_domain::WasmDomain::new()))
}

#[cfg(not(feature = "wasm"))]
fn domain_wasm(_m: &Manifest) -> Result<Arc<dyn ExecutionDomain>, KernelError> {
    Err(KernelError::DomainUnavailable("wasm".into()))
}

#[cfg(feature = "process")]
fn domain_process(m: &Manifest) -> Result<Arc<dyn ExecutionDomain>, KernelError> {
    // M1：NDJSON/stdio 传输（消息结构镜像 schema/kernel.proto）；gRPC（tonic）为同协议升级。
    Ok(Arc::new(super::process_domain::ProcessDomain::new(
        m.semantics == Semantics::Serial,
    )))
}

#[cfg(not(feature = "process"))]
fn domain_process(_m: &Manifest) -> Result<Arc<dyn ExecutionDomain>, KernelError> {
    Err(KernelError::DomainUnavailable("process".into()))
}
