//! WASM guest 示例：实现 `schema/kernel.wit` 中导出的 `plugin` 接口。
//!
//! 构建组件：
//! ```bash
//! cargo build -p agent-kernel-wasm-guest --target wasm32-wasip2 --release
//! # 产物：target/wasm32-wasip2/release/agent_kernel_wasm_guest.wasm
//! ```
//!
//! 说明：本 guest 仅做 echo，不依赖内核 host 函数；`host-api` 由 world 声明为
//! import，wit-bindgen 会生成对应的 import 桩（本示例不调用）。

wit_bindgen::generate!({
    path: "../../schema/kernel.wit",
    world: "agent-plugin",
});

use agent_kernel::plugin::types::{Envelope, KernelError, PluginMeta};

/// echo 插件组件。
struct EchoGuest;

impl exports::agent_kernel::plugin::plugin::Guest for EchoGuest {
    fn meta() -> PluginMeta {
        PluginMeta {
            id: "echo-wasm".to_string(),
            version: "0.1.0".to_string(),
            api_version: "0.1".to_string(),
        }
    }
    fn init(_cfg_json: String) -> Result<(), KernelError> {
        Ok(())
    }
    fn start() -> Result<(), KernelError> {
        Ok(())
    }
    fn on_event(ev: Envelope) -> Result<Option<Envelope>, KernelError> {
        // echo：把 payload 原样包一层返回
        let payload_str = String::from_utf8_lossy(&ev.payload).to_string();
        let echoed = format!("{{\"echo\":{payload_str}}}");
        Ok(Some(Envelope {
            trace_id: ev.trace_id.clone(),
            span_id: ev.span_id.clone(),
            ty: format!("{}.echoed", ev.ty),
            payload: echoed.into_bytes(),
            priority: ev.priority,
            deadline_ms: ev.deadline_ms,
        }))
    }
    fn snapshot() -> Result<Vec<u8>, KernelError> {
        Err(KernelError::PluginFailed("snapshot unsupported in echo-guest".into()))
    }
    fn restore(_payload: Vec<u8>) -> Result<(), KernelError> {
        Err(KernelError::PluginFailed("restore unsupported in echo-guest".into()))
    }
    fn drain(_timeout_ms: u64) -> Result<(), KernelError> {
        Ok(())
    }
    fn stop() -> Result<(), KernelError> {
        Ok(())
    }
    fn destroy() -> Result<(), KernelError> {
        Ok(())
    }
}

export!(EchoGuest);
