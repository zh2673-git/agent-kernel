# agent-kernel 进度文档

> 一份持续维护的进度快照。设计纲要见 `program.md`，设计理由见 `docs/`，使用方式见 `README.md`。
> 最后更新：**2026-09-03**（Phase 6 收官，Process 域传输升级为 gRPC）。

---

## 1. 项目定位

一个 **agent 运行时内核**：内核只做三件事——**开辟/隔离插件的空间、编排插件的执行时间流、强制校验插件契约与权限**。所有具体能力（LLM 适配器、工具、记忆、规划/编排、提示词、向量库……）全部以**插件**形式挂载，并支持**不停机热替换**。

**纯内核边界**：内核只提供原语，**不提供任何业务能力；一切能力皆为插件**。执行域适配器（`InProcess`/`Wasm`/`Process`）是内核的一部分（时空规则的执行者），不是插件。

详细设计原则见 `program.md` 与 `README.md` 的「时空契约」一节。

---

## 2. 架构总览

### 2.1 四层模块（内核 crate 内部）
| 层 | 职责 |
|---|---|
| `core` | 规则基座：id / version / capability / envelope / event / error / lifecycle / manifest（纯数据，无副作用） |
| `domain` | Registry（归属表+依赖图）、LifecycleManager+DrainCoordinator、CapabilityGate、Snapshot/Migration |
| `application` | HotSwap（事务性热替换编排） |
| `runtime` | Scheduler+EventBus、ExecutionDomain 三实现、Observability（trace_id 串联） |
| `infrastructure` / `interfaces` | 外部边界适配（进程/子进程/动态库装载器、契约校验入口） |

### 2.2 三层插件契约
| 层级 | 名称 | 语言相关 | 载体 | 说明 |
|---|---|---|---|---|
| L1 | 语义契约 | 无关 | 文档 + JSON Schema | 身份/能力/事件模型/生命周期/状态/错误/时间语义 |
| L2 | 线协议 ABI | 无关 | `kernel.wit` / `kernel.proto` / C 头 | **真正的插件接口**（`schema/` 唯一源） |
| L3 | 语言绑定 | 有关 | Rust trait / Python / TS | 仅是 L2 的一种方言 |

### 2.3 四种执行域 / 装载方式（均已落地）
| 方式 | 域 | 隔离强度 | 传输 | 构建/装载 |
|---|---|---|---|---|
| 进程内注册 | `InProcess` | 弱（同内存，`catch_unwind` 收敛） | 直接 `Arc<dyn Plugin>` | 实现 `Plugin` → `kernel.register(...)` |
| 子进程 | `Process` | 进程边界，崩溃自愈 | **gRPC**（stub 由 `kernel.proto` 生成；guest 启动打印 `PORT=<n>` 由内核连接） | `ProcessPlugin::spawn(manifest, cmd)` |
| WASM 组件 | `Wasm` | 线性内存强隔离，WASI 无网络 | wasmtime 组件模型（装载 `kernel.wit`） | `xtask build-wasm` → `WasmPlugin::load(...)` |
| dylib 动态库 | `InProcess` | 同进程（同编译器契约） | dlopen + `_KERNEL_PLUGIN_ABI` 守卫 | `xtask build-dylib` → `sdk::dylib::load(...)` |

---

## 3. 分阶段完成状态

| 阶段 | 内容 | 状态 |
|---|---|---|
| 阶段一~三 | 内核核心（Registry/Lifecycle/Scheduler/CapabilityGate/HotSwap/Observability/Snapshot）、多执行域（InProcess/Wasm/Process/dylib）、L2 契约源 `schema/` | ✅ 已跑通（README 状态表） |
| 阶段四 | **纯 Rust、无外部依赖**重构（内核与进程/宿主侧零第三方运行时依赖） | ✅ 完成 |
| 阶段五 | **跨语言 L3 绑定**（Python / TS，与 Rust guest 同一线协议 + echo 示例） | ✅ 完成（原 NDJSON/stdio 协议，见阶段六升级） |
| 阶段六 | **Process 域传输升级为 gRPC**（tonic + protobuf，Rust/Python/TS 三端统一） | ✅ 完成 |

### 阶段六交付细节（本次收尾）
- **Rust 侧**：`build.rs` 用 `tonic-prost-build` 编译 `schema/kernel.proto` 生成 stub；`ProcessPlugin` 改为 gRPC 客户端，`guest` 改为 gRPC server（启动打印 `PORT=<n>` 由内核连接 127.0.0.1）。
- **Python 侧**：`grpcio` + `grpc_tools.protoc` 生成 `bindings/python/agent_kernel/_proto/` stub，重写 `guest.py` 为 gRPC server。
- **TS 侧**：`@grpc/grpc-js` + `@grpc/proto-loader` **运行时加载** `kernel.proto`（免代码生成，stub 永远与契约一致）。
- **关键决策**：`OnEvent` 由「双向流式」改为 **unary RPC（一请求一响应）**。原因：实测 tonic 客户端 → grpc-python 服务的**流式请求 DATA 帧送不出去**（Python↔Python、Rust↔Rust 双向流正常，唯独跨运行时流式卡死）。本内核语义本就是一请求一响应，unary 更贴合且消除整类互操作坑。需要真正的流式（如 LLM token 流）时再新增 `OnEventStream` 双向流 RPC（已写入 `kernel.proto` 注释）。

---

## 4. 当前验证状态（2026-09-03 实测）

| 验证项 | 命令 | 结果 |
|---|---|---|
| 全仓构建 | `cargo build --workspace` | ✅ 通过（无 error；wasm-guest/echo-dylib 的两行为 MSVC 导出库常规信息，非告警） |
| 全仓测试 | `cargo test --workspace` | ✅ 全部通过（无失败；wasm 2 个 `ignored` 因未装 `wasm32-wasip2` target） |
| 架构守卫 | `cargo run -p agent-kernel-xtask -- all` | ✅ all checks passed（目录四层结构、插件仅依赖 sdk、Envelope.deadline 字段级 ABI、L1 Schema 存在、无 kernel 反向依赖） |
| Rust gRPC e2e | `process_domain_roundtrip` | ✅ 内核 spawn Rust 子进程插件往返 dispatch |
| Python gRPC e2e | `python_guest_roundtrip` | ✅ 内核 spawn python 插件往返 dispatch |
| TS gRPC e2e | `node_guest_roundtrip` | ✅ 内核 spawn `node --experimental-strip-types` 插件往返 dispatch |
| WASM e2e | `wasm_domain_roundtrip` | ✅ 已跑通（2026-09-03 补验：`wasm32-wasip2` target 已装，`xtask build-wasm` 构建 guest 组件后 dispatch 往返 + 二次串行调用通过） |

---

## 5. 工具链前置

| 工具 | 用途 | 备注 |
|---|---|---|
| Rust (rustc/cargo) | 全部构建 | 需 `tokio`/`tonic`/`prost` 等（均由 Cargo 管理） |
| `protoc` 29.3 | 编译 `kernel.proto` 生成 stub | 已安装；`process`/`bindings/python` 构建时调用 |
| `wasm32-wasip2` target | 编译 WASM guest 组件 | 已安装；未装时 wasm e2e 自动 skip（组件可由 `xtask build-wasm` 重建） |
| Python 3 + `grpcio` + `grpcio-tools` | Python 跨语言 guest | `pip install grpcio grpcio-tools` |
| Node ≥ 22.6（`--experimental-strip-types`） + `@grpc/grpc-js` + `@grpc/proto-loader` | TS 跨语言 guest | `npm install` 于 `bindings/typescript` |

---

## 6. 目录结构与各组件状态

```
agent-kernel/
├─ crates/
│  ├─ core/        规则基座（L1 数据模型）                       ✅
│  ├─ sdk/         插件唯一可见依赖（Plugin trait / HostApi…）   ✅
│  ├─ kernel/      内核单体：runtime/domain/application/infra/interfaces ✅
│  ├─ plugins/     骨架插件：skeleton-llm / skeleton-tools / echo-dylib ✅
│  ├─ process/     Process 域：host(gRPC client) + guest(gRPC server) + e2e ✅
│  ├─ wasm/        WASM 域：wasmtime 装载 kernel.wit 组件        ✅（e2e 待 target）
│  ├─ wasm-guest/  WASM guest 示例（wit-bindgen → wasm32-wasip2）✅
│  ├─ demo/        宿主二进制：register 插件并开机               ✅
│  └─ xtask/       架构校验脚本（依赖方向守卫）                  ✅
├─ bindings/
│  ├─ python/      agent_kernel（grpcio + 生成 _proto/）+ examples ✅
│  └─ typescript/  src/index.ts（@grpc/grpc-js）+ examples       ✅
├─ schema/         L2：kernel.wit / kernel.proto；L1：plugin-manifest / capability（JSON Schema） ✅
├─ docs/           设计文档树 00~07                               ✅
├─ program.md      设计纲要（用户确认的关键决策）                ✅
├─ project-development-prompt.md  开发 skill 提示               ✅
└─ README.md       使用与架构说明                                ✅
```

---

## 7. 常见命令

```bash
# 构建 / 测试 / 架构守卫
cargo build --workspace
cargo test  --workspace
cargo run -p agent-kernel-xtask -- all          # 架构不变量校验
cargo run -p demo                               # 跑主程序 + 骨架插件 + 热替换闭环

# WASM / dylib 插件构建
cargo run -p agent-kernel-xtask -- build-wasm
cargo run -p agent-kernel-xtask -- build-dylib

# 跨语言 guest（需先装对应依赖）
pip install grpcio grpcio-tools        # python
npm install                            # bindings/typescript
cargo test -p agent-kernel-process     # 触发 python/node/rust gRPC e2e
```

插件开发（本仓库内 / 下游外部依赖）详见 `README.md` 第 101、147 行起的两节。

---

## 8. 未完成 / 后续路线

1. **发布为外部依赖**（git / crates.io）：下游项目仅依赖、不改动内核源码（结构已在 README「下游项目如何引用本内核」一节设计好，尚未真正发布版本）。
2. **充实示例插件**：LLM 适配器、agent 主循环、记忆、工具注册（当前仅有 `skeleton-llm` / `skeleton-tools` 骨架）。
3. **（可选）流式 RPC**：若需 LLM token 流等真流式，于 `kernel.proto` 新增 `OnEventStream` 双向流 RPC（已留注释位）。

---

## 9. 本次清理记录

删除的临时/诊断产物（均非源码，可随时重现）：
- `crates/` 下诊断日志：`e.txt` `o.txt` `err.txt` `out.txt` `test_out.txt` `we.txt` `wt.txt` `xe.txt` `xt.txt`
- 项目根诊断日志：`bindgen-dump.txt` `probe_err.txt` `probe_out.txt` `py_err.txt` `py_out.txt` `wasm-build.log`
- Python 缓存：`bindings/python/agent_kernel/__pycache__/` `bindings/python/agent_kernel/_proto/__pycache__/`

保留的设计/文档：`program.md`（设计纲要）、`docs/`、`README.md`、`project-development-prompt.md`、生成的 `bindings/python/agent_kernel/_proto/`（运行 python e2e 所需，可由 `bindings/python/build_proto.py` 重新生成）。
