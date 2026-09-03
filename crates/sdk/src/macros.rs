//! `#[plugin]` / `plugin!` 宏：为 cdylib 插件导出 dylib 入口符号。
//!
//! 生成两个符号（见 `dylib` 模块）：
//! - `static _KERNEL_PLUGIN_ABI: u32` —— ABI 守卫（宿主与插件必须同 rustc + 同版本 sdk）
//! - `unsafe extern "C" fn _kernel_plugin_create() -> *mut c_void` —— 创建实例，
//!   返回打包的 `Arc<dyn Plugin>`；构造过程 panic 被 `catch_unwind` 收敛为 null
//!   （**panic 严禁跨越 `extern "C"` 边界**，跨 FFI unwind 是 UB）。

/// 导出 dylib 插件入口符号。
///
/// 用法（crate-type = `["cdylib"]` 的插件 crate）：
/// ```ignore
/// agent_kernel_sdk::plugin!(MyPlugin::new());
/// ```
#[macro_export]
macro_rules! plugin {
    ($make:expr) => {
        #[no_mangle]
        pub static _KERNEL_PLUGIN_ABI: u32 = $crate::dylib::PLUGIN_ABI;

        #[no_mangle]
        pub unsafe extern "C" fn _kernel_plugin_create() -> *mut std::ffi::c_void {
            let make = || -> $crate::PluginInstance { std::sync::Arc::new($make) };
            // panic 不得跨越 extern "C" 边界：捕获后返回 null，宿主侧报错
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(make)) {
                Ok(instance) => $crate::dylib::instance_into_raw(instance),
                Err(_) => std::ptr::null_mut(),
            }
        }
    };
}
