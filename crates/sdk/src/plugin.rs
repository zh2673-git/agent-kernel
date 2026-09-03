use crate::{Envelope, KernelContext, KernelResult, Manifest, Migratable, PluginContext, PluginId, TraceId};
use async_trait::async_trait;

/// 插件实例句柄。内核注册表持有 `Arc<dyn Plugin>`（A1）。
pub type PluginInstance = std::sync::Arc<dyn Plugin>;

/// 插件接口（L3 绑定）。
///
/// **A1 铁律**：全部方法 `&self`。插件内部状态自管（Mutex / actor mailbox / 原子量）。
/// `destroy` 是幂等通知；真正的空间释放在 `Drop`。需要补偿语义的插件自行实现幂等键
/// （B2'：async 第三类失败——被 drop 取消时，RAII 跑但业务不变量不自愈）。
#[async_trait]
pub trait Plugin: Send + Sync + 'static {
    /// 插件身份（来自清单）。
    fn id(&self) -> PluginId;

    /// 插件清单（静态，装载期固定）。
    fn manifest(&self) -> &Manifest;

    /// 空间开辟：分配插件私有状态、启动内部 actor mailbox。
    /// 失败应返回 `Err`（内核据此回滚装载事务）。
    async fn init(&self, ctx: &PluginContext) -> KernelResult<()>;

    /// 事件流转：处理一个信封，返回结果载荷。
    /// 实现**不得**跨 `.await` 假设"调用方不会取消"；被取消时由内核 `DrainCoordinator`
    /// 负责不再接新活（B4），但已发起的跨域副作用需插件自身幂等补偿（B2'）。
    async fn on_event(&self, env: Envelope) -> KernelResult<serde_json::Value>;

    /// 空间回收通知（幂等）。真正释放由 `Drop` 完成。
    fn destroy(&self) -> KernelResult<()>;

    /// 若支持热迁移则返回自身（C2：默认 None = 不支持，内核返回 K600）。
    fn as_migratable(&self) -> Option<&dyn Migratable> {
        None
    }

    /// 可选的编排钩子：编排插件可实现以驱动 agent 主循环。
    async fn step(&self, _ctx: &KernelContext) -> KernelResult<()> {
        Ok(())
    }

    /// 调试/可观测：返回当前健康检查（默认健康）。
    async fn health(&self) -> KernelResult<serde_json::Value> {
        Ok(serde_json::json!({"ok": true}))
    }

    /// 返回自身 trace（便于链路串联）。
    fn trace(&self) -> TraceId {
        TraceId::new()
    }
}
