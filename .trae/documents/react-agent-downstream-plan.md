# 下游 Agent 项目实施计划（react-agent，基于 agent-kernel v0.1.0）

## Context

用户要基于 agent-kernel（GitHub: zh2673-git/agent-kernel，tag v0.1.0）构建一个 agent。按 `project-development-prompt.md` 的时空方法论，一切能力皆为插件。决策已确认：

- **独立下游项目**（git 依赖内核，不顺带改内核源码）→ 同时闭环内核路线图「外部依赖验证」项
- **跨语言验证**：Python + TS 插件（走 Process 域 gRPC），Rust 编排
- **LLM 三家全接**：OpenAI 兼容（含 DeepSeek 等，可配 base_url）、Anthropic、Ollama
- **全套四件插件**：agent-loop、llm-adapter、tools、memory

**关键内核约束**（已核实）：Process 域 guest 无 guest→host 回调通道，故 agent-loop 必须跑 Rust InProcess 域（`HostApi::call_plugin` 仅进程内可用，但可跨域调用 Process 插件）。

**内核事实核查结论**（规划代理已验证，实现时遵守）：
1. `Manifest` 无 `::empty()`，需逐字段构造 15 字段；无 `Envelope.ty` 路由——路由只看 `env.target`，op 分派放 payload 内的 `"op"` 字段
2. `PluginContext.config` 恒为 `PluginConfig::default()`；Process guest 的 `Init` 收到 `config_json="null"` → **配置一律走环境变量**（子进程继承 env）
3. `Kernel::register` **吞错误**（仅日志）：硬依赖无 provider ⇒ K302 静默失败 → **注册顺序：memory → llm-adapter → tools → agent-loop（最后）**，注册 agent-loop 前先探测各 provider
4. Host crate 需 `agent-kernel-kernel` 开 `features = ["process"]` + `agent-kernel-process`
5. guest 的 `api_version` 必须 `ApiVersion::new(0, 1)`（握手要求 guest major==host major 且 minor>=host minor）
6. git 依赖可行：cargo 在 git checkout 内部解析 kernel 的 workspace path 依赖，无需 vendor
7. 本机环境已验证：node v22.16.0（≥22.6 ✅）、python 3.11.5、kernel 仓在 `D:\ZH\vibe code\skills\tool\agent-kernel`

## 项目结构

```
D:\ZH\vibe code\skills\tool\react-agent\
├─ Cargo.toml                        # workspace（依赖见下）
├─ README.md                         # 运行说明 + 完整 wire 契约文档
├─ crates/
│  ├─ agent-loop/                    # Rust InProcess 插件：ReAct 状态机
│  │  ├─ Cargo.toml                  # 仅依赖 agent-kernel-sdk + serde/serde_json/async-trait/tokio
│  │  ├─ src/lib.rs                  # AgentLoopPlugin
│  │  ├─ src/contract.rs             # 共享 serde 结构：ChatReq/Resp、LlmChatReq/Resp、ToolSpec、ToolCall、MemoryMsg
│  │  └─ tests/react_mocks.rs        # 纯 Rust mock 测试（不需要 python/node）
│  └─ host/
│     ├─ Cargo.toml                  # agent-kernel-kernel(process) + agent-kernel-process + agent-loop(path)
│     ├─ src/main.rs                 # CLI：装配内核、spawn guest、注册、单轮/REPL
│     ├─ src/config.rs               # HostConfig::from_env：LLM_PROVIDER/LLM_MODEL/LLM_BASE_URL/OPENAI_API_KEY/ANTHROPIC_API_KEY/OLLAMA_HOST/AGENT_KERNEL_REPO/PLUGINS_DIR/MAX_ROUNDS
│     ├─ src/manifests.rs            # guest_manifest(id, caps) + agent_loop_manifest()
│     ├─ src/spawn.rs                # spawn_python()/spawn_node_ts()（设 PYTHONPATH）
│     └─ tests/                      # common/mod.rs + 4 个 e2e（见测试节）
└─ plugins/
   ├─ llm_adapter/llm_plugin.py      # 单文件；openai_compat/anthropic/ollama/mock（httpx）
   ├─ tools/tools_plugin.py          # 单文件；纯 stdlib
   └─ memory/
      ├─ guest_sdk.ts                # re-export shim → kernel checkout 的 bindings/typescript/src/index.ts
      └─ memory_plugin.ts            # Map 会话存储 + 每会话 JSON 文件持久化
```

## Workspace Cargo.toml 要点

```toml
[workspace.dependencies]
agent-kernel-sdk     = { git = "https://github.com/zh2673-git/agent-kernel", tag = "v0.1.0" }
agent-kernel-kernel  = { git = "https://github.com/zh2673-git/agent-kernel", tag = "v0.1.0", features = ["process"] }
agent-kernel-process = { git = "https://github.com/zh2673-git/agent-kernel", tag = "v0.1.0" }
```

- 插件 crate 只依赖 `agent-kernel-sdk`（重导出全部 core 类型）；仅 host 加 kernel+process
- 注释形式的 `[patch."https://github.com/zh2673-git/agent-kernel"]` 段：本地开发可切到 `../agent-kernel/crates/*` path 依赖

## 各插件设计

### manifests（精确值）

| 字段 | agent-loop | llm-adapter(py) | tools(py) | memory(ts) |
|---|---|---|---|---|
| name | agent-loop | llm-adapter | tools | memory |
| kind | Orchestrator | Capability | Capability | Capability |
| api_version | (1,0) | **(0,1)** | **(0,1)** | **(0,1)** |
| capabilities | ["agent.chat"] | ["llm.chat"] | ["tools.exec"] | ["memory.session"] |
| dependencies | 硬依赖 memory.session / llm.chat / tools.exec | [] | [] | [] |
| domain | InProcess | Process | Process | Process |
| semantics | Serial | Serial | Serial | Serial |
| max_inflight | 4 | 4 | 8 | 16 |
| subscriptions | [] | [] | [] | [] |

### agent-loop（Rust InProcess）

- `AgentLoopPlugin{max_rounds:8, 各转发 deadline, host:OnceLock<Arc<dyn HostApi>>, manifest}`，`new(max_rounds) -> PluginInstance`；init 时经 `ctx.host` 捕获 host（模式照抄 skeleton-tools）
- `on_event` 按 `payload["op"]=="chat"` 进入 ReAct 循环（状态在局部变量+memory 插件，&self 无跨调用可变态，A1 合规）：
  1. **感知**：`call_plugin("memory", {"op":"get","session_id"})`（deadline 5s）→ messages = [system] + history + [user]
  2. **规划**：`call_plugin("tools", {"op":"list"})`（每请求一次）→ `call_plugin("llm-adapter", {"op":"chat","messages","tools"})`（deadline 120s），转发时复制入站 `trace_id`
  3. **行动**：有 `tool_calls` 则逐个 `call_plugin("tools", {"op":"call","name","args"})`（deadline 10s）；失败合成 `{"ok":false,"error":...}` 作为 tool 消息回喂，**不中断循环**
  4. **观察**：`call_plugin("memory", {"op":"append","messages":[assistant(tool_calls)+tool 结果...]})` → 回到 2（超 `max_rounds` 后最后一轮不带 tools，强制收敛）
  5. 最终 answer → memory.append → 返回 `ChatResp`
- plugin 业务错误走 payload（`{"ok":false,"error":{code,message}}`），传输/生命周期错误才走 `KernelError`

### llm-adapter（Python guest）

- 作者接口（照 echo_plugin.py 模式）：class 含 `manifest()/{id,version,api_version:"0.1"}`、`init(config)`（config 可能为 None）、`on_event(envelope_dict)`（envelope.payload 为 JSON）、`destroy()`；结尾 `serve(plugin)`，`from agent_kernel.guest import serve`
- 依赖仅 `httpx`（不用 openai/anthropic SDK：一个纯 py 依赖覆盖四 provider，v1 非流式两个 POST 足够）
- provider 按请求 `payload["provider"]` 覆盖，否则 env `LLM_PROVIDER`：openai_compat（base_url 可配，覆盖 OpenAI/DeepSeek）/ anthropic（`anthropic-version: 2023-06-01` 头）/ ollama（`http://{OLLAMA_HOST}:11434/v1`，OpenAI 兼容）/ **mock**（`MOCK_SCRIPT` env = JSON 数组，逐次弹出——离线 e2e 的关键）
- 归一化两种 tool-call 形状：OpenAI 兼容 `tool_calls[].function.arguments` 为 JSON **字符串**需 parse；Anthropic 为 `content[]` 中 `type:"tool_use"` 的 `input` 对象 → 统一 `{id,name,arguments(object)}`

### tools（Python guest）

- ops：`list` / `call{name,args}`；内置工具纯 stdlib：
  - `calculator`：`ast` 模块 AST-walker 求值（白名单 BinOp/UnaryOp/Constant/±*/÷/%/**/FloorDiv/USub），**禁 eval**
  - `current_time`：ISO-8601
  - `http_get`：`urllib.request`，scheme 白名单 http/https，10s 超时

### memory（TypeScript guest）

- duck-typed `Plugin` 接口（照 echo_plugin.ts 模式）：`manifest()/init/onEvent/destroy`；strip-types 安全（无 enum/namespace/参数属性）
- `Map<session_id, MemoryMsg[]>` + append/clear 时重写 `<MEMORY_DATA_DIR|./data>/sessions/<id>.json`（Serial 语义下安全）
- `guest_sdk.ts` 单行 re-export：`export { serve, type Plugin, type PluginManifest } from "../../../../agent-kernel/bindings/typescript/src/index.ts";`——裸说明符 `@grpc/grpc-js` 从其自身目录向上解析到 kernel 的 `bindings/typescript/node_modules`（需一次 `npm install`）

### spawn（host/src/spawn.rs，照 python_guest.rs/node_guest.rs 模式）

- `spawn_python(script)`：`Command::new(python|python3).arg(script).env("PYTHONPATH", <AGENT_KERNEL_REPO>/bindings/python)` → `ProcessPlugin::spawn(guest_manifest(...), &mut cmd)`
- `spawn_node_ts(script)`：`Command::new("node").args(["--experimental-strip-types", script])`
- 路径解析：`AGENT_KERNEL_REPO` env，默认 `env!("CARGO_MANIFEST_DIR")/../..` 旁的 `../agent-kernel`；`PLUGINS_DIR` 默认 `<workspace>/plugins`

## Wire 契约（写入 README「Contracts」节）

- **agent-loop** `{"op":"chat","session_id","user_text"}` → `{"ok":true,"answer","rounds","session_id"}` | `{"ok":false,"error":{...}}`
- **llm-adapter** `{"op":"chat","provider"?,"messages":[{"role","content","tool_calls"?,"tool_call_id"?}],"tools"?}` → `{"ok":true,"content","tool_calls":[{id,name,arguments}],"model","finish_reason","usage"?}`
- **tools** `{"op":"list"}` → `{"ok":true,"tools":[{name,description,parameters}]}`；`{"op":"call","name","args"}` → `{"ok":true,"result"}` | `{"ok":false,"error"}`
- **memory** `{"op":"append","session_id","messages"}` / `{"op":"get","session_id","limit"?}` / `{"op":"clear","session_id"}`；`MemoryMsg` 与 llm messages 同构（单一 canonical schema 定义于 contract.rs，两侧 guest 镜像）

## 实施步骤

1. 搭 workspace（Cargo.toml + 目录骨架）→ `cargo build` 首次拉 git 依赖，验证 git 依赖解析
2. `crates/agent-loop`：contract.rs → lib.rs（ReAct 循环）→ tests/react_mocks.rs（6 个 mock 测试）
3. `plugins/llm_adapter/llm_plugin.py`（含 mock provider）
4. `plugins/tools/tools_plugin.py`
5. `plugins/memory/`（guest_sdk.ts + memory_plugin.ts）
6. `crates/host`：config/manifests/spawn/main（注册顺序 memory→llm→tools→agent-loop，agent-loop 前探测各 provider）→ 4 个 e2e 测试
7. README（运行说明 + wire 契约 + 环境变量表）
8. 本地 git init + commit（内核仓不动；可后续自行建 GitHub 远程）

## 测试与验证

**单元/集成（纯 Rust，无需运行时）**：`cargo test -p react-agent-agent-loop`
- react_answers_without_tool_calls / react_executes_tool_call_and_feeds_observation / react_reaches_max_rounds / react_appends_history_and_final_to_memory / react_tool_error_is_observed_not_fatal / agent_loop_without_providers_is_not_dispatchable

**跨语言 e2e（`cargo test -p react-agent-host`，缺解释器时优雅 skip）**：
- tools_list_and_calculator_roundtrip（python，`1+2*3→7`）
- memory_append_get_clear_roundtrip（node ts）
- llm_mock_echo_and_scripted_toolcall（LLM_PROVIDER=mock + MOCK_SCRIPT）
- full_react_loop_with_mock_llm_and_real_guests（三 guest + 真 agent-loop，mock 脚本 [tool_call(calculator "2+2"), final]，断言 answer=="answer is 4"、rounds==2）

**手工端到端**：`LLM_PROVIDER=ollama LLM_MODEL=<支持工具的模型> cargo run -p react-agent-host -- "你的问题"`（单轮）或无参进 REPL

**前置**：`pip install grpcio httpx`；kernel 仓 `bindings/typescript` 下 `npm install`（一次）

## 风险与对策

| 风险 | 对策 |
|---|---|
| grpcio Windows 轮子 | `pip install --only-binary=:all:`（py3.11 有轮子） |
| node strip-types | 需 ≥22.6（已 22.16）；插件代码避免 enum/namespace |
| TS/Py 依赖 kernel checkout 位置 | TS 隔离在单 shim 文件；Python 由 host 设 PYTHONPATH + AGENT_KERNEL_REPO 兜底；未来发包解决 |
| Ollama 模型不支持工具 | 循环仍收敛（模型直接作答）；README 推荐工具支持模型；mock 保测试恒绿 |
| 注册顺序错→K302 静默 | 固定顺序 + provider 探测（可读报错） |
| 首次 build 需网络拉 git 依赖 | 文档注明 `cargo vendor` 离线替代 |
