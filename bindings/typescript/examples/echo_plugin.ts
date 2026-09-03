/**
 * echo 插件（TypeScript）：被内核以 Process 域 spawn。
 *
 * 运行（Node >= 22.6）：
 *   node --experimental-strip-types examples/echo_plugin.ts
 */
import { serve, type Plugin, type PluginManifest } from "../src/index.ts";

const plugin: Plugin = {
  manifest(): PluginManifest {
    return { id: "echo-ts", version: "0.1.0", api_version: "0.1" };
  },
  init(_config: unknown): void {},
  onEvent(envelope: Record<string, unknown>): unknown {
    // envelope 即 core::Envelope 的 JSON 镜像：target/trace_id/priority/deadline/payload
    return { echo: envelope.payload, from: "typescript" };
  },
  destroy(): void {},
};

serve(plugin).catch((e: unknown) => {
  console.error(e);
  process.exit(1);
});
