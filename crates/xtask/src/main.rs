//! `xtask`：agent-kernel 实施纪律强制工具。
//!
//! 子命令：
//! - `layering`  ：校验 kernel 四层目录存在 + 插件仅依赖 sdk（C1）。
//! - `check-abi` ：字段级 ABI 校验（Envelope.deadline == Option<Duration>，禁 Instant；
//!                 禁止 `Arc<Mutex<dyn Plugin>>` / `switch_generation` / sdk 内 `unimplemented!()`）（A1/A2/A4/C2/C3）。
//! - `deps`      ：插件 Cargo.toml 不得依赖 kernel（C1 信任边界）。
//! - `all`       ：依次执行上述全部。
//!
//! 退出码：发现违规返回 1。

use std::path::{Path, PathBuf};
use std::process::exit;

fn workspace_root() -> PathBuf {
    // xtask 位于 crates/xtask，根工作区为 crates/
    let dir = env!("CARGO_MANIFEST_DIR");
    Path::new(dir).join("..")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
                out.push(p);
            }
        }
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("✗ {msg}");
    exit(1);
}

fn ok(msg: &str) {
    println!("✓ {msg}");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("all");
    match cmd {
        "layering" => check_layering(),
        "check-abi" => check_abi(),
        "deps" => check_deps(),
        "gen-schema" => gen_schema(),
        "build-wasm" => build_wasm(),
        "build-dylib" => build_dylib(),
        "all" => {
            check_layering();
            check_abi();
            check_deps();
            gen_schema();
            println!("✓ all checks passed");
        }
        other => {
            eprintln!("unknown subcommand: {other}");
            exit(2);
        }
    }
}

fn check_layering() {
    let root = workspace_root();
    let kernel_src = root.join("kernel/src");
    for layer in ["interfaces", "application", "domain", "infrastructure", "runtime"] {
        let d = kernel_src.join(layer);
        if !d.is_dir() {
            fail(&format!("kernel/src 缺少分层目录: {layer}（C1 要求四层结构）"));
        }
    }
    ok("kernel 四层目录齐全 (interfaces/application/domain/infrastructure/runtime)");

    // 插件仅依赖 sdk（C1 信任边界）
    let plugins = root.join("plugins");
    if plugins.is_dir() {
        for entry in std::fs::read_dir(&plugins).unwrap().flatten() {
            let pdir = entry.path();
            if !pdir.is_dir() {
                continue;
            }
            let toml = pdir.join("Cargo.toml");
            let content = read(&toml);
            if content.contains("agent-kernel-kernel") {
                fail(&format!(
                    "{} 依赖了 agent-kernel-kernel（C1 禁止：插件只能依赖 sdk）",
                    pdir.file_name().unwrap().to_string_lossy()
                ));
            }
            if !content.contains("agent-kernel-sdk") {
                fail(&format!(
                    "{} 未依赖 agent-kernel-sdk（插件必须依赖 sdk）",
                    pdir.file_name().unwrap().to_string_lossy()
                ));
            }
        }
    }
    ok("插件仅依赖 sdk（信任边界成立）");
}

fn check_abi() {
    let root = workspace_root();
    let mut rs: Vec<PathBuf> = vec![];
    walk(&root.join("core"), &mut rs);
    walk(&root.join("sdk"), &mut rs);
    walk(&root.join("kernel"), &mut rs);

    // 1) Envelope.deadline 字段声明必须是 Option<Duration>，绝非 Instant（A2）
    let envelope_rs = root.join("core/src/envelope.rs");
    let content = read(&envelope_rs);
    let deadline_line = content
        .lines()
        .find(|l| l.contains("pub deadline"))
        .unwrap_or("");
    if !deadline_line.contains("Option<Duration>") {
        fail(&format!(
            "Envelope.deadline 必须是 Option<Duration>（A2）。当前: '{}'",
            deadline_line.trim()
        ));
    }
    if deadline_line.contains("Instant") {
        fail("Envelope.deadline 字段使用了 Instant（A2 禁止跨 ABI 的进程本地时钟）");
    }
    ok("Envelope.deadline == Option<Duration>（A2 字段级校验通过）");

    // 2) 内核句柄禁止 Arc<Mutex<dyn Plugin>>（A1）
    let mut violations = vec![];
    for f in &rs {
        let c = strip_comments(&read(f));
        if c.contains("Arc<Mutex<dyn Plugin>>") || c.contains("Arc<Mutex<dyn plugin::Plugin>>") {
            violations.push(format!("{}: Arc<Mutex<dyn Plugin>> 禁止（A1）", f.display()));
        }
        if c.contains("switch_generation") {
            violations.push(format!("{}: switch_generation 已废弃，改用 compare_switch（A4）", f.display()));
        }
    }
    // 3) sdk 内禁止 unimplemented!() / todo!()（C2）
    for f in &rs {
        if f.to_string_lossy().contains("/sdk/") {
            let c = strip_comments(&read(f));
            if c.contains("unimplemented!()") || c.contains("todo!()") {
                violations.push(format!("{}: sdk 内出现 unimplemented!/todo!()（C2 禁止）", f.display()));
            }
        }
    }
    if !violations.is_empty() {
        for v in &violations {
            eprintln!("✗ {v}");
        }
        exit(1);
    }
    ok("无 A1/A2/A4/C2 违例（字段级 check-abi 通过）");
}

fn check_deps() {
    let root = workspace_root();
    let plugins = root.join("plugins");
    if !plugins.is_dir() {
        ok("无插件目录，跳过 deps");
        return;
    }
    for entry in std::fs::read_dir(&plugins).unwrap().flatten() {
        let pdir = entry.path();
        if !pdir.is_dir() {
            continue;
        }
        let toml = pdir.join("Cargo.toml");
        let content = read(&toml);
        if content.contains("agent-kernel-kernel") {
            fail(&format!(
                "{} 依赖 kernel（C1 信任边界违例）",
                pdir.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    ok("插件依赖图干净（无 kernel 反向依赖）");
}

/// 生成 L1 契约 Schema（plugin-manifest / capability）到 `schema/`。
/// 规范源：docs/05 §3.1。CI 中以"产出与代码一致"为门禁。
fn gen_schema() {
    let schema_dir = workspace_root().join("../schema");
    std::fs::create_dir_all(&schema_dir)
        .unwrap_or_else(|e| fail(&format!("创建 {} 失败: {e}", schema_dir.display())));
    write_file(&schema_dir.join("plugin-manifest.schema.json"), MANIFEST_SCHEMA);
    write_file(&schema_dir.join("capability.schema.json"), CAPABILITY_SCHEMA);
    ok("L1 契约 Schema 已生成 (plugin-manifest / capability)");
}

/// 构建 WASM guest 组件（wasm32-wasip2）。
fn build_wasm() {
    let root = workspace_root();
    let status = std::process::Command::new("cargo")
        .args([
            "build",
            "-p",
            "agent-kernel-wasm-guest",
            "--target",
            "wasm32-wasip2",
            "--release",
        ])
        .current_dir(&root)
        .status()
        .unwrap_or_else(|e| fail(&format!("执行 cargo build 失败: {e}")));
    if !status.success() {
        fail("构建 wasm guest 失败");
    }
    ok("wasm guest 组件已构建: target/wasm32-wasip2/release/agent_kernel_wasm_guest.wasm");
}

/// 构建 dylib 插件（cdylib，供 dlopen 装载；与宿主同编译器契约）。
fn build_dylib() {
    let root = workspace_root();
    let status = std::process::Command::new("cargo")
        .args(["build", "-p", "agent-kernel-echo-dylib"])
        .current_dir(&root)
        .status()
        .unwrap_or_else(|e| fail(&format!("执行 cargo build 失败: {e}")));
    if !status.success() {
        fail("构建 dylib 插件失败");
    }
    ok("dylib 插件已构建: target/debug/echo_dylib.dll");
}

fn write_file(path: &Path, content: &str) {
    std::fs::write(path, content)
        .unwrap_or_else(|e| fail(&format!("写入 {} 失败: {e}", path.display())));
}

const MANIFEST_SCHEMA: &str = r#"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "$id": "https://agent-kernel/schema/plugin-manifest.schema.json",
  "title": "PluginManifest",
  "description": "L1 语义契约：插件清单。deny-by-default。",
  "type": "object",
  "required": ["id", "api_version", "domain"],
  "properties": {
    "id": { "type": "string" },
    "name": { "type": "string" },
    "version": { "type": "string", "pattern": "^[0-9]+\\.[0-9]+\\.[0-9]+$" },
    "api_version": { "type": "string", "pattern": "^[0-9]+\\.[0-9]+\\.[0-9]+$" },
    "domain": { "type": "string", "enum": ["in-process", "wasm", "process"] },
    "entry": { "type": "object", "required": ["path"], "properties": { "path": { "type": "string" } } },
    "capabilities": { "$ref": "capability.schema.json" },
    "depends_on": { "type": "object", "additionalProperties": { "type": "object", "properties": { "version": { "type": "string" }, "kind": { "type": "string", "enum": ["hard", "soft"] } }, "required": ["version"] } },
    "time": { "type": "object", "properties": { "semantics": { "type": "string", "enum": ["serial", "concurrent"] }, "max_inflight": { "type": "integer", "minimum": 1 }, "deadline": { "type": "string" } } },
    "migration": { "type": "object", "properties": { "migratable": { "type": "boolean", "default": false }, "format": { "type": "string" } } }
  },
  "additionalProperties": true
}"#;

const CAPABILITY_SCHEMA: &str = r#"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "$id": "https://agent-kernel/schema/capability.schema.json",
  "title": "CapabilitySet",
  "description": "L1 语义契约：能力声明。deny-by-default——未声明的外部空间一律禁止。",
  "type": "object",
  "properties": {
    "net_outbound": { "type": "array", "items": { "type": "string" } },
    "fs_read": { "type": "array", "items": { "type": "string" } },
    "fs_write": { "type": "array", "items": { "type": "string" } },
    "env_read": { "type": "array", "items": { "type": "string" } },
    "bus_publish": { "type": "array", "items": { "type": "string" } },
    "bus_subscribe": { "type": "array", "items": { "type": "string" } },
    "kernel_spawn": { "type": "boolean" },
    "kernel_shutdown": { "type": "boolean" },
    "clock": { "type": "boolean" },
    "random": { "type": "boolean" }
  },
  "additionalProperties": false
}"#;

/// 去除行注释，避免文档注释中的禁用词（如 `Arc<Mutex<dyn Plugin>>`）造成误报。
fn strip_comments(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(l))
        .collect::<Vec<_>>()
        .join("\n")
}
