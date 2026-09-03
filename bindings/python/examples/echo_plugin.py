#!/usr/bin/env python3
"""echo 插件（Python）：被内核以 Process 域 spawn，经 stdio NDJSON 通信。

内核侧装载：
    ProcessPlugin::spawn(manifest(id="echo-py"), Command::new("python").arg(本文件))
"""

import os
import sys

# 使 examples 可直接运行：把 SDK 目录加入 import 路径
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from agent_kernel.guest import serve  # noqa: E402


class EchoPlugin:
    def manifest(self) -> dict:
        return {"id": "echo-py", "version": "0.1.0", "api_version": "0.1"}

    def init(self, config: dict) -> None:
        pass

    def on_event(self, envelope: dict):
        # envelope 即 core::Envelope 的 JSON 镜像：target/trace_id/priority/deadline/payload
        return {"echo": envelope.get("payload"), "from": "python"}

    def destroy(self) -> None:
        pass


if __name__ == "__main__":
    serve(EchoPlugin())
