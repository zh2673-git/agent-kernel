//! `agent-kernel-process`：Process 执行域（gRPC 传输）。
//!
//! - **传输**：gRPC（tonic + prost），stub 由 `build.rs` 从 `schema/kernel.proto`
//!   生成——L2 契约唯一源。guest 启动时绑定 `127.0.0.1:0` 并向 stdout 首行打印
//!   `PORT=<n>`，内核据此连接（避免固定端口冲突）。
//! - **guest 侧**（`guest::serve`）：插件作者把自己的 `PluginInstance` 交给它，
//!   即成为一个可被内核 spawn 的插件进程（gRPC server）。
//! - **host 侧**（`ProcessPlugin`）：实现 `trait Plugin` 的代理。内核把它当普通
//!   插件注册；`on_event` 经 `OnEvent` 双向流与子进程往返（`idempotency_key`
//!   承载 seq 关联，支持并发语义下的乱序回复）。
//! - **握手**：装载期 `Handshake` RPC 协商 `ApiVersion`（major 相等且 guest
//!   minor >= 内核 minor）；不兼容则 guest 以非零码退出（K201 语义）。

pub mod guest;
pub mod host;
pub mod pb;

pub use host::ProcessPlugin;
