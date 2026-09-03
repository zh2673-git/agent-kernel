"""agent-kernel Python 插件 SDK（L3 绑定）。

M1 传输：NDJSON over stdio（消息结构镜像 schema/kernel.proto）。
用法见 examples/echo_plugin.py。
"""
from .guest import serve

__all__ = ["serve"]
