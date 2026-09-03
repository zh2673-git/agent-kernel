use crate::{ApiVersion, Capability, PluginId, PluginKind, Version};
use serde::{Deserialize, Serialize};

/// 执行域（混合多域统一抽象，03 §2.6 / 05 清单校验）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Domain {
    /// 进程内 Trait 快车道（同语言零开销）。
    InProcess,
    /// WASM 组件模型（强隔离、跨语言）。仅允许 Serial（A3）。
    Wasm,
    /// 独立进程（gRPC + protobuf，D1/D2）。
    Process,
}

/// 执行语义（并发模型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Semantics {
    /// 串行：插件单线程顺序处理（WASM 强制，A3）。载体 = 插件级 mailbox。
    Serial,
    /// 并发：插件内部自行并发（仅 in-process / process 允许）。
    Concurrent,
}

/// 依赖声明。hard 缺失则拒绝加载；soft 缺失则补挂（07 §6.1，B1）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencySpec {
    pub capability: Capability,
    #[serde(default)]
    pub hard: bool,
}

/// 插件清单（`manifest.toml`，格式 D4：TOML + JSON Schema 校验）。
///
/// 注意：清单是 **core 的纯数据**，TOML 解析在 kernel 侧完成。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub name: PluginId,
    #[serde(default)]
    pub kind: PluginKind,
    pub version: Version,
    pub api_version: ApiVersion,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub dependencies: Vec<DependencySpec>,
    pub domain: Domain,
    pub semantics: Semantics,
    #[serde(default)]
    pub priority: u8,
    /// 资源上限：每插件最大在途请求数（背压，B4）。
    #[serde(default)]
    pub max_inflight: Option<usize>,
    /// WASM 域专用：fuel 指令上限（B3）。
    #[serde(default)]
    pub fuel_limit: Option<u64>,
    /// 每个 host call 的墙钟超时（B3）。
    #[serde(default)]
    pub host_timeout_ms: Option<u64>,
    /// epoch tick 间隔毫秒（B3）。
    #[serde(default)]
    pub epoch_interval_ms: Option<u64>,
    /// 订阅的事件类型（事件循环据此路由，program.md 时间契约）。
    #[serde(default)]
    pub subscriptions: Vec<String>,
}

impl Manifest {
    /// A3 校验：WASM 域若声明 concurrent，直接非法（装载期拒，非静默降级）。
    pub fn validate(&self) -> Result<(), crate::error::KernelError> {
        if self.domain == Domain::Wasm && self.semantics == Semantics::Concurrent {
            return Err(crate::error::KernelError::WasmConcurrentForbidden);
        }
        Ok(())
    }
}
