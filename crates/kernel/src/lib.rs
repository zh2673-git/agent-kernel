//! `agent-kernel-kernel`：内核单体。
//!
//! 内部按四层（递归四层模型）切分，依赖方向铁律：
//! `interfaces → application → domain ← infrastructure → core`（core/sdk 在 crate 边界）。
//! 分层从编译期强制**降级为 `xtask layering` 强制**（C1）；但 `plugins/*` 仅依赖 `sdk`
//! 仍是编译期硬禁。
//!
//! 模块映射（数据规范/存储/流转/接口 + 运行时）：
//! - `interfaces` —— 数据接口 + 规则入口：`dispatch`、`KernelHost`、执行域适配器
//! - `application` —— 数据流转（编排）：load / unload / hot-swap 用例
//! - `domain`      —— 数据规范落地的核心规则：registry / lifecycle / dependency / scheduler / hotswap
//! - `infrastructure` —— 数据存储/通信底座：`EventBus`
//! - `runtime`     —— 时空管家：`Kernel` 生命周期（init/start/stop/destroy）+ 主循环

pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod interfaces;
pub mod runtime;

pub use runtime::{Kernel, KernelInner};
