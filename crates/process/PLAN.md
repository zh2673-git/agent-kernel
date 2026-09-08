# PLAN — crates/process：Concurrent 语义失效修复（Process 域）

> 状态：已完成（2026-09-08） ｜ 发起：2026-09-08 ｜ 触发：react-agent K502 雪崩归因时发现内核 Process 域锁 bug

## 一、逆向解读与审计结论

### 现象
- react-agent 观测（K502 雪崩链）：内核 abort（deadline 超时）后，同插件后续请求卡住排队。
- 内核侧疑点：`ProcessPlugin` 在途 RPC 与 client 锁的关系。

### 审计证据（v0.1.1 tag = 083ac7e 与 HEAD f4e7040 对 host.rs 零差异，两版同病）
1. **bug 本体**：`src/host.rs::call_on_event` 注释声称「锁在调用发起后立即释放」，
   实际 `client.on_event(req).await` 的整个 future 在 `client.lock().await` 守卫作用域内
   完成——**整段 unary RPC 持锁**。任何在途请求未返回，后续全部 `on_event` 在锁上排队。
2. **域层无责**：`crates/kernel/src/interfaces/process_domain.rs` 按 `manifest.semantics`
   决定是否持插件级锁（Serial=持锁串行 / Concurrent=不持锁）；`domain_for` 在每插件
   注册时各建一个域适配器（`kernel/src/application/mod.rs:38`），按插件判定语义无歧义。
3. **契约依据**：`Semantics::Concurrent` = 「插件内部自行并发」（`core/src/manifest.rs`），
   host 应允许并发在途 RPC；guest 侧 tonic server 每 RPC 独立 task，天然支持并发。
   本 crate 顶部文档也写明「idempotency_key 承载 seq 关联，支持并发语义下的乱序回复」——
   **设计意图是并发，实现把它无声串行化了**。

### 定性
- 内核 bug：`ProcessPlugin` 的 client 锁覆盖整个 RPC 往返，使 `Semantics::Concurrent`
  在 Process 域失效（退化为串行），与 L1 语义契约相悖。
- 连锁放大：deadline/K502 场景下，一条卡死的在途 RPC 长期占锁，同插件全部请求被阻塞
  （react-agent 症状 B 的内核侧放大器；上游 Python 侧线程泄漏已在其自身仓库修复）。
- 文档漂移（随本次修正）：host.rs 顶部「OnEvent 双向流」、process_guest.rs「stdio NDJSON」
  均为旧传输时代残留（现为一请求一响应 unary gRPC，见 `schema/kernel.proto` 注释）。

## 二、修复方案

### 唯一改动点：`src/host.rs::call_on_event`
- 锁内仅 `client.clone()`（tonic client 克隆共享同一 HTTP/2 Channel，开销极低），
  守卫随即释放；RPC **在锁外 await**。
- Serial 语义不受影响：串行化本就由 ProcessDomain 的插件级锁保证，ProcessPlugin
  不再重复承担（修复前是双重串行，Serial 路径行为不变）。
- `init`/`destroy` 生命周期 RPC 继续走原锁（装载期/销毁期本就单次，无并发需求）。
- 不改 proto、不改 kernel 域层、不改 `trait Plugin` 契约——**对外契约零变化**。

### 落选方案
- 每请求新建 client：`Endpoint::connect` 每请求一次建链开销，拒。
- 结构体改存 `Channel` 按需构造 client：改动面波及 init/destroy，收益不成比例，拒。
- 锁内 clone 是 tonic 多路复用的标准姿势，改动约 3 行，风险最小。

## 三、迭代记录（P/Q/I）

- P（并行验证）：
  - [x] 新增 `tests/concurrent_semantics.rs`：Concurrent manifest + guest 支持
    `delay_ms` 延迟回显；4×300ms 并发 on_event，断言总耗时 < 1000ms
    （修复前串行 ≥ 1200ms）。实测 0.34s（全并发），并已补 `destroy()` 收尸防孤儿 guest。
  - [x] 存量 `tests/e2e.rs`（Serial 往返）+ `python_guest.rs` + `node_guest.rs` 保持绿。
- Q（质量闸）：`cargo clippy -p agent-kernel-process` 无新告警（lib 仅存量
  `needless_borrows` @ to_proto:124，非本次引入）。
- I（集成闸）：workspace `cargo test --workspace` exit 0 全绿。

## 四、发布与下游

- 版本：workspace `0.1.1` → **`0.1.2`**（bugfix 语义）。
- README：Process 域行补并发语义口径（host 侧按声明语义并发在途，串行由域级锁保证）。
- tag：**v0.1.2**；react-agent 的 git 依赖 tag v0.1.1 → v0.1.2（验证期可临时走
  `[patch]` 本地 path，确认后切回 tag，避免长期悬挂本地路径）。
- 下游回归：react-agent 升依赖 → 全链路 e2e → in-stream abort 行为复查（内核侧
  不再因占锁连锁阻塞，预期 K502 后同插件可立即恢复服务）。
