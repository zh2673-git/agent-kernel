use crate::{PluginId, TraceId};
use serde::{Deserialize, Serialize};

/// 总线上的事件（时间流的基本单元）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    /// 事件类型（插件据此 `on_event` 路由）。
    pub ty: String,
    pub trace_id: TraceId,
    pub payload: serde_json::Value,
}

impl Event {
    pub fn new(ty: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            id: uuid::Uuid::now_v7().to_string(),
            ty: ty.into(),
            trace_id: TraceId::new(),
            payload,
        }
    }
}

/// 生命周期阶段（内核驱动全部；插件只依赖 sdk）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecyclePhase {
    Registered,
    Loading,
    Running,
    Draining,
    Failed,
    Unloaded,
}

/// 生命周期事件（走 broadcast 总线，承载审计/指标）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LifecycleEvent {
    pub plugin: PluginId,
    pub phase: LifecyclePhase,
    pub generation: u64,
    pub message: Option<String>,
}
