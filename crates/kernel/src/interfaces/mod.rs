//! 数据接口 + 规则入口（interfaces）：dispatch、HostApi 实现、执行域适配器。
//! 依赖方向：interfaces 可引用 domain / infrastructure（单向）。

pub mod dispatch;
pub mod domains;
pub mod host;
#[cfg(feature = "process")]
pub mod process_domain;
#[cfg(feature = "wasm")]
pub mod wasm_domain;

pub use dispatch::dispatch;
pub use domains::{domain_for, ExecutionDomain, InProcessDomain};
pub use host::KernelHost;
#[cfg(feature = "process")]
pub use process_domain::ProcessDomain;
#[cfg(feature = "wasm")]
pub use wasm_domain::WasmDomain;
