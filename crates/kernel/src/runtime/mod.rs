//! 时空管家（runtime）：`Kernel` 生命周期（init/start/stop/destroy）+ 主事件循环。

use crate::domain::scheduler::SchedulerRef;
use crate::domain::{dependency, hotswap::HotSwapCoordinator, lifecycle::LifecycleManager, registry::Registry, scheduler::Scheduler};
use crate::infrastructure::bus::EventBus;
use crate::interfaces::{dispatch, KernelHost};
use agent_kernel_core::{Capability, KernelError, PluginId};
use agent_kernel_sdk::{Envelope, GlobalConfig, PluginInstance, Priority};
use arc_swap::ArcSwap;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::Notify;

pub struct KernelInner {
    pub(crate) registry: Registry,
    pub(crate) lifecycle: LifecycleManager,
    pub(crate) deps: Mutex<dependency::DependencyGraph>,
    /// K2（v0.1.4）：capability → 提供者索引（读侧无锁 ArcSwap；注册/卸载时全量重建）。
    pub(crate) cap_index: Arc<ArcSwap<HashMap<Capability, PluginId>>>,
    pub(crate) scheduler: SchedulerRef,
    pub(crate) hotswap: HotSwapCoordinator,
    pub(crate) bus: EventBus,
    pub config: GlobalConfig,
    pub(crate) running: AtomicBool,
    pub(crate) stop_notify: Arc<Notify>,
    host: OnceLock<Arc<dyn agent_kernel_sdk::HostApi>>,
    pub(crate) subscriptions: Mutex<HashMap<String, Vec<PluginId>>>,
}

impl KernelInner {
    pub(crate) fn new(bus: EventBus, config: GlobalConfig) -> Self {
        Self {
            registry: Registry::new(),
            lifecycle: LifecycleManager::new(bus.lifecycle_tx()),
            deps: Mutex::new(dependency::DependencyGraph::new()),
            cap_index: Arc::new(ArcSwap::from_pointee(HashMap::new())),
            scheduler: Scheduler::new(config.max_total_inflight),
            hotswap: HotSwapCoordinator::new(),
            bus,
            config,
            running: AtomicBool::new(false),
            stop_notify: Arc::new(Notify::new()),
            host: OnceLock::new(),
            subscriptions: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn set_host(&self, host: Arc<dyn agent_kernel_sdk::HostApi>) {
        let _ = self.host.set(host);
    }

    pub(crate) fn host(&self) -> &Arc<dyn agent_kernel_sdk::HostApi> {
        self.host.get().expect("host not initialized")
    }

    /// K2（v0.1.4）：capability → 提供者。动态解析（每次调用查当前索引）
    /// → 与 hot_swap 兼容（换实现后新调用自动流向新实例）。
    pub(crate) fn cap_provider(&self, capability: &str) -> Result<PluginId, KernelError> {
        self.cap_index
            .load()
            .get(&Capability::new(capability))
            .cloned()
            .ok_or_else(|| KernelError::UnknownCapability(capability.to_string()))
    }
}

/// 内核句柄（薄包装，内部状态在 `Arc<KernelInner>`）。
pub struct Kernel {
    pub(crate) inner: Arc<KernelInner>,
}

impl Kernel {
    /// 构造内核并完成自举（建立 HostApi 自引用闭环）。
    pub fn new(config: GlobalConfig) -> Arc<Self> {
        let bus = EventBus::new();
        let inner = Arc::new(KernelInner::new(bus, config));
        let host: Arc<dyn agent_kernel_sdk::HostApi> = Arc::new(KernelHost { inner: inner.clone() });
        inner.set_host(host);
        Arc::new(Kernel { inner })
    }

    /// 注册一个已构造的插件实例。
    pub async fn register(&self, p: PluginInstance) {
        if let Err(e) = self.inner.register_instance(p).await {
            tracing::error!("register failed: {e}");
        }
    }

    /// 直接分发（供测试 / 同步调用）。
    pub async fn dispatch(&self, env: Envelope) -> Result<Value, agent_kernel_core::KernelError> {
        dispatch::dispatch(self.inner.clone(), env).await
    }

    /// 热替换：用新实例替换同名插件（不停机）。
    pub async fn hot_swap(&self, id: PluginId, p: PluginInstance) {
        if let Err(e) = self.inner.hot_swap(&id, p).await {
            tracing::error!("hot_swap failed: {e}");
        }
    }

    /// 启动主事件循环（时间流）。事件按订阅路由到插件。
    ///
    /// **可选通道**（v0.1.3 K3）：本循环只服务 EventBus 订阅分发；同步 dispatch
    /// （`Kernel::dispatch` / `HostApi::call_plugin`）不依赖它。不调用 `run()` 时
    /// 内核仍是功能完整的调度器，仅 `emit` 会快速失败（K505）。
    pub async fn run(self: Arc<Self>) {
        let mut rx = self.inner.bus.take_event_rx();
        self.inner.running.store(true, Ordering::SeqCst);
        loop {
            tokio::select! {
                maybe = rx.recv() => {
                    match maybe {
                        Some(event) => {
                            let subs = self
                                .inner
                                .subscriptions
                                .lock()
                                .unwrap()
                                .get(&event.ty)
                                .cloned()
                                .unwrap_or_default();
                            for id in subs {
                                let env = Envelope {
                                    target: id,
                                    trace_id: event.trace_id,
                                    priority: Priority::Normal,
                                    deadline: None,
                                    payload: event.payload.clone(),
                                };
                                let inner = self.inner.clone();
                                tokio::spawn(async move {
                                    let _ = dispatch::dispatch(inner, env).await;
                                });
                            }
                        }
                        None => break,
                    }
                }
                _ = self.inner.stop_notify.notified() => break,
            }
        }
        self.inner.running.store(false, Ordering::SeqCst);
    }

    /// 暂停时间流（停止接收新事件）。
    pub fn stop(&self) {
        self.inner.stop_notify.notify_waiters();
        self.inner.running.store(false, Ordering::SeqCst);
    }

    /// 销毁：按注册表卸载全部插件（空间回收，对应 destroy）。
    pub async fn destroy(&self) {
        for id in self.inner.registry.ids() {
            let _ = self.inner.unload(&id).await;
        }
    }
}
