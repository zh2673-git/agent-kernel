//! 插件回调内核的接口（L1 规则契约在宿主侧的镜像）。
//!
//! 仅面向**进程内/可直连**场景；WASM/进程域通过各自的 host function / gRPC 桥接，
//! 不应直接 `impl` 此 trait 跨 ABI。

use crate::{Envelope, Event, KernelResult, Priority, TraceId};
use async_trait::async_trait;
use std::time::Duration;

#[async_trait]
pub trait HostApi: Send + Sync {
    /// 发布一个事件到总线（broadcast 事件类 / watch 状态类由内核路由，B1）。
    async fn emit(&self, event: Event) -> KernelResult<()>;

    /// 同步调用另一个插件（跨插件通信一律走内核，禁止插件间直接内存互访）。
    async fn call_plugin(&self, env: Envelope) -> KernelResult<serde_json::Value>;

    /// 按 capability 调用提供者（K2 寻址契约，v0.1.4）。
    ///
    /// 内核解析 `capability → PluginId` 后复用 dispatch 全链（B5 快照 / B2 panic 隔离 /
    /// B4 背压 / deadline 竞争全部继承）。解析为**每次调用动态查当前注册表**——
    /// 与热替换兼容（换实现后新调用自动流向新实例），这是"面向能力编程"的入口：
    /// 调用方不再硬编码提供者名字，居民因此可被同名/异名实现替换。
    ///
    /// 多提供者规则：**先注册者胜**（见 `dependency::build_cap_index`）。
    /// 默认实现返回 K404——宿主未支持寻址时既有实现者零改动（向后兼容）。
    async fn call_capability(
        &self,
        capability: &str,
        payload: serde_json::Value,
        deadline: Duration,
    ) -> KernelResult<serde_json::Value> {
        let _ = (capability, payload, deadline);
        Err(crate::KernelError::UnknownCapability(capability.to_string()))
    }

    /// 带调用方身份的 capability 寻址（v0.1.7，S1 贯穿）：
    /// `trace_id`（链路串联——补上 `call_capability` 每次新 trace 的缺口）与
    /// `priority`（泳道，T8）由调用方显式指定，其余语义与 [`Self::call_capability`] 相同。
    /// 增量方法（默认实现 K404），v0.1.4 的 `call_capability` 零改动兼容。
    async fn call_capability_as(
        &self,
        capability: &str,
        trace_id: TraceId,
        priority: Priority,
        payload: serde_json::Value,
        deadline: Duration,
    ) -> KernelResult<serde_json::Value> {
        let _ = (capability, trace_id, priority, payload, deadline);
        Err(crate::KernelError::UnknownCapability(capability.to_string()))
    }

    /// 进程内单调时钟（纳秒）。仅用于相对计时；**不**进 `Envelope`（A2）。
    /// 口径澄清（v0.1.3，K4）：这是相对 deadline 测量的时钟源，与 `Envelope.deadline`
    /// （相对时长）配对使用；跨 ABI / 跨进程无意义，禁止持久化或跨端比较。
    fn now_mono(&self) -> u64;

    /// 当前 trace_id（链路串联）。
    ///
    /// 已废弃（v0.1.3，K4 口径澄清）：本方法每次调用返回**全新** TraceId，无法承载
    /// 链路串联语义。链路串联的真相源是 `Envelope.trace_id`（调用方拷贝转发）。
    /// 首个破坏性版本将删除。
    #[deprecated(
        since = "0.1.3",
        note = "链路串联的真相源是 Envelope.trace_id（本方法每次调用生成新 id，无串联语义）；将于首个破坏性版本删除"
    )]
    fn trace_id(&self) -> TraceId;
}
