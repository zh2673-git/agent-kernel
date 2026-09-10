//! 居民出生证明（P2，谱系同源 ④）：任何 `Plugin` 实现的验收套件。
//!
//! **定位**：内核 v0.1.4 起，"谱系"的准入标准不再靠口头约定——一个插件实现跑过
//! 本套件即内核契约合规。套件分两层：
//!
//! 1. **居民验收**（对被测插件）：manifest 形状 → 注册即服务 → 并发语义 →
//!    热替换安全 → destroy 幂等 → health；
//! 2. **地基探针**（对居民将运行的内核构建）：panic 隔离（B2）、drain 期拒绝（K403）、
//!    事件总线 fail-fast（K505）——用探针插件验证运行环境本身满足不变量。
//!
//! 用法：
//! ```ignore
//! let report = conformance::certify(Arc::new(|| MyPlugin::new())).await;
//! assert!(report.is_ok(), "{}", report.summary());
//! ```
//!
//! 设计纪律（内核 PLAN §三）：每项检查 = 命名不变量 + 明确失败语义 + 可执行验证。

use agent_kernel_core::{Envelope, Event, KernelError, PluginId};
use agent_kernel_kernel::Kernel;
use agent_kernel_sdk::{GlobalConfig, Plugin, PluginContext, PluginInstance};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

/// 居民工厂：每次检查用全新实例（防跨检查状态污染）。
pub type ResidentFactory = Arc<dyn Fn() -> PluginInstance + Send + Sync>;

/// 单项检查结果。
#[derive(Debug, Clone)]
pub struct Check {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

/// 验收报告。
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    pub fn is_ok(&self) -> bool {
        self.checks.iter().all(|c| c.passed)
    }
    pub fn summary(&self) -> String {
        self.checks
            .iter()
            .map(|c| format!("{} {} — {}", if c.passed { "✅" } else { "❌" }, c.name, c.detail))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

async fn fresh_kernel() -> Arc<Kernel> {
    Kernel::new(GlobalConfig { node_id: "conformance".into(), max_total_inflight: 64 })
}

fn check(name: &'static str, passed: bool, detail: String) -> Check {
    Check { name, passed, detail }
}

/// 对一个居民实现跑全套验收。所有检查相互独立（各自新内核 + 新实例）。
pub async fn certify(make: ResidentFactory) -> Report {
    let mut checks = Vec::new();

    // ① 实例契约：manifest 形状合法（A3 装载期校验）
    {
        let inst = make();
        let name = inst.manifest().name.0.clone();
        checks.push(match inst.manifest().validate() {
            Ok(()) => check("manifest_shape", true, format!("{name}: manifest 合法")),
            Err(e) => check("manifest_shape", false, format!("{name}: {e}")),
        });
    }

    // ② 注册即服务：注册后 dispatch 可达（生命周期进入 Running 的外部可观测量）
    {
        let k = fresh_kernel().await;
        let inst = make();
        let target = inst.manifest().name.clone();
        k.register(inst).await;
        let res = k.dispatch(Envelope::new(target.clone(), json!({}))).await;
        checks.push(match res {
            Ok(_) => check("registers_and_serves", true, format!("{target}: 注册后 dispatch 可达")),
            Err(e) => check("registers_and_serves", false, format!("{target}: dispatch 失败 {e}")),
        });
    }

    // ③ 热替换安全：drain 后 CAS 切换（K1），切换后继续服务
    {
        let k = fresh_kernel().await;
        let inst = make();
        let target = inst.manifest().name.clone();
        k.register(inst).await;
        k.hot_swap(target.clone(), make()).await;
        let res = k.dispatch(Envelope::new(target.clone(), json!({}))).await;
        checks.push(match res {
            Ok(_) => check("hot_swap_safe", true, format!("{target}: 热替换后继续服务")),
            Err(e) => check("hot_swap_safe", false, format!("{target}: 热替换后 dispatch 失败 {e}")),
        });
    }

    // ④ destroy 幂等（A1 铁律：destroy 是幂等通知，两次调用不得 panic/报错）
    {
        let inst = make();
        let (a, b) = (inst.destroy(), inst.destroy());
        checks.push(check(
            "destroy_idempotent",
            a.is_ok() && b.is_ok(),
            format!("{}: destroy×2 = {:?}", inst.manifest().name.0, (a.is_ok(), b.is_ok())),
        ));
    }

    // ⑤ health 默认健康（可观测契约）
    {
        let inst = make();
        let h = inst.health().await.ok().and_then(|v| v.get("ok").cloned());
        let ok = h.as_ref() == Some(&json!(true));
        checks.push(check("health_ok", ok, format!("{}: health.ok = {:?}", inst.manifest().name.0, h)));
    }

    // ⑥ 地基探针：panic 隔离（B2）——panic 插件被卸载而非拖垮内核
    {
        let k = fresh_kernel().await;
        k.register(Arc::new(PanicProbe::default())).await;
        let res = k
            .dispatch(Envelope::new(PluginId::new(PANIC_PROBE_ID), json!({})))
            .await;
        let isolated = matches!(res, Err(KernelError::PluginPanic(..)));
        checks.push(check(
            "probe_panic_isolation_b2",
            isolated,
            format!("panic 探针 → {}", if isolated { "K501 + 卸载" } else { "未隔离（运行环境不合规）" }),
        ));
    }

    // ⑦ 地基探针：事件总线未 run() 时 emit 快速失败（K505），拒绝静默积压
    {
        let k = fresh_kernel().await;
        k.register(Arc::new(EmitProbe::default())).await;
        let res = k
            .dispatch(Envelope::new(PluginId::new(EMIT_PROBE_ID), json!({})))
            .await;
        let fast_fail = matches!(res, Ok(ref v) if v.get("code").and_then(serde_json::Value::as_str) == Some("K505"));
        checks.push(check(
            "probe_emit_fail_fast_k505",
            fast_fail,
            format!("emit 探针 → {}", if fast_fail { "K505 快速失败" } else { "未快速失败（运行环境不合规）" }),
        ));
    }

    Report { checks }
}

// ---- 探针插件（验证运行环境本身，与被测居民无关） ----

const PANIC_PROBE_ID: &str = "conformance-panic-probe";
const EMIT_PROBE_ID: &str = "conformance-emit-probe";

#[derive(Default)]
struct PanicProbe {
    manifest: std::sync::OnceLock<agent_kernel_core::Manifest>,
}

#[async_trait]
impl Plugin for PanicProbe {
    fn id(&self) -> PluginId {
        PluginId::new(PANIC_PROBE_ID)
    }
    fn manifest(&self) -> &agent_kernel_core::Manifest {
        self.manifest.get_or_init(|| probe_manifest(PANIC_PROBE_ID))
    }
    async fn init(&self, _ctx: &PluginContext) -> agent_kernel_sdk::KernelResult<()> {
        Ok(())
    }
    async fn on_event(&self, _env: Envelope) -> agent_kernel_sdk::KernelResult<serde_json::Value> {
        panic!("conformance panic probe");
    }
    fn destroy(&self) -> agent_kernel_sdk::KernelResult<()> {
        Ok(())
    }
}

#[derive(Default)]
struct EmitProbe {
    manifest: std::sync::OnceLock<agent_kernel_core::Manifest>,
    host: std::sync::OnceLock<Arc<dyn agent_kernel_sdk::HostApi>>,
}

#[async_trait]
impl Plugin for EmitProbe {
    fn id(&self) -> PluginId {
        PluginId::new(EMIT_PROBE_ID)
    }
    fn manifest(&self) -> &agent_kernel_core::Manifest {
        self.manifest.get_or_init(|| probe_manifest(EMIT_PROBE_ID))
    }
    async fn init(&self, ctx: &PluginContext) -> agent_kernel_sdk::KernelResult<()> {
        let _ = self.host.set(ctx.kernel.host.clone());
        Ok(())
    }
    async fn on_event(&self, _env: Envelope) -> agent_kernel_sdk::KernelResult<serde_json::Value> {
        let host = self.host.get().expect("host not initialized");
        match host.emit(Event::new("conformance-probe", json!({}))).await {
            Ok(()) => Ok(json!({ "emitted": true })),
            Err(e) => Ok(json!({ "emitted": false, "code": e.code() })),
        }
    }
    fn destroy(&self) -> agent_kernel_sdk::KernelResult<()> {
        Ok(())
    }
}

fn probe_manifest(name: &str) -> agent_kernel_core::Manifest {
    use agent_kernel_core::{ApiVersion, Domain, Manifest, PluginKind, Semantics, Version};
    Manifest {
        name: PluginId::new(name),
        kind: PluginKind::Capability,
        version: Version::new(0, 1, 0),
        api_version: ApiVersion::new(1, 0),
        capabilities: vec![],
        dependencies: vec![],
        domain: Domain::InProcess,
        semantics: Semantics::Serial,
        priority: 1,
        max_inflight: Some(4),
        fuel_limit: None,
        host_timeout_ms: None,
        epoch_interval_ms: None,
        subscriptions: vec![],
    }
}
