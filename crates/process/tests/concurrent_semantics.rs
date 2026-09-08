//! 并发语义回归：声明 `Semantics::Concurrent` 的 Process 插件不得在 client 锁上
//! 被串行化（修复前 `call_on_event` 整段 RPC 持锁 → 4 个 300ms 请求 ≥ 1200ms；
//! 修复后锁内克隆 client、锁外 await → 总耗时 ≈ 单请求时延）。
//!
//! 串行语义的串行化载体是 ProcessDomain 的插件级锁（kernel 域层），不在本测覆盖。

use agent_kernel_core::{ApiVersion, Domain, Manifest, Semantics, Version};
use agent_kernel_process::ProcessPlugin;
use agent_kernel_sdk::{Envelope, Plugin, PluginId};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;

fn concurrent_manifest() -> Manifest {
    Manifest {
        name: PluginId::new("echo-proc-conc"),
        kind: Default::default(),
        version: Version::new(0, 1, 0),
        api_version: ApiVersion::new(0, 1),
        capabilities: vec![],
        dependencies: vec![],
        domain: Domain::Process,
        semantics: Semantics::Concurrent,
        priority: 0,
        max_inflight: None,
        fuel_limit: None,
        host_timeout_ms: None,
        epoch_interval_ms: None,
        subscriptions: vec![],
    }
}

#[tokio::test]
async fn concurrent_semantics_not_serialized() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_process-guest"));
    let pp = Arc::new(
        ProcessPlugin::spawn(concurrent_manifest(), &mut cmd)
            .await
            .expect("spawn + handshake"),
    );

    // 预热一次（首包建链/HTTP2 就绪，避免计入计时）
    pp.on_event(Envelope::new(
        PluginId::new("echo-proc-conc"),
        json!({ "warmup": true }),
    ))
    .await
    .expect("warmup dispatch");

    let delay = 300u64;
    let n = 4;
    let start = Instant::now();
    let mut tasks = Vec::new();
    for i in 0..n {
        let pp = Arc::clone(&pp);
        tasks.push(tokio::spawn(async move {
            pp.on_event(Envelope::new(
                PluginId::new("echo-proc-conc"),
                json!({ "i": i, "delay_ms": delay }),
            ))
            .await
            .expect("concurrent dispatch")
        }));
    }
    for t in tasks {
        let r = t.await.expect("join");
        assert_eq!(r["echo"]["delay_ms"], json!(delay), "回显应保留 delay_ms");
    }
    let elapsed = start.elapsed();

    // 修复前：整段 RPC 持锁 → 串行 ≥ 4×300ms = 1200ms。
    // 修复后：并发在途 → ≈ 300~450ms。取 1000ms 为界（余量 >500ms，容忍 CI 抖动）。
    assert!(
        elapsed < Duration::from_millis(1000),
        "Concurrent 声明被串行化：4×{delay}ms 总耗时 {elapsed:?}（应接近单请求时延）"
    );

    // 收尸：直连 ProcessPlugin 不经内核生命周期，须显式 destroy 杀掉 guest 进程。
    pp.destroy().expect("destroy guest");
}
