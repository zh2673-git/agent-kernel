//! 插件回调内核的接口（L1 规则契约在宿主侧的镜像）。
//!
//! 仅面向**进程内/可直连**场景；WASM/进程域通过各自的 host function / gRPC 桥接，
//! 不应直接 `impl` 此 trait 跨 ABI。

use crate::{Envelope, Event, KernelResult, TraceId};
use async_trait::async_trait;

#[async_trait]
pub trait HostApi: Send + Sync {
    /// 发布一个事件到总线（broadcast 事件类 / watch 状态类由内核路由，B1）。
    async fn emit(&self, event: Event) -> KernelResult<()>;

    /// 同步调用另一个插件（跨插件通信一律走内核，禁止插件间直接内存互访）。
    async fn call_plugin(&self, env: Envelope) -> KernelResult<serde_json::Value>;

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
