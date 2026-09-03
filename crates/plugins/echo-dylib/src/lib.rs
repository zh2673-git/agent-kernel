//! dylib 插件示例：echo 插件编译为 cdylib，由宿主 dlopen 装载。
//!
//! 构建：`cargo build -p agent-kernel-echo-dylib`
//! 产物：`target/debug/echo_dylib.dll`（Windows）/ `target/debug/libecho_dylib.so`（Unix）
//!
//! **同编译器契约**：本 dylib 必须与宿主由同一 rustc + 同版本 agent-kernel-sdk 编译，
//! 否则装载期 `PLUGIN_ABI` 守卫会拒绝。

// 全部类型经 sdk 重导出获取（插件唯一可见依赖 = sdk）
use agent_kernel_sdk::{
    ApiVersion, Domain, Envelope, KernelResult, Manifest, Plugin, PluginContext, PluginId,
    Semantics, Version,
};
use async_trait::async_trait;
use serde_json::{json, Value};

struct EchoDylib {
    manifest: Manifest,
}

impl EchoDylib {
    fn new() -> Self {
        Self {
            manifest: Manifest {
                name: PluginId::new("echo-dylib"),
                kind: Default::default(),
                version: Version::new(0, 1, 0),
                api_version: ApiVersion::new(0, 1),
                capabilities: vec![],
                dependencies: vec![],
                domain: Domain::InProcess,
                semantics: Semantics::Serial,
                priority: 0,
                max_inflight: None,
                fuel_limit: None,
                host_timeout_ms: None,
                epoch_interval_ms: None,
                subscriptions: vec![],
            },
        }
    }
}

#[async_trait]
impl Plugin for EchoDylib {
    fn id(&self) -> PluginId {
        self.manifest.name.clone()
    }
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    async fn init(&self, _ctx: &PluginContext) -> KernelResult<()> {
        Ok(())
    }
    async fn on_event(&self, env: Envelope) -> KernelResult<Value> {
        Ok(json!({ "echo": env.payload, "from": "dylib" }))
    }
    fn destroy(&self) -> KernelResult<()> {
        Ok(())
    }
}

// 导出 dylib 入口符号（_KERNEL_PLUGIN_ABI / _kernel_plugin_create）
agent_kernel_sdk::plugin!(EchoDylib::new());
