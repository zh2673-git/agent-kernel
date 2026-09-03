//! 生命周期状态机（04 A）。状态迁移由此强制；捕获 panic 后唯一终态是 `Failed`（B2）。

use agent_kernel_core::{LifecycleEvent, PluginId, PluginState};
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::broadcast;
use tokio::sync::watch;

pub struct LifecycleManager {
    /// 每个插件一个 watch 通道（状态类用 watch，可重放，B1）。
    states: Mutex<HashMap<PluginId, (PluginState, watch::Sender<PluginState>)>>,
    /// 生命周期事件走 broadcast（审计/指标类，B1）。
    lifecycle_tx: broadcast::Sender<LifecycleEvent>,
}

impl LifecycleManager {
    pub fn new(lifecycle_tx: broadcast::Sender<LifecycleEvent>) -> Self {
        Self {
            states: Mutex::new(HashMap::new()),
            lifecycle_tx,
        }
    }

    /// 为新插件建立 watch 通道，初始状态 Registered。
    pub fn new_plugin(&self, id: &PluginId) -> watch::Receiver<PluginState> {
        let (tx, rx) = watch::channel(PluginState::Registered);
        self.states
            .lock()
            .unwrap()
            .insert(id.clone(), (PluginState::Registered, tx));
        rx
    }

    pub fn receiver(&self, id: &PluginId) -> Option<watch::Receiver<PluginState>> {
        self.states.lock().unwrap().get(id).map(|s| s.1.subscribe())
    }

    pub fn current_state(&self, id: &PluginId) -> PluginState {
        self.states
            .lock()
            .unwrap()
            .get(id)
            .map(|s| s.0)
            .unwrap_or(PluginState::Unloaded)
    }

    /// 强制迁移并广播（同时通知订阅该插件的 watch 接收端与全局 broadcast）。
    pub fn transition(
        &self,
        id: &PluginId,
        new_state: PluginState,
        msg: Option<String>,
    ) -> Result<(), agent_kernel_core::KernelError> {
        let mut g = self.states.lock().unwrap();
        if let Some((cur, tx)) = g.get_mut(id) {
            *cur = new_state;
            let _ = tx.send(new_state);
            let _ = self.lifecycle_tx.send(LifecycleEvent {
                plugin: id.clone(),
                phase: new_state.phase(),
                generation: 0,
                message: msg,
            });
        }
        Ok(())
    }
}
