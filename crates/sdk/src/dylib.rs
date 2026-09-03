//! dylib 动态装载（M1：**同编译器契约**）。
//!
//! **ABI 安全声明**：Rust 无稳定 ABI。本模块采用"同一 rustc + 同一版本
//! `agent-kernel-sdk`"契约——插件 dylib 与宿主必须满足该契约，否则
//! `PLUGIN_ABI` 守卫会拒绝装载。真正的跨语言 / 跨编译器隔离请使用
//! WASM 域或 Process 域（L2 线协议）。
//!
//! **传递方式**：`Arc<dyn Plugin>` 是胖指针（数据指针 + vtable 指针），
//! 直接转 `*mut c_void` 会丢失 vtable（UB）。故将其拆为两个 `usize`，
//! 打包进堆上 `[usize; 2]` 后经 `extern "C"` 边界传递，宿主侧重构胖指针。
//!
//! **加载器**：纯 std 实现——Windows 用 `LoadLibraryW/GetProcAddress`，
//! Unix 用 `dlopen/dlsym`（均无第三方 crate）。

use crate::{KernelError, Plugin, PluginInstance};
use std::ffi::c_void;
use std::path::Path;
use std::sync::Arc;

/// 插件 dylib ABI 版本。sdk 破坏性变更时 +1（宿主与插件不一致即拒绝装载）。
pub const PLUGIN_ABI: u32 = 1;

/// 插件侧：把实例打包为可跨 `extern "C"` 边界的裸指针（`[usize; 2]` 堆块）。
///
/// 由 [`crate::plugin!`] 宏生成的 `_kernel_plugin_create` 调用。
pub fn instance_into_raw(instance: PluginInstance) -> *mut c_void {
    let fat: *const dyn Plugin = Arc::into_raw(instance);
    // 胖指针 = (数据指针, vtable 指针)，拆成两个 usize 打包（同编译器契约下布局一致）
    let (data, vtable): (usize, usize) = unsafe { std::mem::transmute(fat) };
    let packed: Box<[usize; 2]> = Box::new([data, vtable]);
    Box::into_raw(packed) as *mut c_void
}

/// 宿主侧：从打包指针恢复实例。**必须与 [`instance_into_raw`] 成对使用**，
/// 且 dylib 与宿主满足同编译器契约。
///
/// # Safety
/// `raw` 必须由同契约 dylib 的 `_kernel_plugin_create` 返回，且只能恢复一次。
pub unsafe fn instance_from_raw(raw: *mut c_void) -> PluginInstance {
    assert!(!raw.is_null(), "null plugin handle");
    let packed: Box<[usize; 2]> = Box::from_raw(raw as *mut [usize; 2]);
    let fat: *const dyn Plugin = std::mem::transmute((packed[0], packed[1]));
    Arc::from_raw(fat)
}

/// 已装载的 dylib 插件。**句柄必须保持存活**（Drop 时卸载库）。
pub struct LoadedPlugin {
    instance: PluginInstance,
    _lib: Library,
}

impl LoadedPlugin {
    /// 取出插件实例（`Arc<dyn Plugin>`），可直接 `kernel.register(...)`。
    pub fn instance(&self) -> PluginInstance {
        Arc::clone(&self.instance)
    }
}

/// 装载一个 dylib 插件：打开库 → `PLUGIN_ABI` 守卫 → 调用 `_kernel_plugin_create`。
pub fn load(path: &Path) -> Result<LoadedPlugin, KernelError> {
    let lib = Library::open(path)?;

    // ABI 守卫：宿主与插件必须由同一 rustc + 同版本 sdk 编译
    let abi = lib.abi()?;
    if abi != PLUGIN_ABI {
        return Err(KernelError::DomainUnavailable(format!(
            "plugin abi mismatch: dylib={abi}, host={PLUGIN_ABI}（请用同一 rustc 与同版本 agent-kernel-sdk 重新编译插件 dylib）"
        )));
    }

    let create = lib.create_fn()?;
    let raw = unsafe { create() };
    if raw.is_null() {
        return Err(KernelError::DomainUnavailable(
            "_kernel_plugin_create panicked or returned null".into(),
        ));
    }
    let instance = unsafe { instance_from_raw(raw) };
    Ok(LoadedPlugin { instance, _lib: lib })
}

// ---------------------------------------------------------------------------
// 平台加载器（纯 std + 平台链接，无第三方 crate）
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod imp {
    use super::*;
    use std::os::windows::ffi::OsStrExt;

    // kernel32 由默认链接配置提供，无需额外链接属性
    extern "system" {
        fn LoadLibraryW(lpfilename: *const u16) -> isize; // HMODULE
        fn GetProcAddress(hmodule: isize, lpprocname: *const u8) -> *mut c_void;
        fn FreeLibrary(hmodule: isize) -> i32;
    }

    pub struct Library {
        handle: isize,
    }

    // HMODULE 是进程级整数句柄，跨线程使用安全
    unsafe impl Send for Library {}
    unsafe impl Sync for Library {}

    impl Library {
        pub fn open(path: &Path) -> Result<Self, KernelError> {
            let wide: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let handle = unsafe { LoadLibraryW(wide.as_ptr()) };
            if handle == 0 {
                return Err(KernelError::DomainUnavailable(format!(
                    "LoadLibraryW failed: {}",
                    path.display()
                )));
            }
            Ok(Self { handle })
        }

        pub unsafe fn raw_symbol(&self, name: &[u8]) -> Result<*mut c_void, KernelError> {
            let p = GetProcAddress(self.handle, name.as_ptr());
            if p.is_null() {
                return Err(KernelError::DomainUnavailable(format!(
                    "missing symbol: {}",
                    String::from_utf8_lossy(name)
                )));
            }
            Ok(p)
        }
    }

    impl Drop for Library {
        fn drop(&mut self) {
            unsafe { FreeLibrary(self.handle) };
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::ffi::CString;

    #[link(name = "dl")]
    extern "C" {
        fn dlopen(filename: *const std::os::raw::c_char, flags: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const std::os::raw::c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> i32;
    }
    const RTLD_NOW: i32 = 2;
    const RTLD_LOCAL: i32 = 0;

    pub struct Library {
        handle: *mut c_void,
    }

    unsafe impl Send for Library {}
    unsafe impl Sync for Library {}

    impl Library {
        pub fn open(path: &Path) -> Result<Self, KernelError> {
            let c = CString::new(path.as_os_str().to_string_lossy().as_bytes())
                .map_err(|e| KernelError::DomainUnavailable(format!("bad path: {e}")))?;
            let handle = unsafe { dlopen(c.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
            if handle.is_null() {
                return Err(KernelError::DomainUnavailable(format!(
                    "dlopen failed: {}",
                    path.display()
                )));
            }
            Ok(Self { handle })
        }

        pub unsafe fn raw_symbol(&self, name: &[u8]) -> Result<*mut c_void, KernelError> {
            let c = CString::new(&name[..name.len() - 1]) // 去 NUL
                .map_err(|e| KernelError::DomainUnavailable(format!("bad symbol: {e}")))?;
            let p = dlsym(self.handle, c.as_ptr());
            if p.is_null() {
                return Err(KernelError::DomainUnavailable(format!(
                    "missing symbol: {}",
                    String::from_utf8_lossy(name)
                )));
            }
            Ok(p)
        }
    }

    impl Drop for Library {
        fn drop(&mut self) {
            unsafe { dlclose(self.handle) };
        }
    }
}

use imp::Library;

impl Library {
    /// 读取 `_KERNEL_PLUGIN_ABI` 静态导出。
    fn abi(&self) -> Result<u32, KernelError> {
        unsafe {
            let p = self.raw_symbol(b"_KERNEL_PLUGIN_ABI\0")? as *const u32;
            if p.is_null() {
                Err(KernelError::DomainUnavailable("missing abi symbol".into()))
            } else {
                Ok(*p)
            }
        }
    }

    /// 取 `_kernel_plugin_create` 函数指针。
    fn create_fn(&self) -> Result<unsafe extern "C" fn() -> *mut c_void, KernelError> {
        unsafe {
            let p = self.raw_symbol(b"_kernel_plugin_create\0")?;
            Ok(std::mem::transmute::<
                *mut c_void,
                unsafe extern "C" fn() -> *mut c_void,
            >(p))
        }
    }
}
