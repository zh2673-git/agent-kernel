# agent-kernel

> GitHub：**https://github.com/zh2673-git/agent-kernel** ｜ 许可证：MIT OR Apache-2.0 ｜ 进度快照见 `docs/PROGRESS.md`

一个 **agent 运行时内核**：内核只做三件事——**开辟/隔离插件的空间、编排插件的执行时间流、强制校验插件契约与权限**。所有具体能力（LLM 适配器、工具、记忆、规划/编排、提示词、向量库……）全部以**插件**形式挂载到内核，并支持**不停机热替换**。

---

## 设计原则：时空契约

内核 = **空间隔离 + 时间调度 + 规则校验**，任何代码最终都落到这三者：

- **空间契约**：每个插件拥有独立 `PluginContext`（自身状态/配置/资源句柄）；内核持有只读共享 `KernelContext`（全局配置、事件总线句柄）。插件间**禁止直接内存互访**，跨插件通信一律走事件总线。热替换时旧插件状态需"可丢弃"或"可快照迁移"。
- **时间契约**：主执行流为**事件循环 Loop**（`while let Some(env) = bus.recv().await`）；agent 主循环本身是状态机（感知→规划→行动→观察），由"编排插件"（本身也是插件）驱动。
- **规则契约**：编译时以 `trait Plugin` + `ApiVersion` 限定接口；运行时加载校验符号/契约 + 能力声明（`Capability`）。新插件必须满足同版本契约，否则拒绝加载。

**纯内核边界（本质）**：内核只提供原语，**不提供任何业务能力；一切能力皆为插件**。执行域适配器（`InProcess/Wasm/Process`）是内核的一部分（时空规则的执行者），不是插件。

---

## 目录结构

```
agent-kernel/
├─ crates/
│  ├─ core/        规则基座：id / version / capability / envelope / event / error / lifecycle / manifest
│  ├─ sdk/         插件唯一可见依赖：Plugin trait / HostApi / PluginContext / KernelContext / Migratable
│  ├─ kernel/      内核单体：runtime / domain / application / infrastructure / interfaces
│  ├─ plugins/     业务插件（骨架示例）：skeleton-llm / skeleton-tools / echo-dylib（cdylib，plugin! 宏）
│  ├─ process/     Process 执行域：host 侧 ProcessPlugin 代理（gRPC client）+ guest 侧 gRPC serve（stub 由 schema/kernel.proto 生成）+ 示例 guest + e2e
│  ├─ wasm/        WASM 执行域：host 侧 WasmPlugin 代理（wasmtime 组件模型装载 kernel.wit 组件，B3 fuel/epoch 双限制，WASI 无网络）
│  ├─ wasm-guest/  WASM guest 示例：wit-bindgen 导出 kernel.wit 的 plugin 接口，编译为 wasm32-wasip2 组件
│  ├─ demo/        宿主二进制：把插件 register 进内核并开机
│  └─ xtask/       架构校验脚本（依赖方向守卫）
├─ bindings/       跨语言 L3 SDK：python（agent_kernel，grpcio + 生成的 stub）/ typescript（src/index.ts，@grpc/grpc-js + proto-loader 运行时加载 proto），与 Rust 同一 gRPC 协议 + echo 示例
├─ schema/         契约唯一源（L2：kernel.wit / kernel.proto；L1：plugin-manifest / capability 两个 JSON Schema）
├─ docs/           设计文档树（00~07）+ program.md（设计纲要）+ PROGRESS.md（进度快照）
├─ LICENSE-MIT / LICENSE-APACHE  双许可（MIT OR Apache-2.0）
└─ README.md       本文件
```

**依赖方向铁律**：`interfaces → application → domain ← infrastructure → core`；插件只允许依赖 `sdk`（编译期硬禁依赖 `kernel` 内部模块）。这是**防规则穿透**的物理手段——插件永远碰不到内核内部。

**crate 划分**：`core` 零出向依赖（纯规则定义）；`sdk` 仅依赖 `core`；`kernel` 依赖 `core`+`sdk`+内部各层；`plugins/*` 仅依赖 `sdk`；`demo` 依赖 `kernel`+各 `plugins`。

---

## 插件契约（三层）与 SDK 的角色

"插件接口"不是单一接口，而是三层：

| 层级 | 名称 | 载体 | 说明 |
|---|---|---|---|
| L1 | 语义契约 | 文档 + JSON Schema | 身份/能力/事件模型/生命周期/状态/错误/时间语义 |
| L2 | 线协议 ABI | `kernel.wit` / `kernel.proto` | **真正的插件接口**（跨语言、跨动态库） |
| L3 | 语言绑定 | Rust trait / Python / TS | L2 的方言（Rust 跑进程内域；Python/TS 经 gRPC 跑进程域，各自带 echo 示例） |

> 关键：Rust 无稳定 ABI，`Box<dyn Plugin>` 跨动态库边界有 UB 风险，故 `trait Plugin` 降级为 L3 绑定，L2 才是唯一契约源。

**SDK（`crates/sdk`）是插件作者面对的全部世界**（`crates/sdk/src/lib.rs`）：

```rust
pub use agent_kernel_core::*;
pub use context::{GlobalConfig, KernelContext, PluginConfig, PluginContext};
pub use host::HostApi;
pub use migratable::Migratable;
pub use plugin::{Plugin, PluginInstance};
```

插件作者只 `use agent_kernel_sdk::*`，实现 `Plugin` trait，即可被内核加载、调度、门控、热替换。

---

## 核心模块速览

| 模块 | 职责 |
|---|---|
| **Registry**（`domain/registry`） | 插件身份↔私有空间的账本 + 依赖图；`ArcSwap` 整体原子换代；路由索引按优先级预排序 |
| **LifecycleManager + DrainCoordinator**（`domain/lifecycle`） | 插件状态机唯一仲裁者；in-flight 计数 + 超时熔断（热替换不撕裂时间流） |
| **Scheduler + EventBus**（`runtime/scheduler`） | 事件循环 Loop + 4 条有界优先级泳道 + 背压；状态用 `watch`、事件用 `broadcast` |
| **ExecutionDomain**（`runtime/domain`） | 三种隔离强度：`InProcess`（亚微秒）/ `Wasm`（线性内存强隔离）/ `Process`（独立进程自愈） |
| **CapabilityGate**（`domain/capability`） | 能力门控，**deny-by-default**（插件能碰哪些外部空间） |
| **HotSwap**（`application/hotswap`） | 事务性热替换：validate→load→[snapshot→restore]→start→CAS→drain→destroy→失败回滚 |
| **Observability**（`runtime/observability`） | 内核承担跨插件 `trace_id`（ULID）串联；插件自打日志无法完成链路追踪 |
| **Snapshot/Migration**（`domain/snapshot`） | 热替换时旧状态"可丢弃或可快照迁移"的协议（`trait Migratable`） |

一次事件的时间线：

```
publish(event)
 → 分配 trace_id，构造 Envelope{priority, deadline}
 → 入优先级泳道（满则触发背压策略）
 → 主循环 recv() 加权取出 → 开 span（注入 trace_id）
 → Registry.route(event.ty) → 命中订阅者（已按 priority 排序）
 → 对每个订阅者：CapabilityGate.check → DrainCoordinator.acquire → 按 time_semantics 串行/spawn
 → ExecutionDomain.dispatch()（panic 边界在此）
 → 结果写回 / 错误 record_error + PluginFailed
 → 闭 span，trace 导出
```

---

## 开发插件（本仓库内）

本仓库既是内核，也自带 `demo` 与骨架插件。在仓库内新增一个插件（例如 `my-llm`）：

1. 在 `crates/plugins/` 新建文件夹 `my-llm`。
2. 写 `Cargo.toml`，仅依赖 `sdk`：
   ```toml
   [package]
   name = "my-llm"
   edition = "2021"

   [dependencies]
   agent-kernel-sdk = { path = "../../sdk" }
   serde_json = "1"
   async-trait = "0.1"
   ```
3. 在 `crates/Cargo.toml` 的 `members` 中加入 `"plugins/my-llm"`。
4. 写 `src/lib.rs`，实现 `Plugin`（最小骨架）：
   ```rust
   use agent_kernel_sdk::{Plugin, PluginContext, PluginInstance, Envelope, KernelResult, Manifest, PluginId};
   use async_trait::async_trait;

   struct MyLlm;

   #[async_trait]
   impl Plugin for MyLlm {
       fn id(&self) -> PluginId { PluginId::new("my-llm") }
       fn manifest(&self) -> &Manifest {
           static M: Manifest = Manifest::empty("my-llm");
           &M
       }
       async fn init(&self, _ctx: &PluginContext) -> KernelResult<()> { Ok(()) }
       async fn on_event(&self, env: Envelope) -> KernelResult<serde_json::Value> {
           Ok(serde_json::json!({"echo": env.payload}))
       }
       fn destroy(&self) -> KernelResult<()> { Ok(()) }
   }

   pub fn instance() -> PluginInstance { std::sync::Arc::new(MyLlm) }
   ```
   > 铁律（A1）：所有方法均为 `&self`，插件内部可变性自行承担（Mutex / actor mailbox）；句柄恒为 `Arc<dyn Plugin>`；`destroy` 是幂等通知，真正释放在 `Drop`。
5. 在 `demo` 中注册：`kernel.register(my_llm::instance()).await;`。
6. 运行：`cargo run -p demo`。

---

## 下游项目如何引用本内核（不改动内核源码）

本仓库发布后，一个**全新的 agent 项目应把内核当作外部依赖**，而非把内核源码搬进自己的 `crates/`。这样内核的 `crates/`（core/kernel/sdk）始终保持不动，下游只通过 `sdk` 这扇门与之交互。

**依赖方式**（三选一）：

```toml
# ① GitHub git 依赖（已发布，推荐）
[workspace.dependencies]
agent-kernel-sdk    = { git = "https://github.com/zh2673-git/agent-kernel" }
agent-kernel-kernel = { git = "https://github.com/zh2673-git/agent-kernel" }

# ② 开发期：本地 path 指向本仓库
# agent-kernel-sdk    = { path = "../agent-kernel/crates/sdk" }
# agent-kernel-kernel = { path = "../agent-kernel/crates/kernel" }

# ③ crates.io 版本依赖（尚未发布，后续可选）
# agent-kernel-sdk    = "0.1"
# agent-kernel-kernel = "0.1"
```

**推荐的下游项目结构**：内核不在本仓库内，下游自己的 `crates/` 只放 host（主程序）+ 各插件，每个都可按四层模型（core/domain/infrastructure/application/runtime/interfaces）独立分层，互不干扰：

```
my-agent/                         ← 下游项目根（产品）
├─ Cargo.toml                     ← workspace，内核以外部依赖引入
├─ crates/
│  ├─ host/                       ← 主程序（相当于本仓库的 demo），内部自有四层结构
│  └─ plugins/
│     ├─ my-llm/                  ← 插件1，内部可再分层
│     └─ agent-loop/              ← 插件2，内部可再分层
└─ （agent-kernel 不在本仓库，作为外部依赖）
```

要点：
- 内核源码**永不进入**下游仓库，故"内核文件不动"天然成立。
- 下游的 host 与每个插件各自遵循四层模型递归展开；层级互相隔离，跨单元边界只有 `sdk` 定义的 `Plugin` 接口。
- 内核看不见 host/插件的内部结构，只认 `Plugin` 这扇门——这正是"规则防穿透"。

---

## 四种装载方式速查

| 方式 | 域 | 隔离强度 | 适用 | 构建/装载 |
|---|---|---|---|---|
| 进程内注册 | `InProcess` | 弱（同内存，`catch_unwind` 收敛） | 可信的同语言插件，零开销快车道 | 实现 `Plugin` → `kernel.register(...)` |
| 子进程 | `Process` | 进程边界，崩溃自愈 | Rust/Python 等任意语言（gRPC，stub 由 `kernel.proto` 生成；guest 启动打印 `PORT=<n>` 由内核连接） | `ProcessPlugin::spawn(manifest, cmd)` |
| WASM 组件 | `Wasm` | 线性内存强隔离，无网络 | 不可信插件 | `xtask build-wasm` → `WasmPlugin::load(...)` |
| dylib 动态库 | `InProcess` | 同进程（**同编译器契约**） | 同 rustc + 同版本 sdk 的插件独立分发 | `xtask build-dylib` → `sdk::dylib::load(...)` |

---

## 热替换（零停机）

内核用**代际（generation）**机制：新旧版本短暂双版本共存（内存/CPU 瞬时翻倍，以可用性优先换取一致性延迟），旧世代的在途请求继续跑完（绝不撕裂时间流），新请求经 `ArcSwap` 的 `compare_switch`（CAS）切到新世代，旧版 `drain` 完成后 `destroy`。未实现 `Migratable` 的插件走"丢弃 + 冷启动"。

---

## 当前进度

| 能力 | 状态 |
|---|---|
| 进程内域 + Registry + Scheduler + Lifecycle + CapabilityGate + HotSwap | ✅ 已跑通（骨架插件热替换闭环） |
| L2 契约源 `schema/`（kernel.wit / kernel.proto / 两个 JSON Schema） | ✅ 已建立（`cargo run -p xtask -- gen-schema` 可重新生成） |
| `ProcessDomain`（进程域） | ✅ 已跑通（**gRPC 传输**：tonic + protobuf，stub 由 `schema/kernel.proto` 经 `build.rs` 生成；`OnEvent` 为一请求一响应 unary RPC；e2e 验证内核 spawn Rust/Python/node 子进程插件并往返 dispatch；握手协商 ApiVersion，EOF/崩溃收敛为 K700，deadline 超时 K502）。需本机安装 `protoc` |
| `WasmDomain`（WASM 域） | ✅ 已跑通（wasmtime 组件模型装载 `kernel.wit` 组件；B3 燃料+epoch 双时间上限；WASI p2 基础接口、**不开放网络**；e2e 验证内核装载 `wasm32-wasip2` 组件并往返 dispatch）。构建组件：`cargo run --bin xtask -- build-wasm` |
| dylib 动态装载（`plugin!` 宏 + dlopen） | ✅ 已跑通（sdk 导出 `_KERNEL_PLUGIN_ABI` 守卫 + `_kernel_plugin_create`；加载器纯 std 实现 LoadLibrary/dlopen；构造 panic 被 `catch_unwind` 收敛为 null。**同编译器契约**：dylib 与宿主须同 rustc + 同版本 sdk，否则 ABI 守卫拒绝）。构建插件：`cargo run --bin xtask -- build-dylib` |
| 跨语言 L3 绑定（Python / TS） | ✅ 已跑通（`bindings/python` 经 `grpcio` + `grpc_tools.protoc` 生成 stub；`bindings/typescript` 经 `@grpc/grpc-js` + `@grpc/proto-loader` 运行时加载 `kernel.proto`，免代码生成；e2e 验证内核 spawn python / `node --experimental-strip-types` 插件并往返 dispatch；与 Rust guest 同一 gRPC 协议） |

---

## 后续路线

1. ✅ Process 域传输已升级为 gRPC（tonic + protobuf，详见上文；Python/TS SDK 已同步升级）。
2. ✅ 内核已发布 GitHub（git 依赖可引用）；待用第一个真实外部插件项目验证「仅依赖、不改内核源码」路径；crates.io 发布可选。
3. 充实示例插件：LLM 适配器、agent 主循环、记忆、工具注册。
4. （可选）流式 RPC：需要 LLM token 流等真流式时，于 `kernel.proto` 新增 `OnEventStream` 双向流 RPC（已留注释位）。

---

> 更底层的设计理由见 `docs/`（架构选型、模块四层设计、插件契约与跨语言、热替换与状态迁移、插件依赖图与加载顺序）。
