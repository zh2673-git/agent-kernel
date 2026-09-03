//! `HostApi` 的内核实现。插件通过它回调内核（发布事件 / 调用其他插件）。

use crate::interfaces::dispatch;
use crate::runtime::KernelInner;
use agent_kernel_core::{Envelope, Event, KernelError, TraceId};
use agent_kernel_sdk::HostApi;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

pub struct KernelHost {
    pub(crate) inner: Arc<KernelInner>,
}

#[async_trait]
impl HostApi for KernelHost {
    async fn emit(&self, event: Event) -> Result<(), KernelError> {
        self.inner.bus.publish_event(event).await
    }

    async fn call_plugin(&self, env: Envelope) -> Result<Value, KernelError> {
        dispatch::dispatch(self.inner.clone(), env).await
    }

    fn now_mono(&self) -> u64 {
        let start = START.get_or_init(Instant::now);
        start.elapsed().as_nanos() as u64
    }

    fn trace_id(&self) -> TraceId {
        TraceId::new()
    }
}
