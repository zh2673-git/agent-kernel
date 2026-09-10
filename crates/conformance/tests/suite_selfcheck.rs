//! 套件自检：用一个标准合规居民跑全套验收，必须全绿（套件本身的 P/Q 验证）。
//!
//! 本测试同时是使用范例：任何居民仓的验收测试照此三行即可。

use agent_kernel_core::{ApiVersion, Capability, Domain, Envelope, Manifest, PluginId, PluginKind, Semantics, Version};
use agent_kernel_sdk::{KernelResult, Plugin, PluginContext, PluginInstance};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

struct SampleResident {
    manifest: Manifest,
}

impl SampleResident {
    fn new() -> PluginInstance {
        Arc::new(Self {
            manifest: Manifest {
                name: PluginId::new("sample-resident"),
                kind: PluginKind::Capability,
                version: Version::new(0, 1, 0),
                api_version: ApiVersion::new(1, 0),
                capabilities: vec![Capability::new("sample.echo")],
                dependencies: vec![],
                domain: Domain::InProcess,
                semantics: Semantics::Concurrent,
                priority: 1,
                max_inflight: Some(4),
                fuel_limit: None,
                host_timeout_ms: None,
                epoch_interval_ms: None,
                subscriptions: vec![],
            },
        })
    }
}

#[async_trait]
impl Plugin for SampleResident {
    fn id(&self) -> PluginId {
        self.manifest.name.clone()
    }
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    async fn init(&self, _ctx: &PluginContext) -> KernelResult<()> {
        Ok(())
    }
    async fn on_event(&self, env: Envelope) -> KernelResult<serde_json::Value> {
        Ok(json!({ "echo": env.payload }))
    }
    fn destroy(&self) -> KernelResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn sample_resident_passes_full_suite() {
    let report = agent_kernel_conformance::certify(Arc::new(SampleResident::new)).await;
    assert!(
        report.is_ok(),
        "合规居民必须通过全套验收：\n{}",
        report.summary()
    );
    // 7 项检查全部在册（manifest/serve/hot_swap/destroy/health + B2/K505 探针）
    assert_eq!(report.checks.len(), 7, "{}", report.summary());
}
