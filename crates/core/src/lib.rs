//! `agent-kernel-core`：数据规范层（core）。
//!
//! 本 crate 承载 L1 语义契约在 Rust 侧的**单一事实源**：ID、版本、能力、事件、
//! 错误码、状态枚举、世代、信封。所有类型都跨 ABI 可序列化，且**禁止**出现
//! `Instant` / `Arc` / 裸引用 / 非 `'static` 借用（A2：时间维度相对化）。
//!
//! 依赖规则：core 不依赖任何业务 crate（零依赖本 crate 之外除通用库）。

mod capability;
mod envelope;
mod error;
mod event;
mod generation;
mod id;
mod lifecycle;
mod manifest;
mod version;

pub use capability::{Capability, CapabilitySet};
pub use envelope::{Envelope, Priority};
pub use error::{KernelError, KernelResult};
pub use event::{Event, LifecycleEvent, LifecyclePhase};
pub use generation::Generation;
pub use id::{PluginId, PluginKind, TraceId};
pub use lifecycle::PluginState;
pub use manifest::{DependencySpec, Domain, Manifest, Semantics};
pub use version::{ApiVersion, Version};
