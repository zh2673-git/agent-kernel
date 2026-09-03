//! `agent-kernel-wasm`：WASM 执行域 host 侧（M1）。
//!
//! - 装载 `schema/kernel.wit` 定义的 **WASM 组件**（guest 由 `crates/wasm-guest` 产出）。
//! - 每个插件一个 `Store` ⇒ 独立线性内存（内存级强隔离），也因此**同时只能 1 个 in-flight
//!   调用**，故 WASM 域强制 `Serial`（A3，装载期由 `Manifest::validate` 拒绝 concurrent）。
//! - **B3 三重时间上限**：`fuel_limit`（指令计数）+ `epoch_tick`（墙钟中断）+ `host_timeout`（单次调用超时）。
//! - 架构与 `agent-kernel-process` 同构：本 crate 提供实现 `trait Plugin` 的 `WasmPlugin` 代理，
//!   内核把它当普通插件注册；kernel 侧的 `WasmDomain` 只负责语义（强制串行）。

use agent_kernel_core::{Envelope, KernelError, KernelResult, Manifest, PluginId};
use agent_kernel_sdk::{Plugin, PluginContext};
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex as AsyncMutex;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

// 新版 wasmtime(48) 的 bindgen! 不再接受 `async` 选项（同时生成同步/异步调用）。
wasmtime::component::bindgen!({
    path: "../../schema/kernel.wit",
    world: "agent-plugin",
});

use agent_kernel::plugin::types::{
    Envelope as WitEnvelope, KernelError as WitKernelError, PluginMeta,
};

/// B3-2 默认墙钟预算（ms）：未配置 `host_timeout_ms` 时用于折算 epoch deadline。
const DEFAULT_EPOCH_BUDGET_MS: u64 = 5_000;

/// host 侧状态：实现 guest 导入的 `host-api`（M1 不回传宿主调用）+ WASI 视图。
///
/// **B3 安全约束**：`WasiCtxBuilder` **不调用** `inherit_network()` / 不挂载 `wasi:sockets`，
/// 故 guest 在结构上无法发起网络请求（与 `docs/03` §5 一致）。
struct Ctx {
    trace_id: String,
    wasi: WasiCtx,
    table: ResourceTable,
}

impl WasiView for Ctx {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

// 说明：wasmtime 48 的 bindgen! 对 wit 的 `result<T, E>` 生成 **普通 Rust Result**：
// host 侧为 `Result<T, E>`（E = wit 的 kernel-error），guest 导出调用为 `Result<Result<T, E>, Trap>`。
impl agent_kernel::plugin::host_api::Host for Ctx {
    fn publish(&mut self, _ev: WitEnvelope) -> Result<String, WitKernelError> {
        Err(WitKernelError::PluginFailed(
            "host-api publish unavailable in M1 (guest 不能回传宿主调用)".into(),
        ))
    }
    fn check_capability(&mut self, _cap: String) -> Result<(), WitKernelError> {
        Err(WitKernelError::PluginFailed(
            "host-api check-capability unavailable in M1".into(),
        ))
    }
    fn trace_id(&mut self) -> String {
        self.trace_id.clone()
    }
}

/// `types` interface 无函数，但 bindgen 仍为其生成（空）`Host` trait，需实现。
impl agent_kernel::plugin::types::Host for Ctx {}

/// wasmtime 48 的 bindgen 要求 `add_to_linker::<T, D>` 提供一个 `HasData` 类型，
/// 其 `Data<'a>` 需实现各 interface 的 `Host`（此处为 `&mut Ctx`）。
struct HostImpl;

impl wasmtime::component::HasData for HostImpl {
    type Data<'a> = &'a mut Ctx;
}

pub struct WasmPlugin {
    manifest: Manifest,
    store: AsyncMutex<Store<Ctx>>,
    bindings: AgentPlugin,
    host_timeout: Option<Duration>,
    fuel_limit: Option<u64>,
    /// B3-2：每次调用允许的 epoch tick 数（墙钟预算 / epoch 间隔）。
    epoch_ticks: u64,
    /// 与引擎 epoch 同步镜像的本地计数器（wasmtime 48 无 epoch 读取器）。
    epoch_counter: Arc<AtomicU64>,
    alive: Arc<AtomicBool>,
}

impl WasmPlugin {
    /// 装载一个 WASM 组件（装载期）。
    pub fn load(
        manifest: Manifest,
        wasm_path: &Path,
    ) -> Result<Self, KernelError> {
        let bytes = std::fs::read(wasm_path).map_err(|e| {
            KernelError::DomainUnavailable(format!("read wasm component failed: {e}"))
        })?;

        let mut cfg = Config::new();
        cfg.wasm_component_model(true);
        cfg.consume_fuel(true);              // B3-1：指令计数
        cfg.epoch_interruption(true);        // B3-2：墙钟中断
        let engine = Engine::new(&cfg).map_err(|e| KernelError::DomainUnavailable(format!("wasmtime engine: {e}")))?;
        let component = Component::new(&engine, &bytes).map_err(|e| {
            KernelError::DomainUnavailable(format!("decode wasm component failed: {e}"))
        })?;

        let mut linker: Linker<Ctx> = Linker::new(&engine);
        // WASI：仅补齐 guest 运行所需的基础接口（io/clocks 等），**不含网络**。
        wasmtime_wasi::p2::add_to_linker_sync::<Ctx>(&mut linker)
            .map_err(|e| KernelError::DomainUnavailable(format!("link wasi failed: {e}")))?;
        AgentPlugin::add_to_linker::<Ctx, HostImpl>(&mut linker, |ctx: &mut Ctx| ctx).map_err(|e| {
            KernelError::DomainUnavailable(format!("link host-api failed: {e}"))
        })?;

        let mut store = Store::new(
            &engine,
            Ctx {
                trace_id: "unknown".into(),
                // 不 inherit_network、不 inherit_stdio：guest 无网络、无宿主文件访问
                wasi: WasiCtxBuilder::new().build(),
                table: ResourceTable::new(),
            },
        );
        // B3-2：把墙钟预算折算成 epoch tick 数（须在建立 store 前算好）
        let interval_ms = manifest.epoch_interval_ms.unwrap_or(10).max(1);
        let budget_ms = manifest
            .host_timeout_ms
            .unwrap_or(DEFAULT_EPOCH_BUDGET_MS)
            .max(interval_ms);
        let epoch_ticks = (budget_ms + interval_ms - 1) / interval_ms;
        let epoch_counter = Arc::new(AtomicU64::new(0));

        // B3-1：consume_fuel(true) 下，实例化（含 guest 初始化）本身也消耗燃料，
        // 必须先预置，否则 store 燃料为 0 会立刻 trap。
        let initial_fuel = manifest.fuel_limit.unwrap_or(100_000_000);
        store
            .set_fuel(initial_fuel)
            .map_err(|e| KernelError::DomainUnavailable(format!("set initial fuel: {e}")))?;
        // epoch deadline 默认为 0，必须显式给出绝对截止值，否则一 increment 就 trap。
        // wasmtime 48 的 increment_epoch() 无返回值，故用本地计数器镜像引擎 epoch。
        let cur_epoch = epoch_counter.load(Ordering::Relaxed);
        store.set_epoch_deadline(cur_epoch + epoch_ticks);

        let bindings = AgentPlugin::instantiate(&mut store, &component, &linker)
            .map_err(|e| KernelError::DomainUnavailable(format!("instantiate component failed: {e}")))?;

        // 装载期调用 meta() 校验身份（与 manifest.name 对齐）
        let meta: PluginMeta = bindings
            .agent_kernel_plugin_plugin()
            .call_meta(&mut store)
            .map_err(|e| KernelError::DomainUnavailable(format!("meta() trap: {e}")))?;
        let id_str = serde_json::to_value(&manifest.name)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        if meta.id != id_str {
            return Err(KernelError::DomainUnavailable(format!(
                "wasm component id mismatch: manifest={id_str}, component={}",
                meta.id
            )));
        }

        let fuel_limit = manifest.fuel_limit;
        let host_timeout = manifest.host_timeout_ms.map(Duration::from_millis);

        // B3-2：epoch 推进任务（插件存活期间周期性 increment_epoch）
        let alive = Arc::new(AtomicBool::new(true));
        let epoch_engine = engine.clone();
        let epoch_alive = alive.clone();
        let epoch_count = epoch_counter.clone();
        let tick = Duration::from_millis(interval_ms);
        tokio::spawn(epoch_engine_loop(epoch_engine, epoch_alive, epoch_count, tick));

        Ok(Self {
            manifest,
            store: AsyncMutex::new(store),
            bindings,
            host_timeout,
            fuel_limit,
            epoch_ticks,
            epoch_counter,
            alive,
        })
    }

    async fn call_on_event(&self, env: Envelope) -> KernelResult<Value> {
        let wit_env = to_wit_envelope(&env)?;
        let mut store = self.store.lock().await;
        let st: &mut Store<Ctx> = &mut *store; // 显式再借用（泛型参数不做 deref 强转）
        if let Some(fuel) = self.fuel_limit {
            st.set_fuel(fuel)
                .map_err(|e| KernelError::DomainUnavailable(format!("fuel: {e}")))?;
        }
        // B3-2：为本次调用设置墙钟截止（绝对 epoch 值）
        let cur_epoch = self.epoch_counter.load(Ordering::Relaxed);
        st.set_epoch_deadline(cur_epoch + self.epoch_ticks);
        // B3：guest 调用的时间上限由 fuel（指令计数）+ epoch（墙钟中断）保证——
        // 这是 wasmtime 的原生机制，能真正拦住纯计算死循环与慢 guest。
        // `host_timeout` 用于约束 host call（M1 无 host call），此处保留待接入。
        let _ = self.host_timeout;
        let result = self
            .bindings
            .agent_kernel_plugin_plugin()
            .call_on_event(st, &wit_env);
        match result {
            Ok(Ok(Some(resp))) => Ok(payload_to_value(&resp.payload)),
            Ok(Ok(None)) => Ok(Value::Null),
            Ok(Err(e)) => Err(KernelError::DomainUnavailable(format!("wasm plugin error: {e:?}"))),
            // fuel/epoch 耗尽或 guest trap，统一收敛为 K700
            Err(trap) => Err(KernelError::DomainUnavailable(format!("wasm trap: {trap}"))),
        }
    }
}

async fn epoch_engine_loop(
    engine: Engine,
    alive: Arc<AtomicBool>,
    counter: Arc<AtomicU64>,
    tick: Duration,
) {
    while alive.load(Ordering::Relaxed) {
        engine.increment_epoch();
        counter.fetch_add(1, Ordering::Relaxed); // 镜像引擎内部 epoch 计数
        tokio::time::sleep(tick).await;
    }
}

fn to_wit_envelope(env: &Envelope) -> KernelResult<WitEnvelope> {
    let target = serde_json::to_value(&env.target)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".into());
    let trace_id = serde_json::to_value(&env.trace_id)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".into());
    let payload = serde_json::to_vec(&env.payload)
        .map_err(|e| KernelError::Internal(format!("payload serialize: {e}")))?;
    Ok(WitEnvelope {
        trace_id,
        span_id: String::new(),
        ty: target,
        payload,
        priority: env.priority as u8,
        deadline_ms: env.deadline.map(|d| d.as_millis() as u64),
    })
}

fn payload_to_value(bytes: &[u8]) -> Value {
    serde_json::from_slice::<Value>(bytes).unwrap_or(Value::Null)
}

#[async_trait]
impl Plugin for WasmPlugin {
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
        self.call_on_event(env).await
    }
    fn destroy(&self) -> KernelResult<()> {
        // 幂等通知：停止 epoch 推进任务；线性内存随 Store 丢弃回收。
        self.alive.store(false, Ordering::Relaxed);
        Ok(())
    }
}
