use crate::LifecyclePhase;
use serde::{Deserialize, Serialize};
use std::fmt;

/// 插件运行时状态。生命周期状态机由 `LifecycleManager` 强制（04 A）。
///
/// 合法迁移：
/// Registered → Loading → Running → Draining → Unloaded
/// Running/Draining → Failed（捕获 panic 后唯一合法终态，B2）
/// Loading → Unloaded（装载失败回滚）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    Registered,
    Loading,
    Running,
    Draining,
    Failed,
    Unloaded,
}

impl PluginState {
    /// 是否仍持有资源（需要在 destroy 时回收）。Failed/Unloaded 之外都视为"活着"。
    pub fn is_alive(self) -> bool {
        !matches!(self, PluginState::Unloaded)
    }
    /// 是否可接收新事件。
    pub fn accepts_new_work(self) -> bool {
        matches!(self, PluginState::Running)
    }
    pub fn phase(self) -> LifecyclePhase {
        match self {
            PluginState::Registered => LifecyclePhase::Registered,
            PluginState::Loading => LifecyclePhase::Loading,
            PluginState::Running => LifecyclePhase::Running,
            PluginState::Draining => LifecyclePhase::Draining,
            PluginState::Failed => LifecyclePhase::Failed,
            PluginState::Unloaded => LifecyclePhase::Unloaded,
        }
    }
}

impl fmt::Display for PluginState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PluginState::Registered => "registered",
            PluginState::Loading => "loading",
            PluginState::Running => "running",
            PluginState::Draining => "draining",
            PluginState::Failed => "failed",
            PluginState::Unloaded => "unloaded",
        })
    }
}
