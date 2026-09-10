use crate::{ApiVersion, Generation, PluginId};
use serde::Serialize;
use thiserror::Error;

/// 内核错误。跨 ABI 可序列化（core 层，无裸引用）。错误码稳定，便于跨语言映射。
#[derive(Debug, Error, Clone, PartialEq, Serialize)]
pub enum KernelError {
    // ---- 规则契约（加载期） ----
    /// K202：WASM 域被声明为 concurrent（仅允许 serial，A3）。
    #[error("[K202] wasm domain only supports serial semantics, got concurrent")]
    WasmConcurrentForbidden,
    /// K301：ApiVersion 不兼容，规则穿透风险。
    #[error("[K301] api_version mismatch: plugin {plugin} requires {required}, found {found}")]
    ApiVersionMismatch {
        plugin: PluginId,
        required: ApiVersion,
        found: ApiVersion,
    },
    /// K302：硬依赖的能力未被任何已加载插件声明。
    #[error("[K302] unresolved hard dependency: {0} requires capability '{1}'")]
    HardDependencyUnsatisfied(PluginId, String),
    /// K303：依赖图存在环。
    #[error("[K303] dependency cycle detected: {0}")]
    DependencyCycle(String),
    /// K304：清单解析失败。
    #[error("[K304] manifest parse error: {0}")]
    ManifestParse(String),

    // ---- 空间/注册表 ----
    /// K401：未知插件（注册表中无此 ID）。
    #[error("[K401] unknown plugin: {0}")]
    UnknownPlugin(PluginId),
    /// K402：插件当前状态不可接客（非 Running）。
    #[error("[K402] plugin {0} not in running state (current: {1})")]
    NotRunning(PluginId, String),
    /// K403：drain 期间豁免 Block，退化为 Reject（B4）。
    #[error("[K403] plugin {0} is draining; request rejected (backpressure degraded to reject)")]
    DrainingReject(PluginId),

    // ---- 时间/执行 ----
    /// K501：catch_unwind 捕获到 panic（B2）。
    #[error("[K501] plugin {0} panicked: {1}")]
    PluginPanic(PluginId, String),
    /// K502：deadline 超时。
    #[error("[K502] plugin {0} deadline exceeded")]
    DeadlineExceeded(PluginId),
    /// K503：请求被取消（被 drop，async 第三类失败，B2'）。
    #[error("[K503] request to {0} cancelled")]
    Cancelled(PluginId),
    /// K504：世代 CAS 失败（热替换回滚，A4）。
    #[error("[K504] generation mismatch on {plugin}: expected {expected}, found {found}")]
    GenerationMismatch {
        plugin: PluginId,
        expected: Generation,
        found: Generation,
    },
    /// K505：事件总线未启动（`Kernel::run()` 未运行时 emit；fail-fast 拒绝静默积压，B1）。
    #[error("[K505] event bus not running: Kernel::run() not started; emit rejected (fail-fast, no silent buffering)")]
    EventBusNotRunning,

    // ---- 迁移 ----
    /// K600：插件未实现 Migratable（默认返回，C2）。
    #[error("[K600] plugin {0} does not support hot migration")]
    MigrationUnsupported(PluginId),

    // ---- 通用 ----
    /// 域适配器不支持/未启用（如 process/wasm 在禁用 feature 时）。
    #[error("[K700] execution domain '{0}' not available in this build")]
    DomainUnavailable(String),
    /// 内部不一致（不应发生，用于断言型错误）。
    #[error("[K999] internal invariant violated: {0}")]
    Internal(String),
}

pub type KernelResult<T> = Result<T, KernelError>;

impl KernelError {
    /// 稳定错误码（供跨语言/跨 ABI 映射）。
    pub fn code(&self) -> &'static str {
        match self {
            KernelError::WasmConcurrentForbidden => "K202",
            KernelError::ApiVersionMismatch { .. } => "K301",
            KernelError::HardDependencyUnsatisfied(..) => "K302",
            KernelError::DependencyCycle(_) => "K303",
            KernelError::ManifestParse(_) => "K304",
            KernelError::UnknownPlugin(_) => "K401",
            KernelError::NotRunning(..) => "K402",
            KernelError::DrainingReject(_) => "K403",
            KernelError::PluginPanic(..) => "K501",
            KernelError::DeadlineExceeded(_) => "K502",
            KernelError::Cancelled(_) => "K503",
            KernelError::GenerationMismatch { .. } => "K504",
            KernelError::EventBusNotRunning => "K505",
            KernelError::MigrationUnsupported(_) => "K600",
            KernelError::DomainUnavailable(_) => "K700",
            KernelError::Internal(_) => "K999",
        }
    }
}
