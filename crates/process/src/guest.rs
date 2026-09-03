//! guest 侧：gRPC server（`PluginService`）。
//!
//! 启动时绑定 `127.0.0.1:0`，向 stdout 首行打印 `PORT=<n>`（内核据此连接），
//! 随后服务到 stdin 之外的生命周期 RPC。收到 `Destroy` 后正常返回。

use crate::pb::v1::{
    plugin_service_server::{PluginService, PluginServiceServer},
    DrainReq, DrainResp, Empty, Envelope as PbEnvelope, HandshakeReq, HandshakeResp, HealthResp,
    InitReq, InitResp, PluginMeta, SnapshotReq, SnapshotResp,
};
use agent_kernel_core::{Envelope, Event, KernelError, KernelResult, PluginState};
use agent_kernel_sdk::{
    GlobalConfig, HostApi, KernelContext, PluginConfig, PluginContext, PluginInstance, TraceId,
};
use async_trait::async_trait;
use serde_json::Value;
use std::io::Write;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::watch;
use tonic::{Request, Response, Status};

/// guest 侧 HostApi：M1 不回传宿主调用（插件间通信暂不支持跨进程），一律错误。
struct GuestHost {
    #[allow(dead_code)]
    start: Instant,
}

#[async_trait]
impl HostApi for GuestHost {
    async fn emit(&self, _event: Event) -> KernelResult<()> {
        Err(KernelError::DomainUnavailable(
            "guest cannot emit to kernel over M1 gRPC transport".into(),
        ))
    }
    async fn call_plugin(&self, _env: Envelope) -> KernelResult<Value> {
        Err(KernelError::DomainUnavailable(
            "guest cannot call other plugins over M1 gRPC transport".into(),
        ))
    }
    fn now_mono(&self) -> u64 {
        self.start.elapsed().as_nanos() as u64
    }
    fn trace_id(&self) -> TraceId {
        TraceId::new()
    }
}

fn parse_version(v: &str) -> (Option<u16>, u16) {
    let s = v.trim().trim_start_matches('v');
    let mut it = s.split('.');
    let major = it.next().and_then(|x| x.parse().ok());
    let minor = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    (major, minor)
}

struct Svc {
    plugin: PluginInstance,
}

#[tonic::async_trait]
impl PluginService for Svc {
    async fn handshake(&self, req: Request<HandshakeReq>) -> Result<Response<HandshakeResp>, Status> {
        let want = req.into_inner();
        let mine = self.plugin.manifest().api_version;
        let (wmaj, wmin) = parse_version(&want.api_version);
        let ok = mine.major == wmaj.unwrap_or(u16::MAX) && mine.minor >= wmin;
        if ok {
            Ok(Response::new(HandshakeResp {
                compatible: true,
                reason: String::new(),
            }))
        } else {
            // 不兼容：返回 compatible=false，宿主拒绝装载（K201 语义）
            Ok(Response::new(HandshakeResp {
                compatible: false,
                reason: format!(
                    "api_version mismatch: kernel wants {}, plugin supports {}",
                    want.api_version, mine
                ),
            }))
        }
    }

    async fn meta(&self, _req: Request<Empty>) -> Result<Response<PluginMeta>, Status> {
        let m = self.plugin.manifest();
        Ok(Response::new(PluginMeta {
            id: serde_json::to_value(&m.name)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            version: m.version.to_string(),
            api_version: m.api_version.to_string(),
        }))
    }

    async fn init(&self, req: Request<InitReq>) -> Result<Response<InitResp>, Status> {
        let m = self.plugin.manifest().clone();
        let config = PluginConfig {
            raw: serde_json::from_str(&req.into_inner().config_json).unwrap_or(Value::Null),
        };
        let (_, rx) = watch::channel(PluginState::Loading);
        let host: Arc<dyn HostApi> = Arc::new(GuestHost { start: Instant::now() });
        let ctx = PluginContext::new(
            m,
            config,
            rx,
            KernelContext::new(host, GlobalConfig::default()),
        );
        self.plugin
            .init(&ctx)
            .await
            .map_err(|e| Status::failed_precondition(format!("{} {}", e.code(), e)))?;
        Ok(Response::new(InitResp {}))
    }

    /// 一请求一响应（unary）：请求与回复经 `idempotency_key` 关联 seq。
    async fn on_event(&self, request: Request<PbEnvelope>) -> Result<Response<PbEnvelope>, Status> {
        let env = request.into_inner();
        let seq = env.idempotency_key.clone();
        let payload: Value = serde_json::from_slice(&env.payload).unwrap_or(Value::Null);
        let core_env = Envelope::new(agent_kernel_sdk::PluginId::new(&env.ty), payload);
        match self.plugin.on_event(core_env).await {
            Ok(v) => {
                let bytes = serde_json::to_vec(&v).unwrap_or_default();
                Ok(Response::new(PbEnvelope {
                    trace_id: env.trace_id,
                    span_id: String::new(),
                    ty: format!("{}.ok", env.ty),
                    payload: bytes,
                    priority: 0,
                    deadline_ms: None,
                    idempotency_key: seq,
                }))
            }
            Err(e) => Err(Status::internal(format!("{} {}", e.code(), e))),
        }
    }

    async fn snapshot(&self, _req: Request<Empty>) -> Result<Response<SnapshotResp>, Status> {
        Err(Status::unimplemented("snapshot unsupported in M1"))
    }
    async fn restore(&self, _req: Request<SnapshotReq>) -> Result<Response<Empty>, Status> {
        Err(Status::unimplemented("restore unsupported in M1"))
    }
    async fn drain(&self, _req: Request<DrainReq>) -> Result<Response<DrainResp>, Status> {
        Ok(Response::new(DrainResp {}))
    }
    async fn start(&self, _req: Request<Empty>) -> Result<Response<Empty>, Status> {
        Ok(Response::new(Empty {}))
    }
    async fn stop(&self, _req: Request<Empty>) -> Result<Response<Empty>, Status> {
        Ok(Response::new(Empty {}))
    }
    async fn destroy(&self, _req: Request<Empty>) -> Result<Response<Empty>, Status> {
        let _ = self.plugin.destroy();
        Ok(Response::new(Empty {}))
    }
    async fn health(&self, _req: Request<Empty>) -> Result<Response<HealthResp>, Status> {
        Ok(Response::new(HealthResp {
            ok: true,
            detail: String::new(),
        }))
    }
}

/// 运行插件进程：绑定随机端口 → stdout 首行 `PORT=<n>` → 服务直到被 kill/断开。
pub async fn serve(
    plugin: PluginInstance,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    println!("PORT={port}");
    std::io::stdout().flush()?;
    tonic::transport::Server::builder()
        .add_service(PluginServiceServer::new(Svc { plugin }))
        .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
        .await?;
    Ok(())
}
