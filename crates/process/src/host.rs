//! host 侧：`ProcessPlugin` —— 实现 `trait Plugin` 的进程插件代理（gRPC）。
//!
//! 流程：spawn 子进程 → 读 stdout 首行 `PORT=<n>` → gRPC 连接 `127.0.0.1:<n>`
//! → `Handshake` 协商 ApiVersion → 之后 `on_event` 经 `OnEvent` 一请求一响应
//! （unary）往返（`idempotency_key` 承载 seq，支持并发语义下的乱序回复关联）。
//! deadline 超时 → K502；连接断开 → 在途请求收敛 K700。

use crate::pb::v1::{
    plugin_service_client::PluginServiceClient, DrainReq, Envelope as PbEnvelope, HandshakeReq,
    InitReq,
};
use agent_kernel_core::{Envelope, KernelError, KernelResult, Manifest, PluginId};
use agent_kernel_sdk::{Plugin, PluginContext};
use async_trait::async_trait;
use serde_json::Value;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::sync::Mutex as AsyncMutex;

pub struct ProcessPlugin {
    manifest: Manifest,
    client: Arc<AsyncMutex<PluginServiceClient<tonic::transport::Channel>>>,
    child: Arc<Mutex<tokio::process::Child>>,
    next_seq: AtomicU64,
}

impl ProcessPlugin {
    /// spawn 子进程并完成 gRPC 连接 + 握手（装载期）。
    pub async fn spawn(
        manifest: Manifest,
        cmd: &mut tokio::process::Command,
    ) -> Result<Self, KernelError> {
        // stdin 置空（不再走 stdio 协议）；stdout 仅用于读取 PORT 首行
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::inherit());
        let mut child = cmd.spawn().map_err(|e| {
            KernelError::DomainUnavailable(format!("spawn plugin process failed: {e}"))
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| KernelError::DomainUnavailable("no stdout".into()))?;

        // ---- 读首行 PORT=<n>（5s 硬上限）----
        let mut lines = tokio::io::BufReader::new(stdout).lines();
        let port_line = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
            .await
            .map_err(|_| KernelError::DomainUnavailable("guest port line timeout".into()))?
            .map_err(|e| KernelError::DomainUnavailable(format!("read port failed: {e}")))?
            .ok_or_else(|| KernelError::DomainUnavailable("guest exited before announcing port".into()))?;
        let port: u16 = port_line
            .strip_prefix("PORT=")
            .ok_or_else(|| {
                KernelError::DomainUnavailable(format!("bad first line: {port_line}"))
            })?
            .parse()
            .map_err(|e| KernelError::DomainUnavailable(format!("bad port: {e}")))?;

        // ---- gRPC 连接 ----
        let endpoint = format!("http://127.0.0.1:{port}");
        let channel = tokio::time::timeout(Duration::from_secs(5), async {
            tonic::transport::Endpoint::from_shared(endpoint.clone())
                .map_err(|e| KernelError::DomainUnavailable(format!("bad endpoint: {e}")))?
                .connect()
                .await
                .map_err(|e| KernelError::DomainUnavailable(format!("connect failed: {e}")))
        })
        .await
        .map_err(|_| KernelError::DomainUnavailable("connect timeout".into()))??;

        // ---- 握手（Handshake RPC，装载期）----
        let mut client = PluginServiceClient::new(channel);
        let resp = tokio::time::timeout(
            Duration::from_secs(5),
            client.handshake(HandshakeReq {
                api_version: manifest.api_version.to_string(),
                capabilities: vec![],
            }),
        )
        .await
        .map_err(|_| KernelError::DomainUnavailable("handshake timeout".into()))?
        .map_err(|e| KernelError::DomainUnavailable(format!("handshake rpc failed: {e}")))?
        .into_inner();
        if !resp.compatible {
            return Err(KernelError::DomainUnavailable(format!(
                "handshake rejected: {}",
                resp.reason
            )));
        }

        Ok(Self {
            manifest,
            client: Arc::new(AsyncMutex::new(client)),
            child: Arc::new(Mutex::new(child)),
            next_seq: AtomicU64::new(1),
        })
    }

    async fn call_on_event(&self, env: Envelope) -> KernelResult<Value> {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let req = to_proto(&env, seq)?;

        // 一请求一响应（unary）。Concurrent 语义下允许并发在途 RPC：锁内仅克隆
        // client（tonic 克隆共享同一 Channel，开销极低），RPC 在锁外 await——
        // 在途请求不阻塞后续调用。串行化由 ProcessDomain 的插件级锁按
        // manifest.semantics 保证，不在此重复。
        let mut client = self.client.lock().await.clone();
        let reply = client
            .on_event(req)
            .await
            .map_err(|e| KernelError::DomainUnavailable(format!("on_event rpc failed: {e}")))?
            .into_inner();
        Ok(payload_to_value(&reply.payload))
    }
}

fn to_proto(env: &Envelope, seq: u64) -> KernelResult<PbEnvelope> {
    let target = serde_json::to_value(&env.target)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    let trace_id = serde_json::to_value(&env.trace_id)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    let payload = serde_json::to_vec(&env.payload)
        .map_err(|e| KernelError::Internal(format!("payload serialize: {e}")))?;
    Ok(PbEnvelope {
        trace_id,
        span_id: String::new(),
        ty: target,
        payload,
        priority: env.priority as u32,
        deadline_ms: env.deadline.map(|d| d.as_millis() as u64),
        idempotency_key: seq.to_string(), // seq 关联（并发乱序回复）
    })
}

fn payload_to_value(bytes: &[u8]) -> Value {
    serde_json::from_slice::<Value>(bytes).unwrap_or(Value::Null)
}

#[async_trait]
impl Plugin for ProcessPlugin {
    fn id(&self) -> PluginId {
        self.manifest.name.clone()
    }
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    async fn init(&self, ctx: &PluginContext) -> KernelResult<()> {
        let mut client = self.client.lock().await;
        tokio::time::timeout(
            Duration::from_secs(10),
            client.init(InitReq {
                config_json: ctx.config.raw.to_string(),
            }),
        )
        .await
        .map_err(|_| KernelError::DomainUnavailable("init timeout".into()))?
        .map_err(|e| KernelError::DomainUnavailable(format!("init rpc failed: {e}")))?;
        Ok(())
    }
    async fn on_event(&self, env: Envelope) -> KernelResult<Value> {
        self.call_on_event(env).await
    }
    fn destroy(&self) -> KernelResult<()> {
        // 1) 同步强杀：start_kill 立即发出终止信号，不依赖宿主运行时存活。
        //    （修复：此前 kill 走 fire-and-forget 任务，宿主在 destroy 后立刻关闭
        //    运行时会导致 kill 任务未执行 → guest 进程残留。）
        let _ = self.child.lock().unwrap().start_kill();
        // 2) 幂等 best-effort Destroy RPC：进程多半已被 kill，失败静默。
        if let Ok(h) = tokio::runtime::Handle::try_current() {
            let client = Arc::clone(&self.client);
            h.spawn(async move {
                if let Ok(mut c) = client.try_lock() {
                    let _ = c
                        .destroy(tonic::Request::new(crate::pb::v1::Empty {}))
                        .await;
                }
            });
        }
        Ok(())
    }
}

// DrainReq 被 init/destroy 之外的路径使用时才需要；当前仅协议完整性保留
#[allow(dead_code)]
fn _unused(_: DrainReq) {}
