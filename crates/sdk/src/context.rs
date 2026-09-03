use crate::{Event, HostApi, KernelError, Manifest, PluginState};
use std::sync::Arc;
use tokio::sync::watch;

/// 全局配置（内核只读共享部分）。
#[derive(Clone, Debug, Default)]
pub struct GlobalConfig {
    pub node_id: String,
    pub max_total_inflight: usize,
}

/// 插件私有配置（来自清单 + 部署覆盖）。
#[derive(Clone, Debug, Default)]
pub struct PluginConfig {
    pub raw: serde_json::Value,
}

/// 只读共享内核上下文（空间契约：插件只持有不可变句柄）。
pub struct KernelContext {
    pub host: Arc<dyn HostApi>,
    pub global: GlobalConfig,
}

impl KernelContext {
    pub fn new(host: Arc<dyn HostApi>, global: GlobalConfig) -> Self {
        Self { host, global }
    }
}

/// 插件上下文（空间契约：插件独占自身状态/配置/资源句柄）。
///
/// `state` 是 `watch` 接收端——订阅即得当前 `PluginState`（B1：状态类用 watch，
/// 可重放，消除 broadcast 的 Lagged 静默丢状态风险）。`config` 是插件私有配置。
pub struct PluginContext {
    pub manifest: Manifest,
    pub config: PluginConfig,
    pub state: watch::Receiver<PluginState>,
    pub kernel: KernelContext,
}

impl PluginContext {
    pub fn new(
        manifest: Manifest,
        config: PluginConfig,
        state: watch::Receiver<PluginState>,
        kernel: KernelContext,
    ) -> Self {
        Self { manifest, config, state, kernel }
    }

    /// 读取当前生命周期状态（非阻塞，reconcile 用）。
    pub fn current_state(&self) -> PluginState {
        *self.state.borrow()
    }

    /// 便捷：发布事件。
    pub async fn emit(&self, event: Event) -> Result<(), KernelError> {
        self.kernel.host.emit(event).await
    }
}
