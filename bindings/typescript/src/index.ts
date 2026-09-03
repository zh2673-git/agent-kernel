/**
 * agent-kernel TypeScript 插件 SDK（L3 绑定）。
 *
 * 传输：gRPC。`schema/kernel.proto` 由 `@grpc/proto-loader` 在运行时加载
 * （免代码生成，stub 始终与契约文件一致）。
 * 运行：node --experimental-strip-types（Node >= 22.6）或 tsc 编译后运行。
 */
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import * as grpc from "@grpc/grpc-js";
import * as protoLoader from "@grpc/proto-loader";

export interface PluginManifest {
  id: string;
  version: string;
  api_version: string;
}

/** 插件接口（duck typing，与 Rust / Python 侧约定一致）。 */
export interface Plugin {
  manifest(): PluginManifest;
  init(config: unknown): void | Promise<void>;
  onEvent(envelope: Record<string, unknown>): unknown | Promise<unknown>;
  destroy(): void | Promise<void>;
}

const here = path.dirname(fileURLToPath(import.meta.url));
/** 契约唯一源：仓库根 schema/kernel.proto */
const protoPath = path.resolve(here, "../../../schema/kernel.proto");

function parseVersion(v: string): [number | null, number] {
  const parts = String(v).trim().replace(/^v/, "").split(".");
  const major = parts.length > 0 && /^\d+$/.test(parts[0]) ? Number(parts[0]) : null;
  const minor = parts.length > 1 && /^\d+$/.test(parts[1]) ? Number(parts[1]) : 0;
  return [major, minor];
}

/** 启动 gRPC server：绑定 127.0.0.1:0 → stdout 首行打印 PORT=<n> → 阻塞服务。 */
export async function serve(plugin: Plugin, defaultApiVersion = "0.1"): Promise<void> {
  const definition = protoLoader.loadSync(protoPath, {
    keepCase: false,
    defaults: true,
    oneofs: true,
  });
  const pkg = grpc.loadPackageDefinition(definition) as any;
  const service = pkg.agentkernel.v1.PluginService.service;

  const meta = plugin.manifest();
  const apiVersion = String(meta.api_version ?? defaultApiVersion);
  const [pmaj, pmin] = parseVersion(apiVersion);

  const server = new grpc.Server();
  server.addService(service, {
    handshake(call: any, cb: any): void {
      const want = String(call.request?.apiVersion ?? "");
      const [wmaj, wmin] = parseVersion(want);
      const ok = pmaj !== null && pmaj === wmaj && pmin >= wmin;
      cb(null, {
        compatible: ok,
        reason: ok
          ? ""
          : `api_version mismatch: kernel wants ${want}, plugin supports ${apiVersion}`,
      });
    },

    meta(_call: any, cb: any): void {
      cb(null, { id: meta.id, version: meta.version, apiVersion });
    },

    async init(call: any, cb: any): Promise<void> {
      let config: unknown = null;
      const raw = call.request?.configJson;
      if (raw) {
        try {
          config = JSON.parse(raw);
        } catch {
          config = null;
        }
      }
      try {
        await plugin.init(config);
        cb(null, {});
      } catch (e) {
        cb({ code: grpc.status.FAILED_PRECONDITION, message: String(e) });
      }
    },

    /** 一请求一响应（unary）：请求与回复经 idempotencyKey 关联 seq。 */
    async onEvent(call: any, cb: any): Promise<void> {
      const env = call.request;
      let payload: unknown = null;
      if (env?.payload?.length) {
        try {
          payload = JSON.parse(Buffer.from(env.payload).toString("utf8"));
        } catch {
          payload = null;
        }
      }
      let value: unknown;
      try {
        value = await plugin.onEvent({ target: env?.ty ?? "", payload });
      } catch (e) {
        cb({ code: grpc.status.INTERNAL, message: String(e) });
        return;
      }
      cb(null, {
        traceId: env?.traceId ?? "",
        ty: `${env?.ty ?? ""}.ok`,
        payload: Buffer.from(JSON.stringify(value ?? null), "utf8"),
        idempotencyKey: env?.idempotencyKey ?? "",
      });
    },

    snapshot(_call: any, cb: any): void {
      cb({ code: grpc.status.UNIMPLEMENTED, message: "snapshot unsupported in M1" });
    },
    restore(_call: any, cb: any): void {
      cb({ code: grpc.status.UNIMPLEMENTED, message: "restore unsupported in M1" });
    },
    drain(_call: any, cb: any): void {
      cb(null, {});
    },
    start(_call: any, cb: any): void {
      cb(null, {});
    },
    stop(_call: any, cb: any): void {
      cb(null, {});
    },
    async destroy(_call: any, cb: any): Promise<void> {
      try {
        await plugin.destroy();
      } catch {
        // 收尾失败不再上报
      }
      cb(null, {});
    },
    health(_call: any, cb: any): void {
      cb(null, { ok: true, detail: "" });
    },
  });

  const port = await new Promise<number>((resolve, reject) => {
    server.bindAsync(
      "127.0.0.1:0",
      grpc.ServerCredentials.createInsecure(),
      (err, p) => (err ? reject(err) : resolve(p)),
    );
  });
  server.start();
  // 首行 PORT=<n>：内核读取后连接（同步写，确保立即可见）
  fs.writeSync(1, `PORT=${port}\n`);
}
