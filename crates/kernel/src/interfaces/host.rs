//! `HostApi` 的内核实现。插件通过它回调内核（发布事件 / 调用其他插件）。

use crate::interfaces::dispatch;
use crate::runtime::KernelInner;
use agent_kernel_core::{Envelope, Event, KernelError, Priority, TraceId};
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
        // B1/K505（v0.1.3 K3）：事件总线是可选调试通道——`Kernel::run()` 未启动时
        // 快速失败，拒绝静默积压（有界 mpsc 满后会阻塞发布者，延迟暴露更难排查）。
        if !self.inner.running.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(KernelError::EventBusNotRunning);
        }
        self.inner.bus.publish_event(event).await
    }

    async fn call_plugin(&self, env: Envelope) -> Result<Value, KernelError> {
        dispatch::dispatch(self.inner.clone(), env).await
    }

    async fn call_capability(
        &self,
        capability: &str,
        payload: Value,
        deadline: std::time::Duration,
    ) -> Result<Value, KernelError> {
        // K2（v0.1.4）：动态解析 → 复用 dispatch 全链（B5/B2/B4/deadline 全部继承）。
        // 解析每次查当前索引 → 与 hot_swap 兼容（换实现后新调用流向新实例）。
        self.call_capability_as(capability, TraceId::new(), Priority::Normal, payload, deadline)
            .await
    }

    async fn call_capability_as(
        &self,
        capability: &str,
        trace_id: TraceId,
        priority: Priority,
        payload: Value,
        deadline: std::time::Duration,
    ) -> Result<Value, KernelError> {
        // v0.1.7（S1/T8）：调用方身份（trace_id/priority）显式传入——链路串联与
        // 泳道调度在 capability 寻址路径上与 call_plugin 同权。
        let target = self.inner.cap_provider(capability)?;
        let env = Envelope {
            target,
            trace_id,
            priority,
            deadline: Some(deadline),
            payload,
        };
        dispatch::dispatch(self.inner.clone(), env).await
    }

    fn now_mono(&self) -> u64 {
        let start = START.get_or_init(Instant::now);
        start.elapsed().as_nanos() as u64
    }

    #[allow(deprecated)] // K4：trace_id 已废弃，实现保留至首个破坏性版本删除
    fn trace_id(&self) -> TraceId {
        TraceId::new()
    }
}
