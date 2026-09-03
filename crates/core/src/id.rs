use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// 插件身份。全局唯一，作为注册表主键。
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PluginId(pub String);

impl PluginId {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 插件种类：内核原语（不可插件化）vs 普通能力插件。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    /// 业务/能力插件
    #[default]
    Capability,
    /// 编排插件（驱动 agent 主循环的状态机本身也是插件）
    Orchestrator,
}

/// 全链路追踪 ID。时间有序（UUIDv7），便于按时间切片检索（02 选型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId(pub Uuid);

impl TraceId {
    /// 生成一个新的时间有序 trace_id。
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
