use crate::{PluginId, TraceId};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// 调度优先级。决定泳道归属与背压策略强度（03 §3.2 / 04 C）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// 后台/批处理：被 drain/背压最先牺牲。
    Background = 0,
    /// 普通能力调用。
    Normal = 1,
    /// 用户直接触发。
    User = 2,
    /// 系统/编排心跳：最高优先，豁免 Block（06 反模式）。
    System = 3,
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Normal
    }
}

/// 调用信封（L1/L2 线协议在 Rust 侧的镜像）。
///
/// **A2（关键）**：`deadline` 是 `Option<Duration>`（相对时长），不是 `Instant`。
/// `Instant` 是进程本地单调时钟，跨 ABI / 跨进程无意义；与 wit `deadline-ms`、
/// proto `deadline_ms` 一致（均为相对时长）。出现在 `Envelope` 中的类型都必须
/// 跨 ABI 可序列化——`Instant` / `Arc` / 裸引用一律禁止。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    /// 目标插件。
    pub target: PluginId,
    /// 全链路 trace_id（时间有序）。
    pub trace_id: TraceId,
    /// 优先级。
    pub priority: Priority,
    /// 相对截止时长。None = 无硬截止。
    pub deadline: Option<Duration>,
    /// 载荷（跨 ABI 序列化后的数据）。
    pub payload: serde_json::Value,
}

impl Envelope {
    pub fn new(target: PluginId, payload: serde_json::Value) -> Self {
        Self {
            target,
            trace_id: TraceId::new(),
            priority: Priority::Normal,
            deadline: None,
            payload,
        }
    }
    pub fn with_priority(mut self, p: Priority) -> Self {
        self.priority = p;
        self
    }
    pub fn with_deadline(mut self, d: Duration) -> Self {
        self.deadline = Some(d);
        self
    }
}
