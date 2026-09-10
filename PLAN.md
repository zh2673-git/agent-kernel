# agent-kernel 优化迭代方案（模块级 PLAN）

> 定位：本文件是内核的**演进轨迹**，每轮迭代更新「迭代记录」；README/docs 只在契约实际变更时同步最终形态。
> 方法论：`react-agent/project-development-prompt.md`「优化模式」§3.4。
> 演进模式：**螺旋**——内核定义不变量 → 居民（agent）作为试金石构建 → 居民审计的偏离与契约回灌本 PLAN → 内核修订打 tag → 下一代居民在更严谨的地基上出生。
> 创建：2026-09-10 | 依据：首个居民 react-agent（v0.1.13，运行于 v0.1.2）四轮架构审计回灌。

## 〇、北极星（谱系判定，不可妥协的验收标准）

**一个新 agent，不改内核一行、不改既有 agent 一行、host 装配之外零代码，即可上线运行。**

等价于四个"同源"全部成立：

| 同源 | 内容 | 状态 |
|---|---|---|
| 1. 实例契约 | `Plugin` trait + Manifest（A1/A2 等不变量） | ✅ v0.1.2 已闭合 |
| 2. **寻址契约** | 按 capability 找到提供者（而非按插件名硬编码） | ❌ 本轮主攻（K2） |
| 3. 运行不变量 | A/B/C/K 族全部机器可验证 | ⚠️ K1 补齐后闭合 |
| 4. 验收套件 | 内核级 conformance 测试 = 任何居民的出生证明 | ❌ K 阶段建设 |

## 一、本质定义（内核自身的时空契约）

- **本质**：居民的时空管家——开辟空间（registry/init/destroy）、编排时间（调度/deadline/事件流）、强制规则（依赖解析/生命周期状态机/故障隔离）。
- **空间契约**：`Registry`（ArcSwap）是插件空间归属的唯一真相源，读侧无锁；插件私有状态归插件，跨插件只传 Value payload。
- **时间契约**：同步 dispatch 调用流为**主**时间流（调度 acquire → deadline select → drain）；事件总线为**可选**通道（v0.1.3 起显式声明）。
- **规则契约**：三级拦截——编译期（trait/类型）、装载期（manifest validate / PLUGIN_ABI / 依赖解析 K302）、运行期（B2 panic 隔离 / B4 drain / K4xx）。

### 本质竞品锚定记录（AI 推断，供用户逐项确认）

| 竞品 | 本质定义（AI 推断） | 与本内核的时空差异 | 用户确认 |
|------|-------------------|------------------|---------|
| OSGi（Java） | 类加载器隔离的模块热插拔容器 | 空间：以 ClassLoader 隔离换热插拔，代价是类空间泄漏这一顽疾；本内核以进程/value 传递隔离，规则拦截前移到 manifest + trait | [ ] |
| Linux 内核模块 | 编译期强耦合内核符号表的内核态插件 | 空间：共享内核地址空间，一个模块 panic 全内核崩溃；本内核 B2 隔离 panic 到单插件卸载 | [ ] |
| VS Code 扩展宿主 | 独立进程 + JSON-RPC API 的扩展容器 | 时间：宿主提供能力、扩展被调用（方向与本内核相同），但无依赖解析/世代/调度原语；本内核把这些做成一等不变量 | [ ] |
| LangGraph / LangChain | 应用层编排框架（图/链即代码） | 层级：它是"agent 本身"，无运行时语义（无生命周期状态机/故障隔离/热替换）；本内核是它之下的地基——二者是层关系不是竞品 | [ ] |
| MCP | 工具协议（线格式） | 层级：只定义"调用长什么样"，不定义"运行时怎么管"；本内核的 Process 域线协议与它同层可互操作，但不解决调度/隔离 | [ ] |
| **本内核** | **管理居民空间归属与时间调度的轻量运行时，不变量以命名条目（A/B/C/K 族）交付** | — | — |

> 锚定结论：本内核的差异化不在"能插拔"（各家都有），而在**插拔安全性被做成命名不变量 + 调度原语**（世代 CAS、drain、panic 隔离、依赖解析）。V0.1.3/V0.2.0 的全部工作都是把这个差异化补完整，而非新增能力。

## 二、审计结论（回灌自 react-agent，2026-09-10）

### 根因（一句话）

四个缺陷全部是**半成品机制**，不是缺功能——开了头没有闭环。半成品比缺失更伤严谨：它制造"好像该用"的模糊性，迫使每个居民各自猜测正确用法（react-agent 对 EventBus / step() / hot_swap / PluginConfig 的全部偏离均源于此）。

| # | 问题 | 位置 | 分类 | 严重度 |
|---|------|------|------|--------|
| K1 | **hot_swap 不 drain**：`unload` 走 `Draining + drain_wait(5s)`，`hot_swap` 直接 CAS，在途调用跨世代执行 → 取消通道/会话态断裂 | `crates/kernel/src/application/mod.rs:90-144`（对照 `:67-72`） | 规则（B4 未闭合于热替换路径） | 高 |
| K2 | **capability 寻址半成品**：manifest 声明 capability、依赖按 capability 解析（K302），但 dispatch 按 `PluginId` 寻址，capability 不参与路由 → 调用方硬编码插件名 → "可替换"在内核层未成立 | `crates/kernel/src/interfaces/dispatch.rs:20`；`domain/dependency.rs:41-46`（cap_index 用完即弃） | 规则（寻址契约缺失） | **高（谱系堵点）** |
| K3 | **事件通道形态不明**：EventBus 单消费者（rx 仅可 take 一次）、无持久化、`run()` 可选（无人消费时 publish 在 1024 满后阻塞）；`Plugin::step()` 全仓零实现零调用（死 API） | `infrastructure/bus.rs:19,38-43`；`sdk/src/plugin.rs:38` | 规则（机制未闭环） | 中 |
| K4 | `HostApi::trace_id()` 每次返回新 TraceId，与 `Envelope.trace_id` 口径含糊；`now_mono()` 与 A2（相对时长）口径并存无说明 | `sdk/src/host.rs` | 工程 | 低 |

**居民侧旁证**（react-agent 的偏离全部有裁决记录，见其 `crates/agent-loop/PLAN.md`）：事件通道用持久文件（优于内存总线）、配置走 env（待收编）、替换走停机重启（K1 修复前的最优解）。

## 三、内核工程纪律（贡献门槛，自本轮起生效）

1. **机制闭环三要素**：每引入一个机制，必须同时交付——①命名的失败语义（错误码）②写明的不变量 ③验收测试。三者缺一不合入；**宁可不提供，不提供半截**。
2. **严谨 = 契约闭合，不是功能完备**：地基的危险不是不够强大，而是太想强大（变成框架）。新增能力必须由真实居民需求驱动（本 PLAN 的每一条都来自 react-agent 审计）。
3. **读侧无锁惯例**：高频路径解析结果用 `ArcSwap` 缓存（registry 先例），不新增锁争用点。
4. **失败即显式**：拒绝静默回退（fail-fast / fail-closed），错误进 K 族编号。

## 四、路线

### 版本纪律（0.x.y 语义约定，先于阶段定义）

- `z` +1：缺陷修复 / 非破坏性新增（新 API 必须带默认实现，旧实现者零改动）。
- `y` +1（0.1 → 0.2）：**仅限破坏性变更**（删/改公开 API、行为契约变化）——本方案不包含任何此类变更，该跳变留给未来真正违约之日。
- 删除公开 API：先 deprecate 一个版本（文档标注），删除推迟到首个破坏性版本。
- 阶段序号（P0–P3）≠ 版本号：阶段是演进叙事，版本号只由变更类别决定；不产生内核变更的阶段不发布内核 tag。

| 阶段 | 内核发布 | 内容 | 对应审计项 | 状态 |
|-------|------|------|-----------|------|
| P0 还债 | **v0.1.3** | K1 hot_swap 补 drain；K3 EventBus 收口 + step() 标注 deprecated；K4 口径澄清；invariants 测试补齐 | K1 K3 K4 | 待实施 |
| P1 寻址 | **v0.1.4** | K2 `HostApi::call_capability` + 动态解析 + ArcSwap 缓存 + 多 provider 规则（非破坏新增） | K2 | 待实施 |
| P2 制度 | 无内核发布（新 crate conformance v0.1.0） | conformance 套件上收内核（居民出生证明） | 谱系同源 4 | 待实施 |
| P3 证明 | 无内核发布 | 第二个最小居民诞生（不改地基上线） | 北极星 | 待实施 |

### 生命周期钩子落点（本轮改动的 init/start/stop/destroy 影响，§1.2 强制项）

| 钩子（内核对应物） | 现状 | 本轮改动影响 | 落点 |
|---|---|---|---|
| `init`（空间开辟） | register_instance 内：校验 → 依赖解析 → 建域 → init（失败回滚装载） | V0.2.0 不改 init 流程，仅注册后刷新 cap 缓存 | `application/mod.rs::register_instance` |
| `start`（时间流） | `Kernel::run()` 事件主循环（可选，react-agent 未启用） | V0.1.3 仅文档收口 + publish 快速失败，不改启动语义 | `runtime/mod.rs::run` |
| `stop`（时间流暂停） | `Kernel::stop()` notify | 不涉及 | — |
| **热替换路径**（drain → 迁移 → CAS → destroy 旧） | **缺陷：无 drain，直接 CAS** | **V0.1.3 K1 主刀**：CAS 前补 `drain_wait`，B4 不变量在热替换路径闭合 | `application/mod.rs::hot_swap` |
| `destroy`（空间回收） | unload / hot_swap 末尾调用（幂等通知，真正释放在 Drop） | 不涉及 | `application/mod.rs::unload` |

---

### Phase P0「还债」→ 内核 v0.1.3（预计 1 天）

**K1 hot_swap 补 drain**（`application/mod.rs`，~8 行）：
- CAS 切换前插入 `self.scheduler.drain_wait(id, timeout)`（timeout 建议 5s，与 unload 对齐）；drain 超时按现 unload 语义强制继续。
- 语义变化仅影响 hot_swap 调用方（当前为零），属缺陷修复。

**K3 EventBus 收口**：
- `bus.rs` / `runtime/mod.rs` 文档显式声明：事件总线为**可选调试通道**——不持久化、单消费者、必须先 `run()`；未 run 时 `publish_event` 快速失败（新增 K 族错误，拒绝静默积压）。
- `sdk/src/plugin.rs` 的 `step()` **标注 `#[deprecated]`**（全仓已核验零实现零调用，无人受伤；真正删除推迟到首个破坏性版本，见版本纪律）。

**K4**：`host.rs` 文档澄清 `trace_id()` 语义（建议标注 deprecated，指向 `Envelope.trace_id`）；`now_mono()` 注明与 A2 的口径关系（相对时长测量的时钟源）。

**验证契约（P/Q/I）**：
- P：invariants 测试环境就绪（`cargo test -p agent-kernel-kernel` 全绿基线）。
- Q：新增用例——在途慢调用存在时发起 `hot_swap`，断言切换发生在调用收敛之后；`publish_event` 未 run() 时返回错误而非阻塞。
- I：`HostApi` 既有实现者（KernelHost）零改动即编译通过；react-agent 将内核依赖 tag 升至 v0.1.3 后 77 项测试全绿。

---

### Phase P1「寻址」→ 内核 v0.1.4（预计 1–2 天，唯一结构性变更，仍为非破坏新增）

**方案**：
- `domain/dependency.rs`：`DependencyGraph` 增存 `cap_index: HashMap<Capability, PluginId>`（resolve 时已在构建，`or_insert` = **先注册者胜**，写成文档约定）+ 只读访问器。
- `runtime/mod.rs`：解析结果以 `ArcSwap<HashMap<Capability, PluginId>>` 缓存（注册/卸载/热替换时刷新），读侧无锁（纪律 3）。
- `sdk/src/host.rs`：`HostApi` 新增
  `async fn call_capability(&self, capability: &str, payload: Value, deadline: Duration) -> Result<Value, KernelError>`
  **带默认实现返回 `UnknownCapability`** → 既有实现者零改动（向后兼容）。
- `interfaces/host.rs`：`KernelHost` 实现——解析 cap → id → 复用 `dispatch::dispatch`（天然继承 B5/B2/B4/deadline 全部保护）。
- `core/error.rs`：新增 `KernelError::UnknownCapability(Capability)`。
- **解析时机：每次调用动态查当前缓存**（非注册期固化）→ 与 `hot_swap` 天然兼容，换实现后新调用自动流向新实例。

**明确不做**：同 capability 多 provider 的路由策略（负载均衡/优先级）——先注册者胜已够谱系使用，策略化等真实需求。

**验证契约（P/Q/I）**：
- P：注册提供 `llm.chat` 的插件 A。
- Q：`call_capability("llm.chat", ...)` 命中 A；`hot_swap` 换 B 后新调用命中 B；未注册 capability → `UnknownCapability`（K 族）。
- I：既有 77+ 内核测试全绿；react-agent 不采纳新 API 也零回归（默认实现兼容性）；并发 10k 次解析无锁争用退化。

---

### Phase P2「制度」→ 新 crate conformance v0.1.0（内核 tag 不动；预计 2–3 天）

- 内核新增 `crates/conformance`（或 `kernel/tests/conformance/`）：任何居民实现的**出生证明**——
  ① Plugin trait 契约（init 失败回滚、on_event 幂等边界、destroy 幂等）；
  ② 调度不变量（max_inflight / 总闸 / deadline / Draining 拒绝）；
  ③ panic 隔离（B2：panic → Failed + 卸载）；
  ④ 热替换安全（K1 后：drain 后切换、世代 CAS、K504 暴露）；
  ⑤ dylib 装载验收（ABI 守卫 fail-closed）。
- 来源：react-agent 四轮审计提炼的验收清单上收泛化（居民特有部分——事件 schema/旁路协议——留在居民仓）。
- 产出：`cargo test -p conformance --features suite` 可对任意实现了 `Plugin` 的 crate 跑通 = 谱系成员资格。

### Phase P3「证明」→ 内核零改动（发布动作全部在居民侧）

- 最小问答 agent（仅 `llm.chat` + `memory.session`，数百行）。
- **验收 = 北极星判据**：不改内核、不改 react-agent、host 装配仅增两行注册，即可运行。
- 这是比任何代码审查都硬的体系严谨性检验。

## 已评估不做（防重复尝试）

| 项 | 理由 | 重评条件 |
|---|------|---------|
| EventBus 持久化 / 多消费者 | 居民侧已证明文件 trace 通道在持久性/重放/回滚上更优；无第二个事件消费者 | 出现真实的多消费者居民（审计/指标插件） |
| Process 域 guest→host 回调 | 破坏"能力=被调用"的简洁性，双向流是内核级大立项 | 出现必须跨语言编排的真实居民 |
| WASM 域启用 | 无居民需求；框架骨架已就位 | 同上 |
| 装配参数化（ORCHESTRATOR=...） | 属居民仓 host 的事，不进内核 | react-agent 侧另立 |
| 删除 dylib | K2 之后它是"不停机换实现"的装载通道，保留 | — |

## 迭代记录

### P0（内核 v0.1.3）：还债 ✅（2026-09-10，决策：保留）

- **目标**：闭合 B4 于热替换路径（K1）；事件通道形态显式化（K3）；口径澄清（K4）。
- **方案**：
  - K1：`application/mod.rs::hot_swap` CAS 前插入 `scheduler.drain_wait(id, 5s)`（与 unload 同语义，超时强制继续）。
  - K3：`KernelHost::emit` 在 `running==false` 时快速失败（新增 `KernelError::EventBusNotRunning`，**K505**）；
    `bus.rs` / `runtime::run` 文档声明"可选调试通道"契约（内存态、单消费者、不持久化）；
    `step()` 标注 `#[deprecated(since="0.1.3")]`（实删推迟至首个破坏性版本，全仓已核验零实现零调用）。
  - K4：`HostApi::trace_id` 标注 deprecated（真相源 = `Envelope.trace_id`）；`now_mono` 文档注明与 A2 配对口径。
- **验证（P ✅ / Q ✅ / I ✅）**：
  - P：core/sdk/kernel 三 crate 编译通过。
  - Q：新增回归 `k1_hot_swap_waits_for_inflight_to_drain`（在途 400ms、100ms 时发起 swap → 断言 100ms 处未完成、drain 后切换、B5 旧实例完成旧请求、新请求走新实例；修复前必失败）与
    `k3_emit_fails_fast_without_run`（未 run() 时 emit 返回 K505 而非静默入队；run() 后成功；修复前必失败）。
  - I：内核 invariants 8/8 + dylib_e2e 1/1 全绿；workspace check 零警告；react-agent 经 [patch] 本地内核跑全量 **93 项全绿零回归**（I 项达成：既有 KernelHost 实现零改动编译通过）。
- **决策**：保留（P/Q/I 全过）。
- **遗留**：① git tag v0.1.3 待打（打后 react-agent 以 tag 依赖复验一次）；② `step()`/`trace_id()` 实删随首个破坏性版本；
  ③ react-agent 侧存量瑕疵 `crates/host/src/config.rs:426`（test 内 unused `n`，与本轮无关，待居民侧清理）；
  ④ react-agent 的 `[patch]` 已恢复注释（tag 未打前 tag 依赖不受影响）。

### P1（内核 v0.1.4）/ P2（conformance v0.1.0）/ P3（无内核发布）
（待实施；每轮按「目标 → 方案 → P/Q/I 验证 → 保留或回退」追加记录）
