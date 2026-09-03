//! `agent-kernel-sdk`：插件 SDK（L3 语言绑定）。
//!
//! **这是插件唯一可见的依赖**（约束条件：插件只依赖 sdk，禁止依赖 kernel 内部模块）。
//! 本 crate 承载：
//! - `Plugin` trait —— **所有方法 `&self`**（A1）。句柄恒为 `Arc<dyn Plugin>`，
//!   内核侧禁止 `Arc<Mutex<dyn Plugin>>` / `&mut self` / `self: Box<Self>`。插件内部
//!   可变性由插件自己承担（Mutex / actor mailbox）。
//! - `HostApi` —— 插件回调内核的接口。
//! - `PluginContext` / `KernelContext` —— 空间边界（各自状态 / 只读共享内核）。
//! - `Migratable` —— 热迁移协议，**默认实现返回类型化错误，严禁 `unimplemented!()`**（C2）。

mod context;
mod host;
mod migratable;
mod plugin;
pub mod dylib;
mod macros;

pub use agent_kernel_core::*;
pub use context::{GlobalConfig, KernelContext, PluginConfig, PluginContext};
pub use dylib::LoadedPlugin;
pub use host::HostApi;
pub use migratable::Migratable;
pub use plugin::{Plugin, PluginInstance};

/// 注册一个插件为 `PluginInstance`（即 `Arc<dyn Plugin>`）。
///
/// # 示例
/// ```ignore
/// agent_kernel_sdk::register_plugin(MyPlugin::new())
/// ```
pub fn register_plugin(p: impl Plugin + 'static) -> PluginInstance {
    std::sync::Arc::new(p)
}
