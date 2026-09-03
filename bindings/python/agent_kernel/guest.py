"""agent-kernel Python 插件进程运行时（guest 侧，gRPC）。

流程：绑定 `127.0.0.1:0` → stdout 首行打印 `PORT=<n>`（内核据此连接）
→ 服务 `PluginService`（stub 由 schema/kernel.proto 生成）。

插件需实现（duck typing）：
- manifest() -> {"id": str, "version": str, "api_version": str}
- init(config: dict) -> None
- on_event(envelope: dict) -> 任意可 JSON 序列化值
- destroy() -> None
"""

import json
import sys
from concurrent import futures

import grpc

from agent_kernel._proto import kernel_pb2, kernel_pb2_grpc


def _parse_version(v):
    s = str(v).strip().lstrip("v")
    parts = s.split(".")
    major = int(parts[0]) if parts and parts[0].isdigit() else None
    minor = int(parts[1]) if len(parts) > 1 and parts[1].isdigit() else 0
    return major, minor


class _Servicer(kernel_pb2_grpc.PluginServiceServicer):
    def __init__(self, plugin):
        self._plugin = plugin

    # -- 装载期 ------------------------------------------------------------
    def Handshake(self, request, context):
        mine = str((self._plugin.manifest() or {}).get("api_version", "0.1"))
        pmaj, pmin = _parse_version(mine)
        wmaj, wmin = _parse_version(request.api_version)
        if pmaj is not None and pmaj == wmaj and pmin >= wmin:
            return kernel_pb2.HandshakeResp(compatible=True, reason="")
        return kernel_pb2.HandshakeResp(
            compatible=False,
            reason=f"api_version mismatch: kernel wants {request.api_version}, plugin supports {mine}",
        )

    def Meta(self, request, context):
        meta = self._plugin.manifest() or {}
        return kernel_pb2.PluginMeta(
            id=str(meta.get("id", "")),
            version=str(meta.get("version", "0.0.0")),
            api_version=str(meta.get("api_version", "0.1")),
        )

    def Init(self, request, context):
        try:
            config = json.loads(request.config_json) if request.config_json else {}
        except json.JSONDecodeError:
            config = {}
        try:
            self._plugin.init(config)
        except Exception as e:  # noqa: BLE001 — 收敛为 gRPC 状态
            context.abort(grpc.StatusCode.FAILED_PRECONDITION, f"init failed: {e}")
        return kernel_pb2.InitResp()

    # -- 事件（unary：请求与回复以 idempotency_key 承载 seq 关联）----------
    def OnEvent(self, request, context):
        try:
            payload = json.loads(request.payload) if request.payload else None
        except json.JSONDecodeError:
            payload = None
        try:
            value = self._plugin.on_event({"target": request.ty, "payload": payload})
        except Exception as e:  # noqa: BLE001
            context.abort(grpc.StatusCode.INTERNAL, f"on_event failed: {e}")
        return kernel_pb2.Envelope(
            trace_id=request.trace_id,
            ty=f"{request.ty}.ok",
            payload=json.dumps(value).encode("utf-8"),
            idempotency_key=request.idempotency_key,
        )

    def Snapshot(self, request, context):
        context.abort(grpc.StatusCode.UNIMPLEMENTED, "snapshot unsupported in M1")

    def Restore(self, request, context):
        context.abort(grpc.StatusCode.UNIMPLEMENTED, "restore unsupported in M1")

    def Drain(self, request, context):
        return kernel_pb2.DrainResp()

    def Start(self, request, context):
        return kernel_pb2.Empty()

    def Stop(self, request, context):
        return kernel_pb2.Empty()

    def Destroy(self, request, context):
        try:
            self._plugin.destroy()
        except Exception:  # noqa: BLE001 — 收尾失败不再上报
            pass
        return kernel_pb2.Empty()

    def Health(self, request, context):
        return kernel_pb2.HealthResp(ok=True, detail="")


def serve(plugin, max_workers: int = 4) -> None:
    """启动 gRPC server 并阻塞，直到被内核停止。"""
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=max_workers))
    kernel_pb2_grpc.add_PluginServiceServicer_to_server(_Servicer(plugin), server)
    port = server.add_insecure_port("127.0.0.1:0")
    server.start()
    # 首行 PORT=<n>：内核读取后连接（必须立即 flush）
    sys.stdout.write(f"PORT={port}\n")
    sys.stdout.flush()
    server.wait_for_termination()
