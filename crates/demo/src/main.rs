//! agent-kernel 演示：跑通 注册 → 分发 → 跨插件调用 → 不停机热替换 闭环。
//!
//! 运行：`cargo run -p agent-kernel-demo`

use agent_kernel_kernel::Kernel;
use agent_kernel_sdk::{Envelope, GlobalConfig, PluginId};
use agent_kernel_skeleton_llm::LlmPlugin;
use agent_kernel_skeleton_tools::ToolsPlugin;
use serde_json::json;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let kernel: Arc<Kernel> = Kernel::new(GlobalConfig {
        node_id: "demo".into(),
        max_total_inflight: 64,
    });

    // 注册插件（骨架，仅依赖 sdk）
    kernel.register(LlmPlugin::new("v1")).await;
    kernel.register(ToolsPlugin::new()).await;
    tracing::info!("registered llm-adapter(v1) + tools-registry");

    // 启动主事件循环
    let runner = kernel.clone();
    let loop_handle = tokio::spawn(async move { runner.run().await });

    // 直接分发到 LLM
    let r1 = kernel
        .dispatch(Envelope::new(
            PluginId::new("llm-adapter"),
            json!({ "prompt": "hello" }),
        ))
        .await?;
    tracing::info!("llm v1 response: {r1}");

    // 跨插件调用：tools 内部通过 HostApi 转发到 llm
    let r2 = kernel
        .dispatch(Envelope::new(
            PluginId::new("tools-registry"),
            json!({ "action": "call-llm", "prompt": "via-tools" }),
        ))
        .await?;
    tracing::info!("tools -> llm response: {r2}");

    // 不停机热替换：用 v2 替换 llm-adapter（演示 A4 CAS 切换）
    kernel.hot_swap(PluginId::new("llm-adapter"), LlmPlugin::new("v2")).await;
    tracing::info!("hot-swapped llm-adapter -> v2");

    let r3 = kernel
        .dispatch(Envelope::new(
            PluginId::new("llm-adapter"),
            json!({ "prompt": "hello-again" }),
        ))
        .await?;
    tracing::info!("llm v2 response: {r3}");
    assert!(r3["model"] == json!("v2"), "hot-swap should serve v2");

    kernel.stop();
    kernel.destroy().await;
    let _ = loop_handle.await;
    tracing::info!("demo complete");
    Ok(())
}
